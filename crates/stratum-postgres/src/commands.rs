//! Durable write commands.
//!
//! Every writer (kernel sink adapter, approval requester/resolver, admission,
//! started-only reconciliation) funnels through the same transaction template:
//! lock the exact `agent_state` row `FOR UPDATE` (the row lock is both the
//! agent-wide `event_seq` allocator and the serialization point for all
//! writers of one Agent), validate exact identity expectations, assign
//! `last_event_seq + 1`, insert the durable row, apply only the state side
//! effect owned by that event, and advance the high-water in the same commit.

use sqlx::{PgConnection, PgPool, Row};
use stratum_core::{AgentId, ChatRole, DurableAgentEvent, TurnId};
use uuid::Uuid;

use crate::codec;
use crate::error::PostgresError;
use crate::types::{
    AppendEvent, BeginTurn, CommitReceipt, CreateAgent, CreateAgentOutcome, ResolveApproval,
    ResolveApprovalOutcome,
};

/// Constraint names produced by the baseline migration, used to map unique
/// violations to typed errors.
mod constraints {
    pub(crate) const IDEMPOTENCY_KEY: &str = "agents_idempotency_key_key";
    pub(crate) const RUNNING_SESSION: &str = "agent_state_running_session_unique";
    pub(crate) const APPROVAL_REQUESTED: &str = "durable_events_approval_requested_unique";
}

/// Locked `agent_state` row: the allocator and serialization point.
struct LockedState {
    status: crate::types::AgentStatus,
    session_id: Option<Uuid>,
    current_turn_id: Option<Uuid>,
    default_model_config: serde_json::Value,
    last_event_seq: i64,
}

impl LockedState {
    /// The sequence the next append assigns; the Agent-wide space ends at
    /// `i64::MAX`.
    fn next_event_seq(&self, agent_id: AgentId) -> Result<u64, PostgresError> {
        if self.last_event_seq == i64::MAX {
            return Err(PostgresError::SequenceOverflow { agent_id });
        }
        // last_event_seq >= 0 per schema CHECK.
        Ok((self.last_event_seq + 1) as u64)
    }
}

/// Locks the exact Agent's state row inside `tx`.
async fn lock_agent_state(
    tx: &mut PgConnection,
    agent_id: AgentId,
) -> Result<LockedState, PostgresError> {
    let row = sqlx::query(
        "SELECT status, session_id, current_turn_id, default_model_config, last_event_seq \
         FROM agent_state WHERE agent_id = $1 FOR UPDATE",
    )
    .bind(agent_id.as_uuid())
    .fetch_optional(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?
    .ok_or(PostgresError::AgentNotFound { agent_id })?;

    let status_text: String = row
        .try_get("status")
        .map_err(PostgresError::StoreUnavailable)?;
    let status = status_text.parse()?;
    Ok(LockedState {
        status,
        session_id: row
            .try_get("session_id")
            .map_err(PostgresError::StoreUnavailable)?,
        current_turn_id: row
            .try_get("current_turn_id")
            .map_err(PostgresError::StoreUnavailable)?,
        default_model_config: row
            .try_get("default_model_config")
            .map_err(PostgresError::StoreUnavailable)?,
        last_event_seq: row
            .try_get("last_event_seq")
            .map_err(PostgresError::StoreUnavailable)?,
    })
}

/// Row fetched by idempotency key during create replay.
struct ExistingAgent {
    agent_id: Uuid,
    source_template_name: String,
    creation_model_override: Option<serde_json::Value>,
}

async fn find_by_idempotency_key<'e>(
    executor: impl sqlx::PgExecutor<'e>,
    idempotency_key: Uuid,
) -> Result<Option<ExistingAgent>, PostgresError> {
    let row = sqlx::query(
        "SELECT agent_id, source_template_name, creation_model_override \
         FROM agents WHERE idempotency_key = $1",
    )
    .bind(idempotency_key)
    .fetch_optional(executor)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    row.map(|row| {
        Ok(ExistingAgent {
            agent_id: row
                .try_get("agent_id")
                .map_err(PostgresError::StoreUnavailable)?,
            source_template_name: row
                .try_get("source_template_name")
                .map_err(PostgresError::StoreUnavailable)?,
            creation_model_override: row
                .try_get("creation_model_override")
                .map_err(PostgresError::StoreUnavailable)?,
        })
    })
    .transpose()
}

/// Compares a replayed create request with the persisted one.
fn replay_outcome(
    existing: ExistingAgent,
    command: &CreateAgent,
) -> Result<CreateAgentOutcome, PostgresError> {
    let stored_override = codec::decode_optional_model_config(
        existing.creation_model_override,
        "agents.creation_model_override failed v1 decode",
    )?;
    let equivalent = existing.source_template_name == command.source_template_name
        && stored_override == command.creation_model_override;
    if equivalent {
        Ok(CreateAgentOutcome::Replay {
            agent_id: AgentId::from(existing.agent_id),
        })
    } else {
        Err(PostgresError::IdempotencyKeyConflict {
            idempotency_key: command.idempotency_key,
        })
    }
}

/// Idempotent create: idempotency-key lookup first, template re-read never
/// happens here; a miss inserts the immutable Agent and its idle state in one
/// transaction, and a concurrent same-key insert converges through the unique
/// constraint into the same replay comparison.
#[tracing::instrument(skip_all, fields(agent_id = %command.agent_id))]
pub(crate) async fn create_agent(
    pool: &PgPool,
    command: CreateAgent,
) -> Result<CreateAgentOutcome, PostgresError> {
    if let Some(existing) = find_by_idempotency_key(pool, command.idempotency_key).await? {
        return replay_outcome(existing, &command);
    }

    let creation_model_override = command
        .creation_model_override
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|source| PostgresError::EventEncode {
            event_type: "creation_model_override",
            source,
        })?;
    let default_model_config =
        serde_json::to_value(&command.default_model_config).map_err(|source| {
            PostgresError::EventEncode {
                event_type: "default_model_config",
                source,
            }
        })?;

    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let inserted = sqlx::query(
        "INSERT INTO agents (agent_id, agent_version_id, idempotency_key, source_template_name, \
             creation_model_override, definition_schema_version, resolved_definition) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(command.agent_id.as_uuid())
    .bind(command.agent_version_id.as_uuid())
    .bind(command.idempotency_key)
    .bind(&command.source_template_name)
    .bind(creation_model_override)
    .bind(codec::DEFINITION_SCHEMA_VERSION_V1)
    .bind(&command.resolved_definition)
    .execute(&mut *tx)
    .await;

    if let Err(source) = inserted {
        if codec::is_unique_violation_on(&source, constraints::IDEMPOTENCY_KEY) {
            // A concurrent create committed the same key first; the failed
            // transaction never consumed it, so re-read and apply the same
            // equivalence judgment.
            drop(tx);
            let existing = find_by_idempotency_key(pool, command.idempotency_key)
                .await?
                .ok_or(PostgresError::corrupt_invariant(
                    "idempotency key unique violation without a visible owner row",
                ))?;
            return replay_outcome(existing, &command);
        }
        return Err(PostgresError::StoreUnavailable(source));
    }

    sqlx::query(
        "INSERT INTO agent_state (agent_id, status, default_model_config) \
         VALUES ($1, 'idle', $2)",
    )
    .bind(command.agent_id.as_uuid())
    .bind(default_model_config)
    .execute(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    Ok(CreateAgentOutcome::Created {
        agent_id: command.agent_id,
    })
}

/// Admission: CAS on the expected current Turn, bind or validate the Session,
/// commit the `LoopStarted` row with its v1 runtime snapshot, and flip the
/// state to running — all in one transaction.
#[tracing::instrument(skip_all, fields(agent_id = %command.agent_id, turn_id = %command.turn_id))]
pub(crate) async fn begin_turn(
    pool: &PgPool,
    command: BeginTurn,
) -> Result<CommitReceipt, PostgresError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let state = lock_agent_state(&mut tx, command.agent_id).await?;

    if state.status == crate::types::AgentStatus::Running {
        return Err(PostgresError::AgentBusy {
            agent_id: command.agent_id,
        });
    }
    let actual_turn = state.current_turn_id.map(TurnId::from);
    if actual_turn != command.expected_current_turn_id {
        return Err(PostgresError::StaleTurn {
            agent_id: command.agent_id,
            expected: command.expected_current_turn_id,
            actual: actual_turn,
        });
    }
    if let Some(bound) = state.session_id
        && bound != command.session_id.as_uuid()
    {
        return Err(PostgresError::SessionMismatch {
            agent_id: command.agent_id,
        });
    }

    let event_seq = state.next_event_seq(command.agent_id)?;
    let event = DurableAgentEvent::LoopStarted {
        extension_set_version_id: Some(command.snapshot.extension_set_version_id),
    };
    let encoded = codec::encode_event(&event, None)?;
    let snapshot = codec::encode_runtime_snapshot(&command.snapshot)?;

    sqlx::query(
        "INSERT INTO durable_events (agent_id, event_seq, session_id, turn_id, event_type, \
             event_version, payload, runtime_snapshot_version, runtime_snapshot) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
    )
    .bind(command.agent_id.as_uuid())
    .bind(event_seq as i64)
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
        "UPDATE agent_state SET status = 'running', session_id = $2, current_turn_id = $3, \
             last_event_seq = $4, updated_at = now() \
         WHERE agent_id = $1",
    )
    .bind(command.agent_id.as_uuid())
    .bind(command.session_id.as_uuid())
    .bind(command.turn_id.as_uuid())
    .bind(event_seq as i64)
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

/// Centralized durable append used by every writer; see the module docs for
/// the transaction template.
#[tracing::instrument(skip_all, fields(agent_id = %command.agent_id, turn_id = %command.turn_id, event_type = command.event.event_type()))]
pub(crate) async fn append_event(
    pool: &PgPool,
    command: AppendEvent,
) -> Result<CommitReceipt, PostgresError> {
    validate_append_shape(&command)?;

    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let state = lock_agent_state(&mut tx, command.agent_id).await?;

    if state.session_id != Some(command.session_id.as_uuid()) {
        return Err(PostgresError::SessionMismatch {
            agent_id: command.agent_id,
        });
    }
    let actual_turn = state.current_turn_id.map(TurnId::from);
    if actual_turn != Some(command.turn_id) {
        return Err(PostgresError::StaleTurn {
            agent_id: command.agent_id,
            expected: Some(command.turn_id),
            actual: actual_turn,
        });
    }
    if state.status != crate::types::AgentStatus::Running {
        return Err(PostgresError::TurnNotRunning {
            agent_id: command.agent_id,
            turn_id: command.turn_id,
            status: state.status,
        });
    }

    let event_seq = state.next_event_seq(command.agent_id)?;
    let encoded = codec::encode_event(&command.event, command.approval_hook_invocation_id)?;

    // Fail closed before any write when the retained pointer cannot address a
    // real earlier MessageAppended of this Agent.
    if let Some(compaction) = &command.compaction {
        if compaction.retained_from_event_seq == 0
            || compaction.retained_from_event_seq >= event_seq
        {
            return Err(PostgresError::InvalidCompactionPointer {
                agent_id: command.agent_id,
                retained_from_event_seq: compaction.retained_from_event_seq,
            });
        }
        let pointer_valid: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM durable_events \
             WHERE agent_id = $1 AND event_seq = $2 AND event_type = 'message_appended')",
        )
        .bind(command.agent_id.as_uuid())
        .bind(compaction.retained_from_event_seq as i64)
        .fetch_one(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
        if !pointer_valid {
            return Err(PostgresError::InvalidCompactionPointer {
                agent_id: command.agent_id,
                retained_from_event_seq: compaction.retained_from_event_seq,
            });
        }
    }

    let inserted = sqlx::query(
        "INSERT INTO durable_events (agent_id, event_seq, session_id, turn_id, event_type, \
             event_version, payload) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(command.agent_id.as_uuid())
    .bind(event_seq as i64)
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
            "INSERT INTO transcript_compactions (agent_id, event_seq, turn_id, \
                 compacted_iteration, upto, retained_from_event_seq, summary) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(command.agent_id.as_uuid())
        .bind(event_seq as i64)
        .bind(command.turn_id.as_uuid())
        .bind(compaction.compacted_iteration as i64)
        .bind(compaction.upto as i64)
        .bind(compaction.retained_from_event_seq as i64)
        .bind(summary)
        .execute(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    }

    // Only the state fields owned by this event change; the high-water always
    // advances in the same commit.
    let terminal_status = terminal_status_of(&command.event);
    let default_model_update = match &command.default_model_update {
        Some(model) => {
            let current = codec::decode_model_config(
                state.default_model_config,
                "agent_state.default_model_config failed v1 decode",
            )?;
            // No-op when the replacement is identical to the current default.
            if &current == model {
                None
            } else {
                Some(
                    serde_json::to_value(model).map_err(|source| PostgresError::EventEncode {
                        event_type: "default_model_config",
                        source,
                    })?,
                )
            }
        }
        None => None,
    };

    sqlx::query(
        "UPDATE agent_state SET \
             status = COALESCE($2, status), \
             default_model_config = COALESCE($3, default_model_config), \
             last_event_seq = $4, updated_at = now() \
         WHERE agent_id = $1",
    )
    .bind(command.agent_id.as_uuid())
    .bind(terminal_status)
    .bind(default_model_update)
    .bind(event_seq as i64)
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
    let is_compaction = matches!(command.event, DurableAgentEvent::TranscriptCompacted { .. });
    if is_compaction != command.compaction.is_some() {
        return Err(PostgresError::InvalidCommand(
            "transcript_compacted requires exactly one compaction companion",
        ));
    }
    if command.default_model_update.is_some() {
        let is_user_message = matches!(
            &command.event,
            DurableAgentEvent::MessageAppended { message } if message.role == ChatRole::User
        );
        if !is_user_message {
            return Err(PostgresError::InvalidCommand(
                "default_model_update is only valid on a user message_appended",
            ));
        }
    }
    Ok(())
}

/// Terminal status owned by an event, when the event is terminal.
fn terminal_status_of(event: &DurableAgentEvent) -> Option<&'static str> {
    match event {
        DurableAgentEvent::LoopFinished { .. } => Some("finished"),
        DurableAgentEvent::LoopFailed { .. } => Some("failed"),
        DurableAgentEvent::LoopCancelled { .. } => Some("cancelled"),
        // Unknown and future variants are non-terminal by construction;
        // adding a new terminal variant must update this function.
        _ => None,
    }
}

/// Approval resolve: linearized with every other writer of the Agent through
/// the shared state row lock; terminal wins over every other outcome, an
/// identical earlier decision is an idempotent success, and only an
/// undecided request on a running Turn appends the unique Resolved row.
#[tracing::instrument(skip_all, fields(agent_id = %command.agent_id, turn_id = %command.turn_id))]
pub(crate) async fn resolve_approval(
    pool: &PgPool,
    command: ResolveApproval,
) -> Result<ResolveApprovalOutcome, PostgresError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let state = lock_agent_state(&mut tx, command.agent_id).await?;

    let actual_turn = state.current_turn_id.map(TurnId::from);
    if actual_turn != Some(command.turn_id) {
        return Err(PostgresError::StaleTurn {
            agent_id: command.agent_id,
            expected: Some(command.turn_id),
            actual: actual_turn,
        });
    }

    let requested = sqlx::query(
        "SELECT payload FROM durable_events \
         WHERE agent_id = $1 AND turn_id = $2 AND event_type = 'tool_approval_requested' \
             AND payload ->> 'approval_id' = $3",
    )
    .bind(command.agent_id.as_uuid())
    .bind(command.turn_id.as_uuid())
    .bind(command.approval_id.as_uuid().to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?
    .ok_or(PostgresError::ApprovalNotFound {
        approval_id: command.approval_id,
    })?;
    // Decode eagerly so a malformed request payload fails closed as corrupt.
    let payload: serde_json::Value = requested
        .try_get("payload")
        .map_err(PostgresError::StoreUnavailable)?;
    codec::RequestedApprovalPayload::decode(payload)?;

    let resolved = sqlx::query(
        "SELECT payload FROM durable_events \
         WHERE agent_id = $1 AND event_type = 'tool_approval_resolved' \
             AND payload ->> 'approval_id' = $2",
    )
    .bind(command.agent_id.as_uuid())
    .bind(command.approval_id.as_uuid().to_string())
    .fetch_optional(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    // Terminal wins over every other outcome, including an earlier decision.
    // A Turn matching current_turn_id with a non-running status is terminal:
    // the terminal event and the status update commit in one transaction.
    if state.status != crate::types::AgentStatus::Running {
        return Err(PostgresError::ApprovalInvalidated {
            approval_id: command.approval_id,
        });
    }

    if let Some(resolved) = resolved {
        let payload: serde_json::Value = resolved
            .try_get("payload")
            .map_err(PostgresError::StoreUnavailable)?;
        let resolution = codec::ResolvedApprovalPayload::decode(payload)?;
        return if resolution.decision == command.decision {
            Ok(ResolveApprovalOutcome::AlreadyResolvedSame)
        } else {
            Err(PostgresError::ApprovalAlreadyResolved {
                approval_id: command.approval_id,
            })
        };
    }

    let event_seq = state.next_event_seq(command.agent_id)?;
    let event = DurableAgentEvent::ToolApprovalResolved {
        approval_id: command.approval_id,
        decision: command.decision,
    };
    let encoded = codec::encode_event(&event, None)?;

    sqlx::query(
        "INSERT INTO durable_events (agent_id, event_seq, session_id, turn_id, event_type, \
             event_version, payload) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(command.agent_id.as_uuid())
    .bind(event_seq as i64)
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

    sqlx::query(
        "UPDATE agent_state SET last_event_seq = $2, updated_at = now() WHERE agent_id = $1",
    )
    .bind(command.agent_id.as_uuid())
    .bind(event_seq as i64)
    .execute(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;

    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    Ok(ResolveApprovalOutcome::Resolved {
        receipt: CommitReceipt { event_seq },
    })
}
