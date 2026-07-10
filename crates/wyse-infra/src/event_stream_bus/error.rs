//! Error types for event stream bus operations.

use std::error::Error;

use thiserror::Error;
use wyse_core::EventCursor;

/// Error returned by event stream bus operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EventStreamBusError {
    /// Published envelope does not belong to an agent.
    #[error("stream envelope is missing agent scope")]
    MissingAgentScope,
    /// Requested cursor is no longer retained for the subscribed agent.
    #[error("event cursor {cursor:?} is no longer retained")]
    CursorExpired {
        /// Cursor requested by the subscriber.
        cursor: EventCursor,
    },
    /// Event envelope serialization failed.
    #[error("failed to serialize stream envelope")]
    Serialize(#[source] serde_json::Error),
    /// Event envelope deserialization failed.
    #[error("failed to deserialize stream envelope")]
    Deserialize(#[source] serde_json::Error),
    /// NATS operation failed.
    #[error("nats operation failed")]
    Nats {
        /// Underlying NATS error.
        #[source]
        source: Box<dyn Error + Send + Sync + 'static>,
    },
}

impl EventStreamBusError {
    pub(crate) fn nats(source: impl Error + Send + Sync + 'static) -> Self {
        Self::Nats {
            source: Box::new(source),
        }
    }
}
