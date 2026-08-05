//! Filesystem-backed agent store storage.

use std::{
    collections::BTreeSet,
    ops::Bound::{Excluded, Unbounded},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use stratum_core::{
    AgentEvent, AgentId, AgentLocation, AgentRuntimeContext, ChatRole, HistoryPage, HistoryQuery,
    ModelConfig, NewAgentMessage, RuntimeEvent, SessionId, StreamEnvelope, TokenUsage, TurnId,
    TurnRuntimeSnapshot,
};
use stratum_filesystem::{
    CasExpectation, CasUpdateError, Entry, FILESYSTEM_CAS_RETRIES, FileType, Filesystem,
    FilesystemError, VirtualPath, cas_update,
};
use stratum_store::{
    AGENT_STATE_VERSION, AgentState, AgentStatus, AgentStore, MAX_HISTORY_PAGE_SIZE, StoreError,
};

/// Filesystem-backed store for one agent root.
#[derive(Clone)]
pub struct FilesystemAgentStore {
    filesystem: Arc<dyn Filesystem>,
    root: VirtualPath,
}

enum CommitSequenceOutcome {
    Committed(Box<AgentState>),
    Advanced,
}

impl FilesystemAgentStore {
    /// Creates a store rooted at an agent-visible virtual path.
    #[must_use]
    pub fn new(filesystem: Arc<dyn Filesystem>, root: VirtualPath) -> Self {
        Self { filesystem, root }
    }

    /// Creates the initial agent state and message directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the paths are invalid, the store already exists,
    /// or the filesystem cannot create it.
    pub async fn initialize(
        &self,
        agent_id: AgentId,
        name: String,
    ) -> Result<AgentState, StoreError> {
        let state = AgentState::new(agent_id, name);
        self.filesystem
            .create_dir(&self.messages_path()?)
            .await
            .map_err(StoreError::backend)?;
        self.filesystem
            .put(
                &self.agent_path()?,
                encode_agent_state(&state)?,
                CasExpectation::Absent,
            )
            .await
            .map_err(StoreError::backend)?;
        Ok(state)
    }

    /// Creates the initial host-configured agent state and message directory.
    ///
    /// # Errors
    ///
    /// Returns an error when the paths are invalid, the store already exists,
    /// or the filesystem cannot create it.
    pub async fn initialize_with_model_config(
        &self,
        agent_id: AgentId,
        name: String,
        model_config: ModelConfig,
    ) -> Result<AgentState, StoreError> {
        let state = AgentState::new_configured(agent_id, name, model_config);
        self.filesystem
            .create_dir(&self.messages_path()?)
            .await
            .map_err(StoreError::backend)?;
        self.filesystem
            .put(
                &self.agent_path()?,
                encode_agent_state(&state)?,
                CasExpectation::Absent,
            )
            .await
            .map_err(StoreError::backend)?;
        Ok(state)
    }

    fn agent_path(&self) -> Result<VirtualPath, StoreError> {
        self.child_path("agent.json")
    }

    fn messages_path(&self) -> Result<VirtualPath, StoreError> {
        self.child_path("messages")
    }

    fn message_path(&self, seq: u64) -> Result<VirtualPath, StoreError> {
        self.child_path(&format!("messages/{seq}.json"))
    }

    fn child_path(&self, suffix: &str) -> Result<VirtualPath, StoreError> {
        let path = if self.root.as_str() == "/" {
            format!("/{suffix}")
        } else {
            format!("{}/{suffix}", self.root.as_str())
        };
        VirtualPath::try_from(path.as_str()).map_err(|source| {
            StoreError::backend(FilesystemError::InvalidVirtualPath { path, source })
        })
    }

    async fn read_state(&self) -> Result<AgentState, StoreError> {
        let path = self.agent_path()?;
        let Some(record) = self
            .filesystem
            .get(&path)
            .await
            .map_err(StoreError::backend)?
        else {
            return Err(StoreError::AgentMissing);
        };
        let state = decode_agent_state(&record.entry)?;
        validate_persisted_state(&state)?;
        Ok(state)
    }

    async fn read_message(
        &self,
        state: &AgentState,
        seq: u64,
    ) -> Result<Option<StreamEnvelope>, StoreError> {
        let Some(record) = self
            .filesystem
            .get(&self.message_path(seq)?)
            .await
            .map_err(StoreError::backend)?
        else {
            return Ok(None);
        };
        let envelope = decode_message(&record.entry).inspect_err(|_| {
            trace_store_corruption(state, seq);
        })?;
        validate_message(&envelope, state.agent_id, seq, None, None, None).inspect_err(|_| {
            trace_store_corruption(state, seq);
        })?;
        Ok(Some(envelope))
    }

    async fn commit_sequence(
        &self,
        expected_previous: u64,
        seq: u64,
    ) -> Result<CommitSequenceOutcome, StoreError> {
        let updated_at = Utc::now();
        let attempts = AtomicUsize::new(0);
        let result = cas_update(
            self.filesystem.as_ref(),
            &self.agent_path()?,
            decode_agent_state,
            encode_agent_state,
            |current| {
                attempts.fetch_add(1, Ordering::Relaxed);
                validate_persisted_state(current)?;
                if current.last_seq != expected_previous && current.last_seq != seq {
                    return Err(StoreError::MessageBeyondFrontier {
                        seq,
                        frontier: current.last_seq,
                    });
                }
                let mut next = current.clone();
                next.last_seq = seq;
                next.updated_at = updated_at;
                Ok(next)
            },
        )
        .await;
        trace_cas_outcome(&result, attempts.load(Ordering::Relaxed), Some(seq));
        match result {
            Ok(state) => Ok(CommitSequenceOutcome::Committed(Box::new(state))),
            Err(CasUpdateError::Apply(StoreError::MessageBeyondFrontier {
                seq: attempted,
                frontier,
            })) if attempted == seq && frontier > seq => Ok(CommitSequenceOutcome::Advanced),
            Err(error) => Err(cas_store_error(error)),
        }
    }

    async fn reconcile_frontier(
        &self,
        state: AgentState,
        message_sequences: &BTreeSet<u64>,
    ) -> Result<Option<AgentState>, StoreError> {
        let frontier = self
            .validate_integrity_snapshot(&state, message_sequences)
            .await?;
        if !message_sequences.contains(&frontier) {
            return Ok(Some(state));
        }
        let frontier_message = self
            .read_message(&state, frontier)
            .await?
            .ok_or(StoreError::MissingCommittedMessage { seq: frontier })?;
        validate_message(
            &frontier_message,
            state.agent_id,
            frontier,
            None,
            None,
            None,
        )
        .inspect_err(|_| trace_store_corruption(&state, frontier))?;

        tracing::info!(
            agent_id = %state.agent_id,
            session_id = ?state.session_id,
            turn_id = ?state.turn_id,
            seq = frontier,
            reconciliation_count = 1_u64,
            "store frontier reconciliation"
        );
        match self.commit_sequence(state.last_seq, frontier).await? {
            CommitSequenceOutcome::Committed(state) => Ok(Some(*state)),
            CommitSequenceOutcome::Advanced => Ok(None),
        }
    }

    async fn list_message_sequences(&self) -> Result<BTreeSet<u64>, StoreError> {
        let entries = self
            .filesystem
            .list_dir(&self.messages_path()?)
            .await
            .map_err(StoreError::backend)?;
        let mut sequences = BTreeSet::new();
        for entry in entries {
            let seq = parse_message_filename(&entry.file_name)?;
            if entry.file_type != FileType::File || entry.path != self.message_path(seq)? {
                return Err(StoreError::InvalidMessageFilename {
                    file_name: entry.file_name,
                });
            }
            sequences.insert(seq);
        }
        Ok(sequences)
    }

    async fn read_integrity_snapshot(&self) -> Result<(AgentState, BTreeSet<u64>), StoreError> {
        loop {
            let state = self.read_state().await?;
            let message_sequences = self.list_message_sequences().await?;
            let refreshed = self.read_state().await?;
            if refreshed.last_seq == state.last_seq {
                return Ok((refreshed, message_sequences));
            }
        }
    }

    async fn validate_integrity_snapshot(
        &self,
        state: &AgentState,
        message_sequences: &BTreeSet<u64>,
    ) -> Result<u64, StoreError> {
        let frontier = state
            .last_seq
            .checked_add(1)
            .ok_or(StoreError::SequenceOverflow)?;
        if let Some(seq) = message_sequences
            .range((Excluded(frontier), Unbounded))
            .next()
        {
            trace_store_corruption(state, *seq);
            return Err(StoreError::MessageBeyondFrontier {
                seq: *seq,
                frontier,
            });
        }
        self.validate_committed_messages(state).await?;
        Ok(frontier)
    }

    async fn validate_committed_messages(&self, state: &AgentState) -> Result<(), StoreError> {
        for seq in 1..=state.last_seq {
            if self.read_message(state, seq).await?.is_none() {
                trace_store_corruption(state, seq);
                return Err(StoreError::MissingCommittedMessage { seq });
            }
        }
        Ok(())
    }
}

#[async_trait]
impl AgentStore for FilesystemAgentStore {
    async fn load_agent(&self) -> Result<AgentState, StoreError> {
        loop {
            let (state, message_sequences) = self.read_integrity_snapshot().await?;
            if let Some(state) = self.reconcile_frontier(state, &message_sequences).await? {
                return Ok(state);
            }
        }
    }

    async fn update_state(
        &self,
        status: AgentStatus,
        session_id: Option<SessionId>,
        turn_id: Option<TurnId>,
        usage: TokenUsage,
    ) -> Result<AgentState, StoreError> {
        self.load_agent().await?;
        let updated_at = Utc::now();
        let attempts = AtomicUsize::new(0);
        let result = cas_update(
            self.filesystem.as_ref(),
            &self.agent_path()?,
            decode_agent_state,
            encode_agent_state,
            |current| {
                attempts.fetch_add(1, Ordering::Relaxed);
                validate_runtime_state(current)?;
                if status == AgentStatus::Running
                    && current.status == AgentStatus::Running
                    && (current.session_id != session_id || current.turn_id != turn_id)
                {
                    return Err(StoreError::RunningSessionConflict {
                        current: current.session_id,
                        attempted: session_id,
                    });
                }
                let mut next = current.clone();
                next.status = status;
                next.session_id = session_id;
                next.turn_id = turn_id;
                next.usage = usage;
                next.updated_at = updated_at;
                Ok(next)
            },
        )
        .await;
        trace_cas_outcome(&result, attempts.load(Ordering::Relaxed), None);
        result.map_err(cas_store_error)
    }

    async fn start_turn(
        &self,
        context: &AgentRuntimeContext,
        turn_id: TurnId,
        runtime_snapshot: TurnRuntimeSnapshot,
    ) -> Result<AgentState, StoreError> {
        self.load_agent().await?;
        let updated_at = Utc::now();
        let attempts = AtomicUsize::new(0);
        let result = cas_update(
            self.filesystem.as_ref(),
            &self.agent_path()?,
            decode_agent_state,
            encode_agent_state,
            |current| {
                attempts.fetch_add(1, Ordering::Relaxed);
                validate_persisted_state(current)?;
                if current.status == AgentStatus::Running
                    && (current.session_id != Some(context.session_id)
                        || current.turn_id != Some(turn_id))
                {
                    return Err(StoreError::RunningSessionConflict {
                        current: current.session_id,
                        attempted: Some(context.session_id),
                    });
                }
                validate_runtime_snapshot(current, &runtime_snapshot)?;
                let mut next = current.clone();
                next.status = AgentStatus::Running;
                next.session_id = Some(context.session_id);
                next.turn_id = Some(turn_id);
                next.location = Some(context.location.clone());
                next.turn_runtime_snapshot = Some(runtime_snapshot.clone());
                next.next_iteration = 0;
                next.usage = TokenUsage::default();
                next.model_config = Some(runtime_snapshot.model.clone());
                next.updated_at = updated_at;
                Ok(next)
            },
        )
        .await;
        trace_cas_outcome(&result, attempts.load(Ordering::Relaxed), None);
        result.map_err(cas_store_error)
    }

    async fn complete_iteration(
        &self,
        session_id: SessionId,
        turn_id: TurnId,
        iteration: u64,
        usage: TokenUsage,
    ) -> Result<AgentState, StoreError> {
        let updated_at = Utc::now();
        let attempts = AtomicUsize::new(0);
        let result = cas_update(
            self.filesystem.as_ref(),
            &self.agent_path()?,
            decode_agent_state,
            encode_agent_state,
            |current| {
                attempts.fetch_add(1, Ordering::Relaxed);
                validate_runtime_state(current)?;
                if current.status != AgentStatus::Running {
                    return Err(StoreError::AgentNotRunning {
                        actual: current.status,
                    });
                }
                if current.session_id != Some(session_id) {
                    return Err(StoreError::SessionMismatch {
                        expected: current.session_id.unwrap_or(session_id),
                        actual: session_id,
                    });
                }
                if current.turn_id != Some(turn_id) {
                    return Err(StoreError::TurnMismatch {
                        expected: current.turn_id.unwrap_or(turn_id),
                        actual: turn_id,
                    });
                }
                if current.next_iteration != iteration {
                    return Err(StoreError::IterationMismatch {
                        expected: current.next_iteration,
                        actual: iteration,
                    });
                }
                let mut next = current.clone();
                next.next_iteration = iteration
                    .checked_add(1)
                    .ok_or(StoreError::IterationOverflow)?;
                next.usage = usage;
                next.updated_at = updated_at;
                Ok(next)
            },
        )
        .await;
        trace_cas_outcome(&result, attempts.load(Ordering::Relaxed), None);
        result.map_err(cas_store_error)
    }

    async fn append_message(&self, message: NewAgentMessage) -> Result<StreamEnvelope, StoreError> {
        validate_message_role(message.message.role)?;
        let input_agent_id = message.agent_id;
        let session_id = message.session_id;
        let turn_id = message.turn_id;
        let location = message.location.clone();
        for append_attempt in 1..=FILESYSTEM_CAS_RETRIES {
            let state = self.read_state().await?;
            if input_agent_id != state.agent_id {
                return Err(StoreError::AgentMismatch {
                    expected: state.agent_id,
                    actual: input_agent_id,
                });
            }
            let seq = state
                .last_seq
                .checked_add(1)
                .ok_or(StoreError::SequenceOverflow)?;
            let beyond = seq.checked_add(1).ok_or(StoreError::SequenceOverflow)?;
            let committed = message.clone().into_envelope(seq);
            validate_message(
                &committed,
                state.agent_id,
                seq,
                Some(session_id),
                Some(turn_id),
                Some(&location),
            )?;

            let existing = self.read_message(&state, seq).await?;
            if self.read_message(&state, beyond).await?.is_some() {
                let refreshed = self.read_state().await?;
                if refreshed.last_seq != state.last_seq {
                    continue;
                }
                trace_store_corruption(&state, beyond);
                return Err(StoreError::MessageBeyondFrontier {
                    seq: beyond,
                    frontier: seq,
                });
            }
            if let Some(existing) = existing {
                validate_message(
                    &existing,
                    state.agent_id,
                    seq,
                    Some(session_id),
                    Some(turn_id),
                    Some(&location),
                )
                .inspect_err(|_| trace_store_corruption(&state, seq))?;
                self.commit_sequence(state.last_seq, seq).await?;
                if existing == committed {
                    return Ok(existing);
                }
                tracing::info!(
                    agent_id = %state.agent_id,
                    session_id = %session_id,
                    turn_id = %turn_id,
                    seq,
                    retry_count = append_attempt,
                    "store append retry"
                );
                continue;
            }

            match self
                .filesystem
                .put(
                    &self.message_path(seq)?,
                    encode_message(&committed)?,
                    CasExpectation::Absent,
                )
                .await
            {
                Ok(_) => {
                    self.commit_sequence(state.last_seq, seq).await?;
                    return Ok(committed);
                }
                Err(FilesystemError::VersionMismatch { .. }) => {
                    continue;
                }
                Err(error) => return Err(StoreError::backend(error)),
            }
        }
        Err(StoreError::CasRetriesExhausted)
    }

    async fn history_page(&self, query: HistoryQuery) -> Result<HistoryPage, StoreError> {
        let started = std::time::Instant::now();
        if query.limit == 0 || query.limit > MAX_HISTORY_PAGE_SIZE {
            return Err(StoreError::InvalidHistoryLimit {
                actual: query.limit,
                maximum: MAX_HISTORY_PAGE_SIZE,
            });
        }
        let state = self.read_state().await?;
        let through_seq = query.through_seq.unwrap_or(state.last_seq);
        if through_seq > state.last_seq {
            return Err(StoreError::HistoryBarrierBeyondLast {
                through_seq,
                last_seq: state.last_seq,
            });
        }
        if query.after_seq > through_seq {
            return Err(StoreError::InvalidHistoryRange {
                after_seq: query.after_seq,
                through_seq,
            });
        }

        let available = through_seq - query.after_seq;
        let count = available.min(u64::try_from(query.limit).expect("history limit fits u64"));
        let mut events = Vec::with_capacity(usize::try_from(count).expect("page size fits usize"));
        for offset in 1..=count {
            let seq = query
                .after_seq
                .checked_add(offset)
                .ok_or(StoreError::SequenceOverflow)?;
            let event = self
                .read_message(&state, seq)
                .await?
                .ok_or(StoreError::MissingCommittedMessage { seq })?;
            events.push(event);
        }
        let next_front_seq = events
            .last()
            .and_then(StreamEnvelope::message_seq)
            .unwrap_or(query.after_seq);
        let page = HistoryPage {
            through_seq,
            events,
            next_front_seq,
            has_more: next_front_seq < through_seq,
        };
        tracing::info!(
            agent_id = %state.agent_id,
            session_id = ?state.session_id,
            turn_id = ?state.turn_id,
            seq = page.next_front_seq,
            event_count = page.events.len(),
            latency_micros = started.elapsed().as_micros(),
            "store history page"
        );
        Ok(page)
    }
}

fn decode_agent_state(entry: &Entry) -> Result<AgentState, StoreError> {
    serde_json::from_slice(entry.contents()).map_err(StoreError::DecodeState)
}

fn encode_agent_state(state: &AgentState) -> Result<Entry, StoreError> {
    serde_json::to_vec(state)
        .map(Entry::new)
        .map_err(StoreError::Encode)
}

fn decode_message(entry: &Entry) -> Result<StreamEnvelope, StoreError> {
    let value: Value =
        serde_json::from_slice(entry.contents()).map_err(StoreError::DecodeMessage)?;
    validate_strict_message_json(&value).map_err(StoreError::DecodeMessage)?;
    serde_json::from_value(value).map_err(StoreError::DecodeMessage)
}

fn encode_message(envelope: &StreamEnvelope) -> Result<Entry, StoreError> {
    serde_json::to_vec(envelope)
        .map(Entry::new)
        .map_err(StoreError::Encode)
}

fn validate_persisted_state(state: &AgentState) -> Result<(), StoreError> {
    if state.state_version != AGENT_STATE_VERSION {
        return Err(StoreError::UnsupportedStateVersion {
            version: state.state_version,
        });
    }
    state
        .model_config
        .as_ref()
        .ok_or(StoreError::MissingModelConfig)?;
    Ok(())
}

fn validate_runtime_state(state: &AgentState) -> Result<(), StoreError> {
    validate_persisted_state(state)?;
    if state.model_config.is_none() {
        return Err(StoreError::MissingModelConfig);
    }
    Ok(())
}

fn validate_runtime_snapshot(
    state: &AgentState,
    snapshot: &TurnRuntimeSnapshot,
) -> Result<(), StoreError> {
    let mismatch = if state.agent_version_id != snapshot.agent_version_id {
        Some("agent_version")
    } else if state.skill_set_version_id != snapshot.skill_set_version_id {
        Some("skill_set_version")
    } else if state.extension_set_version_id != snapshot.extension_set_version_id {
        Some("extension_set_version")
    } else if state.hook_handler_versions != snapshot.hook_handler_versions {
        Some("hook_handler_order")
    } else {
        None
    };
    mismatch.map_or(Ok(()), |component| {
        Err(StoreError::RuntimeSnapshotMismatch { component })
    })
}

fn parse_message_filename(file_name: &str) -> Result<u64, StoreError> {
    let Some(number) = file_name.strip_suffix(".json") else {
        return Err(StoreError::InvalidMessageFilename {
            file_name: file_name.to_owned(),
        });
    };
    let seq = number
        .parse::<u64>()
        .ok()
        .filter(|seq| *seq != 0 && number == seq.to_string())
        .ok_or_else(|| StoreError::InvalidMessageFilename {
            file_name: file_name.to_owned(),
        })?;
    Ok(seq)
}

fn validate_message(
    envelope: &StreamEnvelope,
    expected_agent_id: AgentId,
    path_seq: u64,
    expected_session_id: Option<SessionId>,
    expected_turn_id: Option<TurnId>,
    expected_location: Option<&AgentLocation>,
) -> Result<(), StoreError> {
    if let Some(expected) = expected_session_id
        && envelope.session_id != expected
    {
        return Err(StoreError::SessionMismatch {
            expected,
            actual: envelope.session_id,
        });
    }
    let RuntimeEvent::Agent {
        agent_id,
        turn_id,
        location,
        event,
    } = &envelope.event
    else {
        return Err(StoreError::UnexpectedMessageEvent);
    };
    if *agent_id != expected_agent_id {
        return Err(StoreError::AgentMismatch {
            expected: expected_agent_id,
            actual: *agent_id,
        });
    }
    let AgentEvent::Message {
        message_seq,
        message,
    } = event
    else {
        return Err(StoreError::UnexpectedMessageEvent);
    };
    validate_message_role(message.role)?;
    if *message_seq != path_seq {
        return Err(StoreError::MessageSequenceMismatch {
            path_seq,
            event_seq: *message_seq,
        });
    }
    if let Some(expected) = expected_turn_id
        && *turn_id != expected
    {
        return Err(StoreError::TurnMismatch {
            expected,
            actual: *turn_id,
        });
    }
    if let Some(expected) = expected_location
        && location != expected
    {
        return Err(StoreError::LocationMismatch {
            expected: expected.clone(),
            actual: location.clone(),
        });
    }
    Ok(())
}

fn validate_message_role(role: ChatRole) -> Result<(), StoreError> {
    match role {
        ChatRole::User | ChatRole::Assistant | ChatRole::Tool => Ok(()),
        role => Err(StoreError::InvalidMessageRole { role }),
    }
}

fn validate_strict_message_json(value: &Value) -> Result<(), serde_json::Error> {
    let envelope = strict_object(value)?;
    strict_keys(
        envelope,
        &["session_id", "timestamp", "event"],
        &["session_id", "timestamp", "event", "metadata"],
    )?;

    let runtime_event = strict_object(&envelope["event"])?;
    strict_keys(runtime_event, &["type", "data"], &["type", "data"])?;
    if runtime_event["type"] != "agent" {
        return Err(strict_json_error());
    }
    let runtime_data = strict_object(&runtime_event["data"])?;
    strict_keys(
        runtime_data,
        &["agent_id", "turn_id", "location", "event"],
        &["agent_id", "turn_id", "location", "event"],
    )?;
    validate_strict_location(&runtime_data["location"])?;

    let agent_event = strict_object(&runtime_data["event"])?;
    strict_keys(agent_event, &["type", "data"], &["type", "data"])?;
    if agent_event["type"] != "message" {
        return Err(strict_json_error());
    }
    let message_data = strict_object(&agent_event["data"])?;
    strict_keys(
        message_data,
        &["message_seq", "message"],
        &["message_seq", "message"],
    )?;
    validate_strict_chat_message(&message_data["message"])
}

fn validate_strict_location(value: &Value) -> Result<(), serde_json::Error> {
    let location = strict_object(value)?;
    let Some(location_type) = location.get("type").and_then(Value::as_str) else {
        return Err(strict_json_error());
    };
    match location_type {
        "direct" => strict_keys(location, &["type"], &["type"]),
        "workflow_node" => {
            strict_keys(location, &["type", "data"], &["type", "data"])?;
            strict_keys(
                strict_object(&location["data"])?,
                &["workflow_version_id", "node_id"],
                &["workflow_version_id", "node_id"],
            )
        }
        _ => Err(strict_json_error()),
    }
}

fn validate_strict_chat_message(value: &Value) -> Result<(), serde_json::Error> {
    let message = strict_object(value)?;
    strict_keys(
        message,
        &["role", "content"],
        &[
            "role",
            "content",
            "tool_calls",
            "reasoning_content",
            "tool_call_id",
        ],
    )?;
    let content = strict_object(&message["content"])?;
    strict_keys(content, &["type", "data"], &["type", "data"])?;
    if !matches!(content["type"].as_str(), Some("text" | "json")) {
        return Err(strict_json_error());
    }
    if let Some(tool_calls) = message.get("tool_calls") {
        let Some(tool_calls) = tool_calls.as_array() else {
            return Err(strict_json_error());
        };
        for tool_call in tool_calls {
            strict_keys(
                strict_object(tool_call)?,
                &["call_id", "name", "arguments"],
                &["call_id", "name", "arguments"],
            )?;
        }
    }
    Ok(())
}

fn strict_object(value: &Value) -> Result<&serde_json::Map<String, Value>, serde_json::Error> {
    value.as_object().ok_or_else(strict_json_error)
}

fn strict_keys(
    object: &serde_json::Map<String, Value>,
    required: &[&str],
    allowed: &[&str],
) -> Result<(), serde_json::Error> {
    if required.iter().any(|key| !object.contains_key(*key))
        || object.keys().any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(strict_json_error());
    }
    Ok(())
}

fn strict_json_error() -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "invalid strict store message shape",
    ))
}

fn trace_cas_outcome<E>(
    result: &Result<AgentState, CasUpdateError<E>>,
    attempt_count: usize,
    seq: Option<u64>,
) where
    E: std::error::Error + 'static,
{
    let state = result.as_ref().ok();
    let agent_id = state.map(|state| state.agent_id);
    let session_id = state.and_then(|state| state.session_id);
    let turn_id = state.and_then(|state| state.turn_id);
    if attempt_count > 1 {
        tracing::warn!(
            agent_id = ?agent_id,
            session_id = ?session_id,
            turn_id = ?turn_id,
            seq = ?seq,
            retry_count = attempt_count - 1,
            "store cas retry"
        );
    }
    if matches!(result, Err(CasUpdateError::RetriesExhausted)) {
        tracing::error!(
            agent_id = ?agent_id,
            session_id = ?session_id,
            turn_id = ?turn_id,
            seq = ?seq,
            attempt_count,
            exhaustion_count = 1_u64,
            "store cas retries exhausted"
        );
    }
}

fn trace_store_corruption(state: &AgentState, seq: u64) {
    tracing::error!(
        agent_id = %state.agent_id,
        session_id = ?state.session_id,
        turn_id = ?state.turn_id,
        seq,
        corruption_count = 1_u64,
        "store corruption"
    );
}

fn cas_store_error(error: CasUpdateError<StoreError>) -> StoreError {
    match error {
        CasUpdateError::CasUnsupported => StoreError::CasUnsupported,
        CasUpdateError::Timeout => StoreError::CasTimeout,
        CasUpdateError::RetriesExhausted => StoreError::CasRetriesExhausted,
        CasUpdateError::Filesystem(source) => StoreError::backend(source),
        CasUpdateError::Apply(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use stratum_filesystem::{CasUpdateError, FilesystemError};

    use super::*;

    #[test]
    fn cas_update_errors_map_to_store_domain_errors() {
        let unsupported = cas_store_error(CasUpdateError::<StoreError>::CasUnsupported);
        let timeout = cas_store_error(CasUpdateError::<StoreError>::Timeout);
        let exhausted = cas_store_error(CasUpdateError::<StoreError>::RetriesExhausted);
        let filesystem = cas_store_error(CasUpdateError::<StoreError>::Filesystem(
            FilesystemError::UnsupportedCas,
        ));
        let apply = cas_store_error(CasUpdateError::Apply(StoreError::SequenceOverflow));

        assert!(matches!(unsupported, StoreError::CasUnsupported));
        assert!(matches!(timeout, StoreError::CasTimeout));
        assert!(matches!(exhausted, StoreError::CasRetriesExhausted));
        assert!(matches!(
            filesystem,
            StoreError::Backend(source)
                if matches!(
                    source.downcast_ref::<FilesystemError>(),
                    Some(FilesystemError::UnsupportedCas)
                )
        ));
        assert!(matches!(apply, StoreError::SequenceOverflow));
    }
}
