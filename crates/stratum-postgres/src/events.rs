//! Postgres durable event backend for one agent-loop run.
//!
//! Every event is one row in `durable_events` with materialized addressing
//! columns (`session_id`, `agent_id`, `turn_id`, per-run `seq`, `event_type`)
//! and the complete `{"type","data"}` event JSON as the `payload` (jsonb).
//! The per-run `seq` is 1-based to mirror `events.jsonl` line numbers, and is
//! assigned by the sink under its append lock; `UNIQUE (turn_id, seq)` is the
//! fail-closed backstop against duplicated or out-of-order writes. Each
//! `append` is a single-statement transaction: it returns only after the row
//! is committed, matching the filesystem sink's fsync acknowledgement.
//!
//! jsonb does not preserve byte-level key order or whitespace, so "identical
//! to the jsonl line" means field-by-field equality after deserialization,
//! which is exactly what [`read_events`] verifies on replay.

use async_trait::async_trait;
use sqlx::{PgPool, Row};
use stratum_core::{AgentId, DurableAgentEvent, SessionId, TurnId};
use stratum_infra::{DurableEventSink, DurableEventSinkError};
use tokio::sync::Mutex;

use crate::PostgresEventSinkError;

/// Postgres SQLSTATE of a unique-constraint violation.
const UNIQUE_VIOLATION_SQLSTATE: &str = "23505";

/// Append state serialized by the sink's internal async lock.
struct AppendState {
    /// Per-run sequence assigned to the next appended event; 1-based.
    next_seq: u64,
}

/// Durable event sink appending one `durable_events` row per event.
///
/// The sink is bound to one run (`session_id`, `agent_id`, `turn_id`) at
/// construction, analogous to the filesystem sink's run directory. Appends
/// are serialized internally, so concurrent writers never interleave and the
/// committed order matches the append order. One sink owns one run; opening a
/// second sink for the same `turn_id` resumes numbering after the last
/// persisted `seq`, and any stale duplicate is rejected by the unique
/// constraint as [`PostgresEventSinkError::DuplicateSequence`].
pub struct PostgresDurableEventSink {
    pool: PgPool,
    session_id: SessionId,
    agent_id: AgentId,
    turn_id: TurnId,
    append_state: Mutex<AppendState>,
}

impl PostgresDurableEventSink {
    /// Opens a sink for one run, continuing per-run numbering after the last
    /// persisted sequence so a resumed run appends after its existing log.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresEventSinkError::Read`] when the run's sequence
    /// frontier cannot be read.
    pub async fn open(
        pool: PgPool,
        session_id: SessionId,
        agent_id: AgentId,
        turn_id: TurnId,
    ) -> Result<Self, PostgresEventSinkError> {
        let max_seq: Option<i64> =
            sqlx::query_scalar("SELECT max(seq) FROM durable_events WHERE turn_id = $1")
                .bind(turn_id.as_uuid())
                .fetch_one(&pool)
                .await
                .map_err(|source| PostgresEventSinkError::Read { turn_id, source })?;
        let next_seq = match max_seq {
            None => 1,
            Some(seq) => u64::try_from(seq)
                .ok()
                .and_then(|seq| seq.checked_add(1))
                .ok_or(PostgresEventSinkError::SequenceOverflow { turn_id })?,
        };
        Ok(Self {
            pool,
            session_id,
            agent_id,
            turn_id,
            append_state: Mutex::new(AppendState { next_seq }),
        })
    }

    /// Returns the turn this sink's run belongs to.
    #[must_use]
    pub const fn turn_id(&self) -> TurnId {
        self.turn_id
    }
}

#[async_trait]
impl DurableEventSink for PostgresDurableEventSink {
    async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
        let event_type = event.event_type();
        let payload = serde_json::to_value(&event)
            .map_err(|source| PostgresEventSinkError::Serialize { event_type, source })
            .map_err(DurableEventSinkError::backend)?;
        // Hold the async-aware lock across the insert so concurrent appends
        // stay serialized and committed order matches append order.
        let mut state = self.append_state.lock().await;
        tracing::debug!(
            event_type,
            turn_id = %self.turn_id,
            seq = state.next_seq,
            "appending durable event"
        );
        let seq = i64::try_from(state.next_seq).map_err(|_| {
            DurableEventSinkError::backend(PostgresEventSinkError::SequenceOverflow {
                turn_id: self.turn_id,
            })
        })?;
        sqlx::query(
            "INSERT INTO durable_events (session_id, agent_id, turn_id, seq, event_type, payload) \
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(self.session_id.as_uuid())
        .bind(self.agent_id.as_uuid())
        .bind(self.turn_id.as_uuid())
        .bind(seq)
        .bind(event_type)
        .bind(&payload)
        .execute(&self.pool)
        .await
        .map_err(|source| {
            DurableEventSinkError::backend(map_append_error(self.turn_id, state.next_seq, source))
        })?;
        state.next_seq += 1;
        Ok(())
    }
}

impl From<PostgresEventSinkError> for DurableEventSinkError {
    fn from(source: PostgresEventSinkError) -> Self {
        Self::backend(source)
    }
}

/// Maps a failed insert to a typed sink error; the unique-constraint backstop
/// becomes [`PostgresEventSinkError::DuplicateSequence`].
fn map_append_error(turn_id: TurnId, seq: u64, source: sqlx::Error) -> PostgresEventSinkError {
    if let sqlx::Error::Database(database_error) = &source
        && database_error.code().as_deref() == Some(UNIQUE_VIOLATION_SQLSTATE)
    {
        return PostgresEventSinkError::DuplicateSequence { turn_id, seq };
    }
    PostgresEventSinkError::Append { turn_id, source }
}

/// Reads the durable events recorded for `turn_id`, in append order.
///
/// Semantics:
///
/// - a run that never persisted anything yields an empty event stream;
/// - read failures are typed errors;
/// - a malformed payload is a typed error, because single-transaction inserts
///   cannot leave a partially written row behind (unlike a crash-torn jsonl
///   tail line, which the filesystem reader tolerates).
///
/// # Errors
///
/// Returns [`PostgresEventSinkError::Read`] when the log cannot be read,
/// [`PostgresEventSinkError::InvalidSequence`] when a persisted sequence is
/// not representable, and [`PostgresEventSinkError::MalformedEvent`] when a
/// payload fails to parse.
pub async fn read_events(
    pool: &PgPool,
    turn_id: TurnId,
) -> Result<Vec<DurableAgentEvent>, PostgresEventSinkError> {
    let rows =
        sqlx::query("SELECT seq, payload FROM durable_events WHERE turn_id = $1 ORDER BY seq ASC")
            .bind(turn_id.as_uuid())
            .fetch_all(pool)
            .await
            .map_err(|source| PostgresEventSinkError::Read { turn_id, source })?;
    let mut events = Vec::with_capacity(rows.len());
    for row in rows {
        let raw_seq: i64 = row
            .try_get("seq")
            .map_err(|source| PostgresEventSinkError::Read { turn_id, source })?;
        let seq = u64::try_from(raw_seq).map_err(|_| PostgresEventSinkError::InvalidSequence {
            turn_id,
            seq: raw_seq,
        })?;
        let payload: serde_json::Value = row
            .try_get("payload")
            .map_err(|source| PostgresEventSinkError::Read { turn_id, source })?;
        let event = serde_json::from_value(payload).map_err(|source| {
            PostgresEventSinkError::MalformedEvent {
                turn_id,
                seq,
                source,
            }
        })?;
        events.push(event);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use serde_json::json;
    use stratum_core::{ChatMessage, TokenUsage};

    use super::*;

    #[derive(Debug)]
    struct MockDatabaseError {
        code: &'static str,
    }

    impl std::fmt::Display for MockDatabaseError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "mock database error {}", self.code)
        }
    }

    impl std::error::Error for MockDatabaseError {}

    impl sqlx::error::DatabaseError for MockDatabaseError {
        fn message(&self) -> &str {
            "mock database error"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.code))
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
    }

    #[test]
    fn unique_violation_maps_to_duplicate_sequence() {
        let turn_id = TurnId::new();
        let error = sqlx::Error::Database(Box::new(MockDatabaseError { code: "23505" }));

        let mapped = map_append_error(turn_id, 7, error);

        assert!(matches!(
            mapped,
            PostgresEventSinkError::DuplicateSequence { seq: 7, .. }
        ));
    }

    #[test]
    fn other_database_errors_map_to_append() {
        let turn_id = TurnId::new();
        let error = sqlx::Error::Database(Box::new(MockDatabaseError { code: "40001" }));

        let mapped = map_append_error(turn_id, 7, error);

        assert!(matches!(mapped, PostgresEventSinkError::Append { .. }));
    }

    #[test]
    fn event_payload_round_trips_field_by_field() {
        let usage = TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            total_tokens: 3,
        };
        let events = vec![
            DurableAgentEvent::LoopStarted {
                extension_set_version_id: None,
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("hello 中 🚀"),
            },
            DurableAgentEvent::TranscriptCompacted {
                upto: 2,
                summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary"),
                compacted_iteration: 1,
            },
            DurableAgentEvent::IterationCompleted {
                iteration: 1,
                usage,
            },
            DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage,
            },
        ];

        for event in events {
            // jsonb normalizes key order and whitespace, so the contract is
            // field-by-field equality of the deserialized value.
            let payload = serde_json::to_value(&event).expect("event serializes");
            assert_eq!(payload["type"], json!(event.event_type()));
            assert_eq!(
                serde_json::from_value::<DurableAgentEvent>(payload).expect("payload parses"),
                event
            );
        }
    }
}
