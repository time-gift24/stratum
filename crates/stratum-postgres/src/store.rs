//! Postgres-backed agent store storage.
//!
//! One row in `agent_state` carries the persisted runtime state of one agent;
//! committed messages are append-only rows in `agent_messages`, keyed by
//! `(agent_id, message_seq)`. `append_message` allocates the sequence and
//! inserts the message row in a single transaction, so the two always commit
//! or roll back together — unlike the filesystem backend's two-phase
//! write-then-CAS, Postgres has no crash window to reconcile, so `load_agent`
//! is a plain read.
//!
//! Precondition checks mirror the filesystem backend variant for variant. Each
//! mutating method first reads the state row and runs the same ordered
//! validations as the filesystem CAS apply closure (yielding identical
//! `StoreError` variants), then issues a conditional UPDATE whose WHERE clause
//! re-expresses the mutable preconditions as the atomic guard. Zero affected
//! rows means a concurrent writer won the race; the method re-reads the row
//! and re-runs the ordered validation to classify the precise error.

use async_trait::async_trait;
use chrono::Utc;
use serde_json::Value;
use sqlx::{PgPool, Row, postgres::PgRow};
use stratum_core::{
    AgentEvent, AgentId, AgentLocation, AgentRuntimeContext, ChatRole, HistoryPage, HistoryQuery,
    ModelConfig, NewAgentMessage, RuntimeEvent, SessionId, StreamEnvelope, TokenUsage, TurnId,
    TurnRuntimeSnapshot,
};
use stratum_store::{
    AGENT_STATE_VERSION, AgentState, AgentStatus, AgentStore, MAX_HISTORY_PAGE_SIZE, StoreError,
};
use uuid::Uuid;

use crate::tx::run_in_transaction;

/// Columns of one `agent_state` row, shared by SELECT and RETURNING clauses.
const STATE_COLUMNS: &str = "agent_id, state_version, name, agent_version_id, \
    skill_set_version_id, extension_set_version_id, hook_handler_versions, model_config, status, \
    session_id, active_turn_id, location, runtime_snapshot, next_iteration, usage, \
    next_message_seq, updated_at";

/// Postgres-backed store for one agent identity.
#[derive(Clone)]
pub struct PostgresAgentStore {
    pool: PgPool,
    agent_id: AgentId,
}

impl PostgresAgentStore {
    /// Creates a store bound to one agent identity.
    #[must_use]
    pub const fn new(pool: PgPool, agent_id: AgentId) -> Self {
        Self { pool, agent_id }
    }

    /// Creates the initial agent state row.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent id does not match the store's identity,
    /// the state row already exists, or the insert cannot be committed.
    pub async fn initialize(
        &self,
        agent_id: AgentId,
        name: String,
    ) -> Result<AgentState, StoreError> {
        self.insert_state(AgentState::new(agent_id, name)).await
    }

    /// Creates the initial host-configured agent state row.
    ///
    /// # Errors
    ///
    /// Returns an error when the agent id does not match the store's identity,
    /// the state row already exists, or the insert cannot be committed.
    pub async fn initialize_with_model_config(
        &self,
        agent_id: AgentId,
        name: String,
        model_config: ModelConfig,
    ) -> Result<AgentState, StoreError> {
        self.insert_state(AgentState::new_configured(agent_id, name, model_config))
            .await
    }

    async fn insert_state(&self, state: AgentState) -> Result<AgentState, StoreError> {
        if state.agent_id != self.agent_id {
            return Err(StoreError::AgentMismatch {
                expected: self.agent_id,
                actual: state.agent_id,
            });
        }
        sqlx::query(
            "INSERT INTO agent_state (agent_id, state_version, name, agent_version_id, \
                skill_set_version_id, extension_set_version_id, hook_handler_versions, \
                model_config, status, session_id, active_turn_id, location, runtime_snapshot, \
                next_iteration, usage, next_message_seq, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)",
        )
        .bind(state.agent_id.as_uuid())
        .bind(i32::try_from(state.state_version).map_err(StoreError::backend)?)
        .bind(&state.name)
        .bind(state.agent_version_id.as_uuid())
        .bind(state.skill_set_version_id.as_uuid())
        .bind(state.extension_set_version_id.as_uuid())
        .bind(encode_json(&state.hook_handler_versions)?)
        .bind(state.model_config.as_ref().map(encode_json).transpose()?)
        .bind(status_str(state.status))
        .bind(state.session_id.map(SessionId::as_uuid))
        .bind(state.turn_id.map(TurnId::as_uuid))
        .bind(state.location.as_ref().map(encode_json).transpose()?)
        .bind(
            state
                .turn_runtime_snapshot
                .as_ref()
                .map(encode_json)
                .transpose()?,
        )
        .bind(i64::try_from(state.next_iteration).map_err(StoreError::backend)?)
        .bind(encode_json(&state.usage)?)
        .bind(i64::try_from(state.last_seq).map_err(StoreError::backend)?)
        .bind(state.updated_at)
        .execute(&self.pool)
        .await
        .map_err(StoreError::backend)?;
        Ok(state)
    }

    /// Reads and decodes the state row without validating its contents.
    async fn read_state(&self) -> Result<AgentState, StoreError> {
        let row = sqlx::query(&format!(
            "SELECT {STATE_COLUMNS} FROM agent_state WHERE agent_id = $1"
        ))
        .bind(self.agent_id.as_uuid())
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::backend)?;
        let Some(row) = row else {
            return Err(StoreError::AgentMissing);
        };
        decode_state_row(&row)
    }
}

#[async_trait]
impl AgentStore for PostgresAgentStore {
    async fn load_agent(&self) -> Result<AgentState, StoreError> {
        // Single-transaction writes leave no crash window, so unlike the
        // filesystem backend there is no frontier reconciliation here.
        let state = self.read_state().await?;
        validate_persisted_state(&state)?;
        Ok(state)
    }

    async fn update_state(
        &self,
        status: AgentStatus,
        session_id: Option<SessionId>,
        turn_id: Option<TurnId>,
        usage: TokenUsage,
    ) -> Result<AgentState, StoreError> {
        let current = self.read_state().await?;
        validate_runtime_state(&current)?;
        let updated_at = Utc::now();
        let row = sqlx::query(&format!(
            "UPDATE agent_state \
             SET status = $2, session_id = $3, active_turn_id = $4, usage = $5, updated_at = $6 \
             WHERE agent_id = $1 \
               AND ($2 <> 'running' OR status <> 'running' \
                    OR (session_id IS NOT DISTINCT FROM $3 \
                        AND active_turn_id IS NOT DISTINCT FROM $4)) \
             RETURNING {STATE_COLUMNS}"
        ))
        .bind(self.agent_id.as_uuid())
        .bind(status_str(status))
        .bind(session_id.map(SessionId::as_uuid))
        .bind(turn_id.map(TurnId::as_uuid))
        .bind(encode_json(&usage)?)
        .bind(updated_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::backend)?;
        let Some(row) = row else {
            // A concurrent writer flipped the row between the read and the
            // conditional update; classify from a fresh read.
            let current = self.read_state().await?;
            validate_runtime_state(&current)?;
            return Err(running_session_conflict(&current, session_id, turn_id));
        };
        decode_state_row(&row)
    }

    async fn start_turn(
        &self,
        context: &AgentRuntimeContext,
        turn_id: TurnId,
        runtime_snapshot: TurnRuntimeSnapshot,
    ) -> Result<AgentState, StoreError> {
        let current = self.read_state().await?;
        validate_persisted_state(&current)?;
        if current.status == AgentStatus::Running
            && (current.session_id != Some(context.session_id) || current.turn_id != Some(turn_id))
        {
            return Err(StoreError::RunningSessionConflict {
                current: current.session_id,
                attempted: Some(context.session_id),
            });
        }
        validate_runtime_snapshot(&current, &runtime_snapshot)?;
        let updated_at = Utc::now();
        let row = sqlx::query(&format!(
            "UPDATE agent_state \
             SET status = 'running', session_id = $2, active_turn_id = $3, location = $4, \
                 runtime_snapshot = $5, next_iteration = 0, usage = $6, model_config = $7, \
                 updated_at = $8 \
             WHERE agent_id = $1 \
               AND NOT (status = 'running' \
                        AND (session_id IS DISTINCT FROM $2 \
                             OR active_turn_id IS DISTINCT FROM $3)) \
             RETURNING {STATE_COLUMNS}"
        ))
        .bind(self.agent_id.as_uuid())
        .bind(context.session_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(encode_json(&context.location)?)
        .bind(encode_json(&runtime_snapshot)?)
        .bind(encode_json(&TokenUsage::default())?)
        .bind(encode_json(&runtime_snapshot.model)?)
        .bind(updated_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::backend)?;
        let Some(row) = row else {
            let current = self.read_state().await?;
            validate_persisted_state(&current)?;
            return Err(running_session_conflict(
                &current,
                Some(context.session_id),
                Some(turn_id),
            ));
        };
        decode_state_row(&row)
    }

    async fn complete_iteration(
        &self,
        session_id: SessionId,
        turn_id: TurnId,
        iteration: u64,
        usage: TokenUsage,
    ) -> Result<AgentState, StoreError> {
        let current = self.read_state().await?;
        validate_iteration_precondition(&current, session_id, turn_id, iteration)?;
        let updated_at = Utc::now();
        let row = sqlx::query(&format!(
            "UPDATE agent_state \
             SET next_iteration = next_iteration + 1, usage = $5, updated_at = $6 \
             WHERE agent_id = $1 AND status = 'running' AND session_id = $2 \
               AND active_turn_id = $3 AND next_iteration = $4 \
             RETURNING {STATE_COLUMNS}"
        ))
        .bind(self.agent_id.as_uuid())
        .bind(session_id.as_uuid())
        .bind(turn_id.as_uuid())
        .bind(i64::try_from(iteration).map_err(|_| StoreError::IterationOverflow)?)
        .bind(encode_json(&usage)?)
        .bind(updated_at)
        .fetch_optional(&self.pool)
        .await
        .map_err(StoreError::backend)?;
        let Some(row) = row else {
            let current = self.read_state().await?;
            validate_iteration_precondition(&current, session_id, turn_id, iteration)?;
            return Err(StoreError::backend(PreconditionRace));
        };
        decode_state_row(&row)
    }

    async fn append_message(&self, message: NewAgentMessage) -> Result<StreamEnvelope, StoreError> {
        validate_message_role(message.message.role)?;
        if message.agent_id != self.agent_id {
            return Err(StoreError::AgentMismatch {
                expected: self.agent_id,
                actual: message.agent_id,
            });
        }
        let session_id = message.session_id;
        let turn_id = message.turn_id;
        let location = location_tag(&message.location);
        let updated_at = Utc::now();
        let agent_id = self.agent_id;
        run_in_transaction(&self.pool, async move |connection| {
            let row = sqlx::query(&format!(
                "UPDATE agent_state \
                 SET next_message_seq = next_message_seq + 1, updated_at = $2 \
                 WHERE agent_id = $1 \
                 RETURNING {STATE_COLUMNS}"
            ))
            .bind(agent_id.as_uuid())
            .bind(updated_at)
            .fetch_optional(&mut *connection)
            .await
            .map_err(StoreError::backend)?;
            let Some(row) = row else {
                return Err(StoreError::AgentMissing);
            };
            let state = decode_state_row(&row)?;
            validate_persisted_state(&state)?;
            let seq = state.last_seq;
            let committed = message.into_envelope(seq);
            validate_message(
                &committed,
                agent_id,
                seq,
                Some(session_id),
                Some(turn_id),
                None,
            )?;
            sqlx::query(
                "INSERT INTO agent_messages \
                    (agent_id, message_seq, session_id, turn_id, location, envelope) \
                 VALUES ($1, $2, $3, $4, $5, $6)",
            )
            .bind(agent_id.as_uuid())
            .bind(i64::try_from(seq).map_err(|_| StoreError::SequenceOverflow)?)
            .bind(session_id.as_uuid())
            .bind(turn_id.as_uuid())
            .bind(location)
            .bind(encode_json(&committed)?)
            .execute(&mut *connection)
            .await
            .map_err(StoreError::backend)?;
            Ok(committed)
        })
        .await
    }

    async fn history_page(&self, query: HistoryQuery) -> Result<HistoryPage, StoreError> {
        let started = std::time::Instant::now();
        let state = self.read_state().await?;
        validate_persisted_state(&state)?;
        let range = plan_history_range(&query, state.last_seq)?;
        let rows = sqlx::query(
            "SELECT message_seq, envelope FROM agent_messages \
             WHERE agent_id = $1 AND message_seq > $2 AND message_seq <= $3 \
             ORDER BY message_seq ASC LIMIT $4",
        )
        .bind(self.agent_id.as_uuid())
        .bind(i64::try_from(query.after_seq).map_err(StoreError::backend)?)
        .bind(i64::try_from(range.through_seq).map_err(StoreError::backend)?)
        .bind(i64::try_from(range.count).expect("history page size fits i64"))
        .fetch_all(&self.pool)
        .await
        .map_err(StoreError::backend)?;

        let mut events = Vec::with_capacity(rows.len());
        for (index, row) in rows.iter().enumerate() {
            let expected_seq = query
                .after_seq
                .checked_add(u64::try_from(index + 1).expect("page offset fits u64"))
                .ok_or(StoreError::SequenceOverflow)?;
            let raw_seq: i64 = row.try_get("message_seq").map_err(StoreError::backend)?;
            let row_seq = u64::try_from(raw_seq).map_err(StoreError::backend)?;
            if row_seq != expected_seq {
                return Err(StoreError::MissingCommittedMessage { seq: expected_seq });
            }
            let payload: Value = row.try_get("envelope").map_err(StoreError::backend)?;
            let envelope = decode_message(&payload)?;
            validate_message(&envelope, state.agent_id, row_seq, None, None, None)?;
            events.push(envelope);
        }
        let count = usize::try_from(range.count).expect("history page size fits usize");
        if events.len() < count {
            let missing = query
                .after_seq
                .checked_add(u64::try_from(events.len() + 1).expect("page offset fits u64"))
                .ok_or(StoreError::SequenceOverflow)?;
            return Err(StoreError::MissingCommittedMessage { seq: missing });
        }
        let next_front_seq = events
            .last()
            .and_then(StreamEnvelope::message_seq)
            .unwrap_or(query.after_seq);
        let page = HistoryPage {
            through_seq: range.through_seq,
            events,
            next_front_seq,
            has_more: next_front_seq < range.through_seq,
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

/// Planned bounds of one history page.
struct HistoryRange {
    /// Inclusive upper sequence bound of the fixed range.
    through_seq: u64,
    /// Number of messages to read within the range.
    count: u64,
}

/// Validates the query against the committed frontier and computes the page
/// bounds, mirroring the filesystem backend's checks in the same order.
fn plan_history_range(query: &HistoryQuery, last_seq: u64) -> Result<HistoryRange, StoreError> {
    if query.limit == 0 || query.limit > MAX_HISTORY_PAGE_SIZE {
        return Err(StoreError::InvalidHistoryLimit {
            actual: query.limit,
            maximum: MAX_HISTORY_PAGE_SIZE,
        });
    }
    let through_seq = query.through_seq.unwrap_or(last_seq);
    if through_seq > last_seq {
        return Err(StoreError::HistoryBarrierBeyondLast {
            through_seq,
            last_seq,
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
    Ok(HistoryRange { through_seq, count })
}

/// Runs the filesystem backend's `complete_iteration` apply-closure checks in
/// the same order and with the same error variants.
fn validate_iteration_precondition(
    state: &AgentState,
    session_id: SessionId,
    turn_id: TurnId,
    iteration: u64,
) -> Result<(), StoreError> {
    validate_runtime_state(state)?;
    if state.status != AgentStatus::Running {
        return Err(StoreError::AgentNotRunning {
            actual: state.status,
        });
    }
    if state.session_id != Some(session_id) {
        return Err(StoreError::SessionMismatch {
            expected: state.session_id.unwrap_or(session_id),
            actual: session_id,
        });
    }
    if state.turn_id != Some(turn_id) {
        return Err(StoreError::TurnMismatch {
            expected: state.turn_id.unwrap_or(turn_id),
            actual: turn_id,
        });
    }
    if state.next_iteration != iteration {
        return Err(StoreError::IterationMismatch {
            expected: state.next_iteration,
            actual: iteration,
        });
    }
    iteration
        .checked_add(1)
        .map(|_| ())
        .ok_or(StoreError::IterationOverflow)
}

/// Builds the running-conflict error the filesystem backend produces when a
/// new running transition would replace another active session operation.
fn running_session_conflict(
    state: &AgentState,
    session_id: Option<SessionId>,
    turn_id: Option<TurnId>,
) -> StoreError {
    debug_assert!(
        state.status == AgentStatus::Running
            && (state.session_id != session_id || state.turn_id != turn_id)
    );
    StoreError::RunningSessionConflict {
        current: state.session_id,
        attempted: session_id,
    }
}

/// Marker backend error for a conditional-update race that no longer fails
/// any precondition on re-read; effectively unreachable.
#[derive(Debug, thiserror::Error)]
#[error("store precondition changed concurrently")]
struct PreconditionRace;

fn decode_state_row(row: &PgRow) -> Result<AgentState, StoreError> {
    let agent_id: Uuid = row.try_get("agent_id").map_err(StoreError::backend)?;
    let state_version: i32 = row.try_get("state_version").map_err(StoreError::backend)?;
    let status: String = row.try_get("status").map_err(StoreError::backend)?;
    let session_id: Option<Uuid> = row.try_get("session_id").map_err(StoreError::backend)?;
    let active_turn_id: Option<Uuid> =
        row.try_get("active_turn_id").map_err(StoreError::backend)?;
    let next_iteration: i64 = row.try_get("next_iteration").map_err(StoreError::backend)?;
    let next_message_seq: i64 = row
        .try_get("next_message_seq")
        .map_err(StoreError::backend)?;
    Ok(AgentState {
        state_version: u32::try_from(state_version).map_err(StoreError::backend)?,
        agent_id: AgentId::from(agent_id),
        name: row.try_get("name").map_err(StoreError::backend)?,
        agent_version_id: row
            .try_get::<Uuid, _>("agent_version_id")
            .map(From::from)
            .map_err(StoreError::backend)?,
        skill_set_version_id: row
            .try_get::<Uuid, _>("skill_set_version_id")
            .map(From::from)
            .map_err(StoreError::backend)?,
        extension_set_version_id: row
            .try_get::<Uuid, _>("extension_set_version_id")
            .map(From::from)
            .map_err(StoreError::backend)?,
        hook_handler_versions: decode_json(
            row.try_get("hook_handler_versions")
                .map_err(StoreError::backend)?,
        )?,
        model_config: row
            .try_get::<Option<Value>, _>("model_config")
            .map_err(StoreError::backend)?
            .map(decode_json)
            .transpose()?,
        status: decode_status(&status)?,
        session_id: session_id.map(SessionId::from),
        turn_id: active_turn_id.map(TurnId::from),
        location: row
            .try_get::<Option<Value>, _>("location")
            .map_err(StoreError::backend)?
            .map(decode_json)
            .transpose()?,
        turn_runtime_snapshot: row
            .try_get::<Option<Value>, _>("runtime_snapshot")
            .map_err(StoreError::backend)?
            .map(decode_json)
            .transpose()?,
        next_iteration: u64::try_from(next_iteration).map_err(StoreError::backend)?,
        usage: decode_json(row.try_get("usage").map_err(StoreError::backend)?)?,
        last_seq: u64::try_from(next_message_seq).map_err(StoreError::backend)?,
        updated_at: row.try_get("updated_at").map_err(StoreError::backend)?,
    })
}

fn encode_json<T: serde::Serialize>(value: &T) -> Result<Value, StoreError> {
    serde_json::to_value(value).map_err(StoreError::Encode)
}

fn decode_json<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, StoreError> {
    serde_json::from_value(value).map_err(StoreError::DecodeState)
}

/// Serialized `status` text; matches the serde snake_case representation.
fn status_str(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Idle => "idle",
        AgentStatus::Running => "running",
        AgentStatus::Finished => "finished",
        AgentStatus::Failed => "failed",
        AgentStatus::Cancelled => "cancelled",
    }
}

fn decode_status(raw: &str) -> Result<AgentStatus, StoreError> {
    serde_json::from_value(Value::String(raw.to_owned())).map_err(StoreError::DecodeState)
}

/// Materialized `location` text of one message row: the `AgentLocation` type
/// tag. The full location lives inside the envelope payload.
fn location_tag(location: &AgentLocation) -> String {
    match location {
        AgentLocation::Direct => "direct".to_owned(),
        AgentLocation::WorkflowNode { .. } => "workflow_node".to_owned(),
        location => {
            tracing::warn!(?location, "unrecognized agent location tag");
            "unknown".to_owned()
        }
    }
}

fn decode_message(payload: &Value) -> Result<StreamEnvelope, StoreError> {
    validate_strict_message_json(payload).map_err(StoreError::DecodeMessage)?;
    serde_json::from_value(payload.clone()).map_err(StoreError::DecodeMessage)
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

#[cfg(test)]
mod tests {
    use stratum_core::AgentLocation;

    use super::*;

    #[test]
    fn status_text_round_trips_and_matches_serde() {
        for status in [
            AgentStatus::Idle,
            AgentStatus::Running,
            AgentStatus::Finished,
            AgentStatus::Failed,
            AgentStatus::Cancelled,
        ] {
            let serde_text = serde_json::to_value(status)
                .expect("status serializes")
                .as_str()
                .expect("status is a string")
                .to_owned();
            assert_eq!(status_str(status), serde_text);
            assert_eq!(decode_status(&serde_text).expect("status decodes"), status);
        }
        assert!(decode_status("retired").is_err());
    }

    #[test]
    fn history_range_validation_matches_filesystem_semantics() {
        let query = HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 0,
        };
        assert!(matches!(
            plan_history_range(&query, 10),
            Err(StoreError::InvalidHistoryLimit {
                actual: 0,
                maximum: MAX_HISTORY_PAGE_SIZE
            })
        ));

        let query = HistoryQuery {
            limit: MAX_HISTORY_PAGE_SIZE + 1,
            ..query
        };
        assert!(matches!(
            plan_history_range(&query, 10),
            Err(StoreError::InvalidHistoryLimit { .. })
        ));

        let query = HistoryQuery {
            after_seq: 0,
            through_seq: Some(11),
            limit: 10,
        };
        assert!(matches!(
            plan_history_range(&query, 10),
            Err(StoreError::HistoryBarrierBeyondLast {
                through_seq: 11,
                last_seq: 10
            })
        ));

        let query = HistoryQuery {
            after_seq: 5,
            through_seq: Some(4),
            limit: 10,
        };
        assert!(matches!(
            plan_history_range(&query, 10),
            Err(StoreError::InvalidHistoryRange {
                after_seq: 5,
                through_seq: 4
            })
        ));

        let query = HistoryQuery {
            after_seq: 2,
            through_seq: None,
            limit: 256,
        };
        let range = plan_history_range(&query, 10).expect("range plans");
        assert_eq!(range.through_seq, 10);
        assert_eq!(range.count, 8);

        let query = HistoryQuery { limit: 3, ..query };
        let range = plan_history_range(&query, 10).expect("range plans");
        assert_eq!(range.through_seq, 10);
        assert_eq!(range.count, 3);

        let query = HistoryQuery {
            after_seq: 10,
            through_seq: None,
            limit: 3,
        };
        let range = plan_history_range(&query, 10).expect("empty range plans");
        assert_eq!(range.count, 0);
    }

    #[test]
    fn location_tags_match_location_type_names() {
        assert_eq!(location_tag(&AgentLocation::Direct), "direct");
        assert_eq!(
            location_tag(&AgentLocation::WorkflowNode {
                workflow_version_id: stratum_core::WorkflowVersionId::new(),
                node_id: stratum_core::NodeId::from("node-1"),
            }),
            "workflow_node"
        );
    }
}
