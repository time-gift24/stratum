//! Durable write commands.
//!
//! Every writer (kernel sink adapter, approval requester/resolver, admission,
//! started-only reconciliation) funnels through the same transaction template:
//! lock the exact `agent_states` row `FOR UPDATE` (the row lock is both the
//! AgentRuntime-wide `event_seq` allocator and the serialization point for all
//! writers of one runtime), validate exact identity expectations, assign
//! `last_event_seq + 1`, insert the durable row, apply only the state side
//! effect owned by that event, and advance the high-water in the same commit.

use sqlx::{PgConnection, PgPool, Row};
use stratum_core::{AgentId, AgentRuntimeId, ChatRole, DurableAgentEvent, TurnId};
use uuid::Uuid;

use crate::codec;
use crate::error::PostgresError;
use crate::types::{
    AgentRuntimeCreated, AppendEvent, BeginTurn, CommitReceipt, CreateAgentRuntime,
    CreateAgentRuntimeOutcome, EVENT_SEQ_MAX, ResolveApproval, ResolveApprovalOutcome,
    u64_to_bigint,
};

/// Constraint names produced by the baseline migration, used to map unique
/// violations to typed errors.
mod constraints {
    pub(crate) const IDEMPOTENCY_KEY: &str = "agent_states_idempotency_key_unique";
    pub(crate) const RUNNING_SESSION: &str = "agent_states_running_session_unique";
    pub(crate) const APPROVAL_REQUESTED: &str = "durable_events_approval_requested_unique";
    pub(crate) const APPROVAL_ID: &str = "durable_events_approval_requested_by_approval";
}

/// Locked `agent_states` row: the allocator and serialization point.
struct LockedState {
    agent_id: AgentId,
    status: crate::types::AgentStatus,
    session_id: Option<Uuid>,
    current_turn_id: Option<Uuid>,
    model_config: serde_json::Value,
    last_event_seq: i64,
}

impl LockedState {
    /// The sequence the next append assigns; the AgentRuntime-wide space ends at
    /// `i64::MAX`.
    fn next_event_seq(&self, agent_runtime_id: AgentRuntimeId) -> Result<u64, PostgresError> {
        let current = u64::try_from(self.last_event_seq).map_err(|_| {
            PostgresError::corrupt_invariant("agent_states.last_event_seq is negative")
        })?;
        if current == EVENT_SEQ_MAX {
            return Err(PostgresError::SequenceOverflow { agent_runtime_id });
        }
        current
            .checked_add(1)
            .ok_or(PostgresError::SequenceOverflow { agent_runtime_id })
    }
}

/// Locks the exact AgentRuntime state row inside `tx`.
async fn lock_agent_runtime_state(
    tx: &mut PgConnection,
    agent_runtime_id: AgentRuntimeId,
) -> Result<LockedState, PostgresError> {
    let row = sqlx::query(
        "SELECT agent_id, status, session_id, current_turn_id, model_config, last_event_seq \
         FROM agent_states WHERE id = $1 FOR UPDATE",
    )
    .bind(agent_runtime_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?
    .ok_or(PostgresError::AgentRuntimeNotFound { agent_runtime_id })?;

    let status_text: String = row
        .try_get("status")
        .map_err(PostgresError::StoreUnavailable)?;
    let status = status_text.parse()?;
    Ok(LockedState {
        agent_id: AgentId::from(
            row.try_get::<Uuid, _>("agent_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        status,
        session_id: row
            .try_get("session_id")
            .map_err(PostgresError::StoreUnavailable)?,
        current_turn_id: row
            .try_get("current_turn_id")
            .map_err(PostgresError::StoreUnavailable)?,
        model_config: row
            .try_get("model_config")
            .map_err(PostgresError::StoreUnavailable)?,
        last_event_seq: row
            .try_get("last_event_seq")
            .map_err(PostgresError::StoreUnavailable)?,
    })
}

async fn find_by_idempotency_key<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    idempotency_key: Uuid,
) -> Result<Option<AgentRuntimeCreated>, PostgresError> {
    let row = sqlx::query(
        "SELECT s.id AS agent_runtime_id, s.agent_id, a.id AS definition_agent_id, \
             a.name, a.version, s.created_at \
         FROM agent_states s LEFT JOIN agents a ON a.id = s.agent_id \
         WHERE s.idempotency_key = $1",
    )
    .bind(idempotency_key)
    .fetch_optional(executor)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    row.map(|row| decode_created_runtime(&row)).transpose()
}

fn decode_created_runtime(
    row: &sqlx::postgres::PgRow,
) -> Result<AgentRuntimeCreated, PostgresError> {
    let pinned_agent_id: Uuid = row
        .try_get("agent_id")
        .map_err(PostgresError::StoreUnavailable)?;
    let definition_agent_id: Option<Uuid> = row
        .try_get("definition_agent_id")
        .map_err(PostgresError::StoreUnavailable)?;
    if definition_agent_id != Some(pinned_agent_id) {
        return Err(PostgresError::corrupt_invariant(
            "agent_states agent_id pin has no matching agents row",
        ));
    }
    let version: String = row
        .try_get::<Option<String>, _>("version")
        .map_err(PostgresError::StoreUnavailable)?
        .ok_or(PostgresError::corrupt_invariant(
            "pinned agents row lacks its version tag",
        ))?;
    let agent_name: String = row
        .try_get::<Option<String>, _>("name")
        .map_err(PostgresError::StoreUnavailable)?
        .ok_or(PostgresError::corrupt_invariant(
            "pinned agents row lacks its name",
        ))?;
    let agent_version = version.parse().map_err(|_| {
        PostgresError::corrupt_invariant("agents.version violates AgentVersionTag boundary")
    })?;
    Ok(AgentRuntimeCreated {
        agent_runtime_id: AgentRuntimeId::from(
            row.try_get::<Uuid, _>("agent_runtime_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        agent_id: AgentId::from(pinned_agent_id),
        agent_name,
        agent_version,
        created_at: row
            .try_get("created_at")
            .map_err(PostgresError::StoreUnavailable)?,
    })
}

/// Key-only idempotent create of an AgentRuntime and, when needed, its
/// immutable Agent template-version row.
#[tracing::instrument(skip_all)]
pub(crate) async fn create_agent_runtime(
    pool: &PgPool,
    command: CreateAgentRuntime,
) -> Result<CreateAgentRuntimeOutcome, PostgresError> {
    if let Some(existing) = find_by_idempotency_key(pool, command.idempotency_key).await? {
        return Ok(CreateAgentRuntimeOutcome::Replay(existing));
    }

    let resolved_definition = codec::encode_resolved_definition(&command.resolved_definition)?;
    let model_config = serde_json::to_value(&command.model_config).map_err(|source| {
        PostgresError::EventEncode {
            event_type: "model_config",
            source,
        }
    })?;

    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    // Serialize materialization of one exact author-owned name/tag pair. A
    // 64-bit hash collision only over-serializes unrelated pairs; the unique
    // constraint remains the correctness backstop.
    sqlx::query(
        "SELECT pg_advisory_xact_lock( \
             hashtextextended($1, 0) # hashtextextended($2, 1) \
         )",
    )
    .bind(&command.name)
    .bind(command.version.as_str())
    .execute(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    // Recheck after the pair lock. This makes same-key/same-pair races return
    // without attempting any immutable-definition mutation.
    if let Some(existing) = find_by_idempotency_key(&mut *tx, command.idempotency_key).await? {
        return Ok(CreateAgentRuntimeOutcome::Replay(existing));
    }

    let existing_definition = sqlx::query(
        "SELECT id, definition_schema_version, resolved_definition \
         FROM agents WHERE name = $1 AND version = $2",
    )
    .bind(&command.name)
    .bind(command.version.as_str())
    .fetch_optional(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    let agent_id = if let Some(row) = existing_definition {
        let stored_schema_version: i32 = row
            .try_get("definition_schema_version")
            .map_err(PostgresError::StoreUnavailable)?;
        let stored_definition: serde_json::Value = row
            .try_get("resolved_definition")
            .map_err(PostgresError::StoreUnavailable)?;
        let stored_definition =
            codec::decode_resolved_definition(stored_schema_version, stored_definition)?;
        if stored_definition != command.resolved_definition {
            return Err(PostgresError::AgentVersionConflict {
                version: command.version,
            });
        }
        AgentId::from(
            row.try_get::<Uuid, _>("id")
                .map_err(PostgresError::StoreUnavailable)?,
        )
    } else {
        let agent_id = AgentId::new();
        sqlx::query(
            "INSERT INTO agents \
                 (id, name, version, definition_schema_version, resolved_definition) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(agent_id.as_uuid())
        .bind(&command.name)
        .bind(command.version.as_str())
        .bind(codec::DEFINITION_SCHEMA_VERSION_V1)
        .bind(&resolved_definition)
        .execute(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
        agent_id
    };

    let agent_runtime_id = AgentRuntimeId::new();
    let inserted = sqlx::query(
        "INSERT INTO agent_states \
             (id, agent_id, idempotency_key, status, model_config) \
         VALUES ($1, $2, $3, 'idle', $4) \
         RETURNING id AS agent_runtime_id, agent_id, created_at",
    )
    .bind(agent_runtime_id.as_uuid())
    .bind(agent_id.as_uuid())
    .bind(command.idempotency_key)
    .bind(model_config)
    .fetch_one(&mut *tx)
    .await;

    let state_row = match inserted {
        Ok(row) => row,
        Err(source) if codec::is_unique_violation_on(&source, constraints::IDEMPOTENCY_KEY) => {
            tx.rollback()
                .await
                .map_err(PostgresError::StoreUnavailable)?;
            let existing = find_by_idempotency_key(pool, command.idempotency_key)
                .await?
                .ok_or(PostgresError::corrupt_invariant(
                    "idempotency key unique violation without a visible owner row",
                ))?;
            return Ok(CreateAgentRuntimeOutcome::Replay(existing));
        }
        Err(source) => return Err(PostgresError::StoreUnavailable(source)),
    };

    let created = AgentRuntimeCreated {
        agent_runtime_id: AgentRuntimeId::from(
            state_row
                .try_get::<Uuid, _>("agent_runtime_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        agent_id: AgentId::from(
            state_row
                .try_get::<Uuid, _>("agent_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        agent_name: command.name,
        agent_version: command.version,
        created_at: state_row
            .try_get("created_at")
            .map_err(PostgresError::StoreUnavailable)?,
    };

    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    Ok(CreateAgentRuntimeOutcome::Created(created))
}

/// Admission: CAS on the expected current Turn, bind or validate the Session,
/// commit the `LoopStarted` row with its v1 runtime snapshot, and flip the
/// state to running — all in one transaction.
#[tracing::instrument(skip_all, fields(agent_runtime_id = %command.agent_runtime_id, turn_id = %command.turn_id))]
pub(crate) async fn begin_turn(
    pool: &PgPool,
    command: BeginTurn,
) -> Result<CommitReceipt, PostgresError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let state = lock_agent_runtime_state(&mut tx, command.agent_runtime_id).await?;

    // The CAS expectation is judged before the busy status: a stale
    // expectation must fail as StaleTurn even while the Agent is running (a
    // lost-response retry carrying the old expected value must never be told
    // busy). AgentRuntimeBusy applies only when the expectation still matches the
    // current Turn.
    let actual_turn = state.current_turn_id.map(TurnId::from);
    if actual_turn != command.expected_current_turn_id {
        return Err(PostgresError::StaleTurn {
            agent_runtime_id: command.agent_runtime_id,
            expected: command.expected_current_turn_id,
            actual: actual_turn,
        });
    }
    if state.status == crate::types::AgentStatus::Running {
        return Err(PostgresError::AgentRuntimeBusy {
            agent_runtime_id: command.agent_runtime_id,
        });
    }
    if let Some(bound) = state.session_id
        && bound != command.session_id.as_uuid()
    {
        return Err(PostgresError::SessionMismatch {
            agent_runtime_id: command.agent_runtime_id,
        });
    }
    codec::ensure_runtime_snapshot_agent(&command.snapshot, state.agent_id)?;

    let event_seq = state.next_event_seq(command.agent_runtime_id)?;
    let event_seq_db = u64_to_bigint(event_seq, "event sequence exceeds bigint range")?;
    let event = DurableAgentEvent::LoopStarted {
        extension_set_version_id: Some(command.snapshot.extension_set_version_id),
    };
    let encoded = codec::encode_event(&event, None)?;
    let snapshot = codec::encode_runtime_snapshot(&command.snapshot)?;

    sqlx::query(
        "INSERT INTO durable_events (agent_runtime_id, event_seq, session_id, turn_id, event_type, \
             event_version, payload, runtime_snapshot_version, runtime_snapshot) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(event_seq_db)
    .bind(command.session_id.as_uuid())
    .bind(command.turn_id.as_uuid())
    .bind(encoded.event_type)
    .bind(encoded.event_version)
    .bind(&encoded.payload)
    .bind(codec::RUNTIME_SNAPSHOT_VERSION_V1)
    .bind(&snapshot)
    .execute(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    let updated = sqlx::query(
        "UPDATE agent_states SET status = 'running', session_id = $2, current_turn_id = $3, \
             last_event_seq = $4, updated_at = now() \
         WHERE id = $1",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(command.session_id.as_uuid())
    .bind(command.turn_id.as_uuid())
    .bind(event_seq_db)
    .execute(&mut *tx)
    .await;
    match updated {
        Ok(_) => {}
        Err(source) if codec::is_unique_violation_on(&source, constraints::RUNNING_SESSION) => {
            return Err(PostgresError::SessionBusy {
                session_id: command.session_id,
            });
        }
        Err(source) => return Err(PostgresError::StoreUnavailable(source)),
    }

    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    Ok(CommitReceipt { event_seq })
}

/// Takes the runtime serialization lock and revalidates the exact running
/// Session/Turn immediately before the API installs a prepared resume task.
#[tracing::instrument(skip_all, fields(agent_runtime_id = %agent_runtime_id, turn_id = %turn_id))]
pub(crate) async fn revalidate_resume(
    pool: &PgPool,
    agent_runtime_id: AgentRuntimeId,
    agent_id: AgentId,
    session_id: stratum_core::SessionId,
    turn_id: TurnId,
) -> Result<(), PostgresError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let state = lock_agent_runtime_state(&mut tx, agent_runtime_id).await?;

    if state.agent_id != agent_id {
        return Err(PostgresError::corrupt_invariant(
            "agent runtime definition pin changed during resume",
        ));
    }

    if state.session_id != Some(session_id.as_uuid()) {
        return Err(PostgresError::SessionMismatch { agent_runtime_id });
    }
    let actual_turn = state.current_turn_id.map(TurnId::from);
    if actual_turn != Some(turn_id) {
        return Err(PostgresError::StaleTurn {
            agent_runtime_id,
            expected: Some(turn_id),
            actual: actual_turn,
        });
    }
    if state.status != crate::types::AgentStatus::Running {
        return Err(PostgresError::TurnNotRunning {
            agent_runtime_id,
            turn_id,
            status: state.status,
        });
    }

    tx.commit().await.map_err(PostgresError::StoreUnavailable)
}

/// Centralized durable append used by every writer; see the module docs for
/// the transaction template.
#[tracing::instrument(skip_all, fields(agent_runtime_id = %command.agent_runtime_id, turn_id = %command.turn_id, event_type = command.event.event_type()))]
pub(crate) async fn append_event(
    pool: &PgPool,
    command: AppendEvent,
) -> Result<CommitReceipt, PostgresError> {
    validate_append_shape(&command)?;
    let terminal_status = terminal_status_of(&command.event)?;

    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let state = lock_agent_runtime_state(&mut tx, command.agent_runtime_id).await?;

    if state.agent_id != command.agent_id {
        return Err(PostgresError::corrupt_invariant(
            "durable writer agent definition does not match agent_states pin",
        ));
    }

    if state.session_id != Some(command.session_id.as_uuid()) {
        return Err(PostgresError::SessionMismatch {
            agent_runtime_id: command.agent_runtime_id,
        });
    }
    let actual_turn = state.current_turn_id.map(TurnId::from);
    if actual_turn != Some(command.turn_id) {
        return Err(PostgresError::StaleTurn {
            agent_runtime_id: command.agent_runtime_id,
            expected: Some(command.turn_id),
            actual: actual_turn,
        });
    }

    if state.status != crate::types::AgentStatus::Running {
        return Err(PostgresError::TurnNotRunning {
            agent_runtime_id: command.agent_runtime_id,
            turn_id: command.turn_id,
            status: state.status,
        });
    }
    if command.model_config_update.is_some()
        && crate::queries::turn_has_user_message(&mut tx, command.agent_runtime_id, command.turn_id)
            .await?
    {
        return Err(PostgresError::InvalidCommand(
            "model_config_update is only valid on the first user message_appended",
        ));
    }

    let event_seq = state.next_event_seq(command.agent_runtime_id)?;
    let event_seq_db = u64_to_bigint(event_seq, "event sequence exceeds bigint range")?;
    let encoded = codec::encode_event(&command.event, command.approval_hook_invocation_id)?;

    // Fail closed before any write when the retained pointer cannot address a
    // real earlier MessageAppended of this AgentRuntime.
    if let Some(compaction) = &command.compaction {
        if compaction.retained_from_event_seq == 0
            || compaction.retained_from_event_seq >= event_seq
        {
            return Err(PostgresError::InvalidCompactionPointer {
                agent_runtime_id: command.agent_runtime_id,
                retained_from_event_seq: compaction.retained_from_event_seq,
            });
        }
        let pointer_valid: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM durable_events \
             WHERE agent_runtime_id = $1 AND event_seq = $2 AND event_type = 'message_appended')",
        )
        .bind(command.agent_runtime_id.as_uuid())
        .bind(u64_to_bigint(
            compaction.retained_from_event_seq,
            "compaction retained sequence exceeds bigint range",
        )?)
        .fetch_one(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
        if !pointer_valid {
            return Err(PostgresError::InvalidCompactionPointer {
                agent_runtime_id: command.agent_runtime_id,
                retained_from_event_seq: compaction.retained_from_event_seq,
            });
        }
    }

    let inserted = sqlx::query(
        "INSERT INTO durable_events (agent_runtime_id, event_seq, session_id, turn_id, event_type, \
             event_version, payload) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(event_seq_db)
    .bind(command.session_id.as_uuid())
    .bind(command.turn_id.as_uuid())
    .bind(encoded.event_type)
    .bind(encoded.event_version)
    .bind(&encoded.payload)
    .execute(&mut *tx)
    .await;
    if let Err(source) = inserted {
        if codec::is_unique_violation_on(&source, constraints::APPROVAL_REQUESTED) {
            let hook_invocation_id =
                command
                    .approval_hook_invocation_id
                    .ok_or(PostgresError::corrupt_invariant(
                        "approval uniqueness violation without an invocation id",
                    ))?;
            return Err(PostgresError::ApprovalAlreadyRequested { hook_invocation_id });
        }
        if codec::is_unique_violation_on(&source, constraints::APPROVAL_ID) {
            let approval_id = match &command.event {
                DurableAgentEvent::ToolApprovalRequested { approval_id, .. } => *approval_id,
                _ => {
                    return Err(PostgresError::corrupt_invariant(
                        "approval identity constraint rejected a non-request event",
                    ));
                }
            };
            return Err(PostgresError::ApprovalIdConflict { approval_id });
        }
        return Err(PostgresError::StoreUnavailable(source));
    }

    // The TranscriptCompacted companion commits atomically with its
    // discriminator; any failure rolls back the whole transaction.
    if let Some(compaction) = &command.compaction {
        let summary = serde_json::to_value(&compaction.summary).map_err(|source| {
            PostgresError::EventEncode {
                event_type: "transcript_compaction_summary",
                source,
            }
        })?;
        sqlx::query(
            "INSERT INTO transcript_compactions (agent_runtime_id, event_seq, turn_id, \
                 compacted_iteration, upto, retained_from_event_seq, summary) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(command.agent_runtime_id.as_uuid())
        .bind(event_seq_db)
        .bind(command.turn_id.as_uuid())
        .bind(u64_to_bigint(
            compaction.compacted_iteration,
            "compaction iteration exceeds bigint range",
        )?)
        .bind(u64_to_bigint(
            compaction.upto,
            "compaction cut exceeds bigint range",
        )?)
        .bind(u64_to_bigint(
            compaction.retained_from_event_seq,
            "compaction retained sequence exceeds bigint range",
        )?)
        .bind(summary)
        .execute(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    }

    // Only the state fields owned by this event change; the high-water always
    // advances in the same commit.
    let model_config_update = match &command.model_config_update {
        Some(model) => {
            let current = codec::decode_model_config(
                state.model_config,
                "agent_states.model_config failed v1 decode",
            )?;
            // No-op when the replacement is identical to the current runtime
            // configuration.
            if &current == model {
                None
            } else {
                Some(
                    serde_json::to_value(model).map_err(|source| PostgresError::EventEncode {
                        event_type: "model_config",
                        source,
                    })?,
                )
            }
        }
        None => None,
    };

    sqlx::query(
        "UPDATE agent_states SET \
             status = COALESCE($2, status), \
             model_config = COALESCE($3, model_config), \
             last_event_seq = $4, updated_at = now() \
         WHERE id = $1",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(terminal_status)
    .bind(model_config_update)
    .bind(event_seq_db)
    .execute(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    Ok(CommitReceipt { event_seq })
}

/// Command-shape invariants checked before any transaction work.
fn validate_append_shape(command: &AppendEvent) -> Result<(), PostgresError> {
    if matches!(command.event, DurableAgentEvent::LoopStarted { .. }) {
        return Err(PostgresError::InvalidCommand(
            "loop_started is only written by begin_turn",
        ));
    }
    match (&command.event, &command.compaction) {
        (
            DurableAgentEvent::TranscriptCompacted {
                upto,
                summary,
                compacted_iteration,
            },
            Some(compaction),
        ) => {
            if *upto != compaction.upto
                || summary != &compaction.summary
                || *compacted_iteration != compaction.compacted_iteration
            {
                return Err(PostgresError::InvalidCommand(
                    "transcript_compacted event disagrees with its companion facts",
                ));
            }
        }
        (DurableAgentEvent::TranscriptCompacted { .. }, None) => {
            return Err(PostgresError::InvalidCommand(
                "transcript_compacted requires exactly one compaction companion",
            ));
        }
        (_, Some(_)) => {
            return Err(PostgresError::InvalidCommand(
                "compaction companion is only valid for transcript_compacted",
            ));
        }
        (_, None) => {}
    }
    if command.model_config_update.is_some() {
        let is_user_message = matches!(
            &command.event,
            DurableAgentEvent::MessageAppended { message } if message.role == ChatRole::User
        );
        if !is_user_message {
            return Err(PostgresError::InvalidCommand(
                "model_config_update is only valid on a user message_appended",
            ));
        }
    }
    Ok(())
}

/// Terminal status owned by an event, when the event is terminal.
fn terminal_status_of(event: &DurableAgentEvent) -> Result<Option<&'static str>, PostgresError> {
    match event {
        DurableAgentEvent::LoopFinished { .. } => Ok(Some("finished")),
        DurableAgentEvent::LoopFailed { .. } => Ok(Some("failed")),
        DurableAgentEvent::LoopCancelled { .. } => Ok(Some("cancelled")),
        DurableAgentEvent::LoopStarted { .. }
        | DurableAgentEvent::MessageAppended { .. }
        | DurableAgentEvent::ToolApprovalRequested { .. }
        | DurableAgentEvent::ToolApprovalResolved { .. }
        | DurableAgentEvent::ToolExecutionStarted { .. }
        | DurableAgentEvent::HookInvocationPending { .. }
        | DurableAgentEvent::HookInvocationCompleted { .. }
        | DurableAgentEvent::HookInvocationFailed { .. }
        | DurableAgentEvent::TranscriptCompacted { .. }
        | DurableAgentEvent::IterationCompleted { .. } => Ok(None),
        _ => Err(PostgresError::InvalidCommand(
            "unsupported durable event variant",
        )),
    }
}

/// Approval resolve: linearized with every other writer of the AgentRuntime through
/// the shared state row lock; terminal wins over every other outcome, an
/// identical earlier decision is an idempotent success, and only an
/// undecided request on a running Turn appends the unique Resolved row.
#[tracing::instrument(skip_all, fields(agent_runtime_id = %command.agent_runtime_id, turn_id = %command.turn_id))]
pub(crate) async fn resolve_approval(
    pool: &PgPool,
    command: ResolveApproval,
) -> Result<ResolveApprovalOutcome, PostgresError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let state = lock_agent_runtime_state(&mut tx, command.agent_runtime_id).await?;

    if state.agent_id != command.agent_id {
        return Err(PostgresError::corrupt_invariant(
            "approval resolver agent definition does not match agent_states pin",
        ));
    }

    let actual_turn = state.current_turn_id.map(TurnId::from);
    if actual_turn != Some(command.turn_id) {
        return Err(PostgresError::StaleTurn {
            agent_runtime_id: command.agent_runtime_id,
            expected: Some(command.turn_id),
            actual: actual_turn,
        });
    }

    // Validate the complete fact set before JSON-path filters select the
    // request being resolved. A malformed/future fact in this Turn cannot be
    // hidden as an unrelated lookup miss or exclusion.
    crate::queries::ensure_approval_derivation_rows_compatible(
        &mut tx,
        command.agent_runtime_id,
        command.turn_id,
        None,
    )
    .await?;

    let requested_rows = sqlx::query(
        "SELECT r.event_seq, r.event_version, r.payload, r.session_id, r.turn_id, \
             r.runtime_snapshot_version, r.runtime_snapshot, \
             s.agent_id AS state_agent_id, s.session_id AS state_session_id, \
             rc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events r \
         JOIN agent_states s ON s.id = r.agent_runtime_id \
         LEFT JOIN transcript_compactions rc \
             ON rc.agent_runtime_id = r.agent_runtime_id AND rc.event_seq = r.event_seq \
         WHERE r.agent_runtime_id = $1 AND r.turn_id = $2 \
             AND r.event_type = 'tool_approval_requested' \
             AND r.payload ->> 'approval_id' = $3 \
         ORDER BY r.event_seq ASC",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(command.turn_id.as_uuid())
    .bind(command.approval_id.as_uuid().to_string())
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    let mut request = None;
    for row in requested_rows {
        let event_version = crate::queries::decode_non_compaction_event_version(&row)?;
        let (_, row_turn_id) =
            crate::queries::validate_event_row_envelope(&row, "tool_approval_requested")?;
        if row_turn_id != command.turn_id {
            return Err(PostgresError::corrupt_invariant(
                "approval request belongs to a different turn",
            ));
        }
        let payload: serde_json::Value = row
            .try_get("payload")
            .map_err(PostgresError::StoreUnavailable)?;
        let requested = codec::RequestedApprovalPayload::decode(event_version, payload)?;
        if requested.approval_id != command.approval_id {
            return Err(PostgresError::corrupt_invariant(
                "approval request index disagrees with decoded identity",
            ));
        }
        let requested_event_seq: i64 = row
            .try_get("event_seq")
            .map_err(PostgresError::StoreUnavailable)?;
        let requested_event_seq = u64::try_from(requested_event_seq).map_err(|_| {
            PostgresError::corrupt_invariant("approval request sequence is negative")
        })?;
        if request
            .replace((requested_event_seq, requested.hook_invocation_id))
            .is_some()
        {
            return Err(PostgresError::corrupt_invariant(
                "approval resolve matched multiple request rows",
            ));
        }
    }
    let Some((requested_event_seq, hook_invocation_id)) = request else {
        return Err(PostgresError::ApprovalNotFound {
            approval_id: command.approval_id,
        });
    };

    let resolved_rows = sqlx::query(
        "SELECT r.event_seq, r.turn_id, r.session_id, r.event_version, r.payload, \
             r.runtime_snapshot_version, r.runtime_snapshot, \
             s.agent_id AS state_agent_id, s.session_id AS state_session_id, \
             rc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events r \
         JOIN agent_states s ON s.id = r.agent_runtime_id \
         LEFT JOIN transcript_compactions rc \
             ON rc.agent_runtime_id = r.agent_runtime_id AND rc.event_seq = r.event_seq \
         WHERE r.agent_runtime_id = $1 AND r.event_type = 'tool_approval_resolved' \
             AND r.payload ->> 'approval_id' = $2 \
         ORDER BY r.event_seq ASC",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(command.approval_id.as_uuid().to_string())
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    let mut existing_resolution = None;
    for row in resolved_rows {
        let event_version = crate::queries::decode_non_compaction_event_version(&row)?;
        let (_, row_turn_id) =
            crate::queries::validate_event_row_envelope(&row, "tool_approval_resolved")?;
        if row_turn_id != command.turn_id {
            return Err(PostgresError::corrupt_invariant(
                "approval resolution belongs to a different turn",
            ));
        }
        let payload: serde_json::Value = row
            .try_get("payload")
            .map_err(PostgresError::StoreUnavailable)?;
        let resolution = codec::ResolvedApprovalPayload::decode(event_version, payload)?;
        if resolution.approval_id != command.approval_id {
            return Err(PostgresError::corrupt_invariant(
                "approval resolution index disagrees with decoded identity",
            ));
        }
        let resolved_turn_id: Uuid = row
            .try_get("turn_id")
            .map_err(PostgresError::StoreUnavailable)?;
        if resolved_turn_id != command.turn_id.as_uuid() {
            return Err(PostgresError::corrupt_invariant(
                "approval resolution belongs to a different turn",
            ));
        }
        let resolved_event_seq: i64 = row
            .try_get("event_seq")
            .map_err(PostgresError::StoreUnavailable)?;
        let resolved_event_seq = u64::try_from(resolved_event_seq).map_err(|_| {
            PostgresError::corrupt_invariant("approval resolution sequence is negative")
        })?;
        if resolved_event_seq <= requested_event_seq {
            return Err(PostgresError::corrupt_invariant(
                "approval resolution does not follow its request",
            ));
        }
        if existing_resolution
            .replace((resolved_event_seq, resolution.decision))
            .is_some()
        {
            return Err(PostgresError::corrupt_invariant(
                "approval resolve matched multiple resolution rows",
            ));
        }
    }
    let completed_rows = sqlx::query(
        "SELECT r.event_seq, r.session_id, r.turn_id, r.event_version, r.payload, \
             r.runtime_snapshot_version, r.runtime_snapshot, \
             s.agent_id AS state_agent_id, s.session_id AS state_session_id, \
             rc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events r \
         JOIN agent_states s ON s.id = r.agent_runtime_id \
         LEFT JOIN transcript_compactions rc \
             ON rc.agent_runtime_id = r.agent_runtime_id AND rc.event_seq = r.event_seq \
         WHERE r.agent_runtime_id = $1 \
             AND r.event_type = 'hook_invocation_completed' \
             AND r.payload ->> 'invocation_id' = $2 \
         ORDER BY r.event_seq ASC",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(hook_invocation_id.as_uuid().to_string())
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    let mut completed_event_seq = None;
    for row in completed_rows {
        let event_version = crate::queries::decode_non_compaction_event_version(&row)?;
        let (_, row_turn_id) =
            crate::queries::validate_event_row_envelope(&row, "hook_invocation_completed")?;
        if row_turn_id != command.turn_id {
            return Err(PostgresError::corrupt_invariant(
                "approval completion belongs to a different turn",
            ));
        }
        let payload: serde_json::Value = row
            .try_get("payload")
            .map_err(PostgresError::StoreUnavailable)?;
        let event = codec::decode_event("hook_invocation_completed", event_version, payload, None)?;
        let DurableAgentEvent::HookInvocationCompleted { invocation_id, .. } = event else {
            return Err(PostgresError::corrupt_invariant(
                "approval completion decoded as a different event",
            ));
        };
        if invocation_id != hook_invocation_id {
            return Err(PostgresError::corrupt_invariant(
                "approval completion index disagrees with decoded identity",
            ));
        }
        let event_seq: i64 = row
            .try_get("event_seq")
            .map_err(PostgresError::StoreUnavailable)?;
        let event_seq = u64::try_from(event_seq).map_err(|_| {
            PostgresError::corrupt_invariant("approval completion sequence is negative")
        })?;
        if event_seq <= requested_event_seq {
            return Err(PostgresError::corrupt_invariant(
                "approval completion does not follow its request",
            ));
        }
        if completed_event_seq.replace(event_seq).is_some() {
            return Err(PostgresError::corrupt_invariant(
                "approval resolve matched multiple completion rows",
            ));
        }
    }
    if let Some(completed_event_seq) = completed_event_seq {
        let Some((resolved_event_seq, _)) = existing_resolution else {
            return Err(PostgresError::corrupt_invariant(
                "approval completion exists without a durable resolution",
            ));
        };
        if completed_event_seq <= resolved_event_seq {
            return Err(PostgresError::corrupt_invariant(
                "approval completion does not follow its resolution",
            ));
        }
    }

    let terminal_rows = sqlx::query(
        "SELECT r.event_seq, r.event_type, r.event_version, r.payload, r.session_id, r.turn_id, \
             r.runtime_snapshot_version, r.runtime_snapshot, \
             s.agent_id AS state_agent_id, s.session_id AS state_session_id, \
             rc.event_seq IS NOT NULL AS has_compaction_companion \
         FROM durable_events r \
         JOIN agent_states s ON s.id = r.agent_runtime_id \
         LEFT JOIN transcript_compactions rc \
             ON rc.agent_runtime_id = r.agent_runtime_id AND rc.event_seq = r.event_seq \
         WHERE r.agent_runtime_id = $1 AND r.turn_id = $2 \
             AND r.event_type IN ('loop_finished', 'loop_failed', 'loop_cancelled') \
         ORDER BY r.event_seq ASC",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(command.turn_id.as_uuid())
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    let mut terminal_fact = None;
    for row in terminal_rows {
        let event_type: String = row
            .try_get("event_type")
            .map_err(PostgresError::StoreUnavailable)?;
        let event_version = crate::queries::decode_non_compaction_event_version(&row)?;
        let (_, row_turn_id) = crate::queries::validate_event_row_envelope(&row, &event_type)?;
        if row_turn_id != command.turn_id {
            return Err(PostgresError::corrupt_invariant(
                "approval terminal belongs to a different turn",
            ));
        }
        let payload: serde_json::Value = row
            .try_get("payload")
            .map_err(PostgresError::StoreUnavailable)?;
        let event = codec::decode_event(&event_type, event_version, payload, None)?;
        let status = terminal_status_of(&event)?
            .ok_or(PostgresError::corrupt_invariant(
                "approval terminal selector decoded a non-terminal event",
            ))?
            .parse()?;
        let event_seq: i64 = row
            .try_get("event_seq")
            .map_err(PostgresError::StoreUnavailable)?;
        let event_seq = u64::try_from(event_seq).map_err(|_| {
            PostgresError::corrupt_invariant("approval terminal sequence is negative")
        })?;
        if event_seq <= requested_event_seq {
            return Err(PostgresError::corrupt_invariant(
                "approval terminal does not follow its request",
            ));
        }
        if terminal_fact.replace((event_seq, status)).is_some() {
            return Err(PostgresError::corrupt_invariant(
                "approval resolve matched multiple terminal rows",
            ));
        }
    }

    match terminal_fact {
        Some((terminal_event_seq, terminal_status)) => {
            if state.status != terminal_status {
                return Err(PostgresError::corrupt_invariant(
                    "agent_states status disagrees with the Turn terminal event",
                ));
            }
            if let Some((resolved_event_seq, _)) = existing_resolution
                && resolved_event_seq >= terminal_event_seq
            {
                return Err(PostgresError::corrupt_invariant(
                    "approval resolution does not precede the Turn terminal",
                ));
            }
            if let Some(completed_event_seq) = completed_event_seq
                && completed_event_seq >= terminal_event_seq
            {
                return Err(PostgresError::corrupt_invariant(
                    "approval completion does not precede the Turn terminal",
                ));
            }
        }
        None if state.status != crate::types::AgentStatus::Running => {
            return Err(PostgresError::corrupt_invariant(
                "agent_states terminal status lacks a Turn terminal event",
            ));
        }
        None => {}
    }

    // Terminal wins over every other outcome, including an earlier decision.
    if terminal_fact.is_some() {
        return Err(PostgresError::ApprovalInvalidated {
            approval_id: command.approval_id,
        });
    }

    if let Some((_, decision)) = existing_resolution {
        return if decision == command.decision {
            Ok(ResolveApprovalOutcome::AlreadyResolvedSame)
        } else {
            Err(PostgresError::ApprovalAlreadyResolved {
                approval_id: command.approval_id,
            })
        };
    }

    let event_seq = state.next_event_seq(command.agent_runtime_id)?;
    let event = DurableAgentEvent::ToolApprovalResolved {
        approval_id: command.approval_id,
        decision: command.decision,
    };
    let encoded = codec::encode_event(&event, None)?;
    let event_seq_db = u64_to_bigint(event_seq, "event sequence exceeds bigint range")?;

    sqlx::query(
        "INSERT INTO durable_events (agent_runtime_id, event_seq, session_id, turn_id, event_type, \
             event_version, payload) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(command.agent_runtime_id.as_uuid())
    .bind(event_seq_db)
    .bind(state.session_id.ok_or(PostgresError::corrupt_invariant(
        "running state without a bound session",
    ))?)
    .bind(command.turn_id.as_uuid())
    .bind(encoded.event_type)
    .bind(encoded.event_version)
    .bind(&encoded.payload)
    .execute(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    sqlx::query("UPDATE agent_states SET last_event_seq = $2, updated_at = now() WHERE id = $1")
        .bind(command.agent_runtime_id.as_uuid())
        .bind(event_seq_db)
        .execute(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;

    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    Ok(ResolveApprovalOutcome::Resolved {
        receipt: CommitReceipt { event_seq },
    })
}

#[cfg(test)]
mod tests {
    use stratum_core::{ChatMessage, TokenUsage};

    use super::*;

    fn state_with_sequence(last_event_seq: i64) -> LockedState {
        LockedState {
            agent_id: AgentId::new(),
            status: crate::types::AgentStatus::Running,
            session_id: None,
            current_turn_id: None,
            model_config: serde_json::Value::Null,
            last_event_seq,
        }
    }

    #[test]
    fn next_event_sequence_rejects_negative_and_exhausted_state() {
        let agent_runtime_id = AgentRuntimeId::new();
        assert!(matches!(
            state_with_sequence(-1).next_event_seq(agent_runtime_id),
            Err(PostgresError::DurableStateCorrupt { .. })
        ));
        assert!(matches!(
            state_with_sequence(i64::MAX).next_event_seq(agent_runtime_id),
            Err(PostgresError::SequenceOverflow { .. })
        ));
        assert_eq!(
            state_with_sequence(0)
                .next_event_seq(agent_runtime_id)
                .expect("available sequence advances"),
            1
        );
    }

    #[test]
    fn terminal_status_classification_is_explicit() {
        let usage = TokenUsage::default();
        assert_eq!(
            terminal_status_of(&DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage,
            })
            .expect("known event classifies"),
            Some("finished")
        );
        assert_eq!(
            terminal_status_of(&DurableAgentEvent::LoopFailed {
                error_text: "safe error".to_owned(),
                usage,
            })
            .expect("known event classifies"),
            Some("failed")
        );
        assert_eq!(
            terminal_status_of(&DurableAgentEvent::LoopCancelled { usage })
                .expect("known event classifies"),
            Some("cancelled")
        );
        assert_eq!(
            terminal_status_of(&DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("hello"),
            })
            .expect("known event classifies"),
            None
        );
    }
}
