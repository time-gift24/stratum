//! Errors returned by durable agent event sinks.

use std::path::PathBuf;

use thiserror::Error;

use crate::EventStreamBusError;

/// Error returned when a durable agent-loop event cannot be acknowledged.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DurableEventSinkError {
    /// The configured event stream bus rejected the durable event.
    #[error("durable agent event publish failed")]
    EventStreamBus(#[from] EventStreamBusError),
    /// The sink does not know how to project a newer durable event variant.
    #[error("unsupported durable agent event type {event_type}")]
    UnsupportedEvent {
        /// Stable type name of the unsupported event.
        event_type: &'static str,
    },
    /// The sink's ordered publisher is no longer available.
    #[error("durable agent event publisher is unavailable")]
    PublisherUnavailable,
    /// The filesystem sink could not durably persist the event.
    #[error("filesystem durable event sink failed")]
    Filesystem(#[from] FilesystemEventSinkError),
}

/// Error returned by filesystem durable event persistence or replay.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum FilesystemEventSinkError {
    /// A durable event could not be serialized to JSON.
    #[error("failed to serialize durable event {event_type}")]
    Serialize {
        /// Stable type name of the event.
        event_type: &'static str,
        /// Serialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// The run directory could not be created.
    #[error("failed to create run directory {}", path.display())]
    CreateRunDir {
        /// Run directory that could not be created.
        path: PathBuf,
        /// Underlying IO failure.
        #[source]
        source: std::io::Error,
    },
    /// An event line could not be appended or fsynced.
    #[error("failed to append durable event to {}", path.display())]
    Append {
        /// Event log file that could not be written.
        path: PathBuf,
        /// Underlying IO failure.
        #[source]
        source: std::io::Error,
    },
    /// A compaction checkpoint line could not be appended or fsynced.
    #[error("failed to append compaction checkpoint to {}", path.display())]
    AppendCheckpoint {
        /// Checkpoint index file that could not be written.
        path: PathBuf,
        /// Underlying IO failure.
        #[source]
        source: std::io::Error,
    },
    /// The run directory could not be fsynced after an append.
    #[error("failed to sync run directory {}", path.display())]
    SyncRunDir {
        /// Run directory that could not be synced.
        path: PathBuf,
        /// Underlying IO failure.
        #[source]
        source: std::io::Error,
    },
    /// The event log exists but could not be read.
    #[error("failed to read durable events from {}", path.display())]
    Read {
        /// Event log file that could not be read.
        path: PathBuf,
        /// Underlying IO failure.
        #[source]
        source: std::io::Error,
    },
    /// A non-tail line of the event log is malformed.
    #[error("malformed durable event at {} line {line}", path.display())]
    MalformedEvent {
        /// Event log file containing the malformed line.
        path: PathBuf,
        /// 1-based line number of the malformed event.
        line: u64,
        /// Underlying JSON parse failure.
        #[source]
        source: serde_json::Error,
    },
    /// The blocking append task failed to join.
    #[error("durable event append task failed to join")]
    Join {
        /// Join failure of the blocking append task.
        #[source]
        source: tokio::task::JoinError,
    },
}
