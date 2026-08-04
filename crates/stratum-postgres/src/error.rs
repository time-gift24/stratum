//! Error types for the Postgres execution-storage backend.

use stratum_core::TurnId;
use thiserror::Error;

/// Error returned when connecting to or migrating a Postgres backend.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PostgresError {
    /// The Postgres connection could not be established.
    #[error("failed to connect to postgres")]
    Connect(#[source] sqlx::Error),
    /// Schema migrations could not be applied.
    #[error("failed to migrate postgres schema")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

/// Error returned by Postgres durable event persistence or replay.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PostgresEventSinkError {
    /// A durable event could not be serialized to JSON.
    #[error("failed to serialize durable event {event_type}")]
    Serialize {
        /// Stable type name of the event.
        event_type: &'static str,
        /// Serialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// An event row could not be appended or committed.
    #[error("failed to append durable event for turn {turn_id}")]
    Append {
        /// Run that rejected the event.
        turn_id: TurnId,
        /// Underlying database failure.
        #[source]
        source: sqlx::Error,
    },
    /// The per-run sequence already exists in the event log; the database
    /// unique constraint rejected an out-of-order or duplicated write.
    #[error("duplicate durable event sequence {seq} for turn {turn_id}")]
    DuplicateSequence {
        /// Run that rejected the event.
        turn_id: TurnId,
        /// Conflicting per-run sequence.
        seq: u64,
    },
    /// The next per-run sequence cannot be represented.
    #[error("durable event sequence overflow for turn {turn_id}")]
    SequenceOverflow {
        /// Run whose sequence space is exhausted.
        turn_id: TurnId,
    },
    /// The event log could not be read.
    #[error("failed to read durable events for turn {turn_id}")]
    Read {
        /// Run whose event log could not be read.
        turn_id: TurnId,
        /// Underlying database failure.
        #[source]
        source: sqlx::Error,
    },
    /// A persisted event sequence cannot be represented.
    #[error("invalid durable event sequence {seq} for turn {turn_id}")]
    InvalidSequence {
        /// Run containing the invalid sequence.
        turn_id: TurnId,
        /// Persisted sequence value.
        seq: i64,
    },
    /// A persisted event payload is malformed.
    #[error("malformed durable event for turn {turn_id} at sequence {seq}")]
    MalformedEvent {
        /// Run containing the malformed event.
        turn_id: TurnId,
        /// Per-run sequence of the malformed event.
        seq: u64,
        /// Underlying JSON parse failure.
        #[source]
        source: serde_json::Error,
    },
}
