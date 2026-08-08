//! Errors returned by durable agent event sinks.

use thiserror::Error;

/// Error returned when a durable agent-loop event cannot be acknowledged.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DurableEventSinkError {
    /// The sink does not know how to project a newer durable event variant.
    #[error("unsupported durable agent event type {event_type}")]
    UnsupportedEvent {
        /// Stable type name of the unsupported event.
        event_type: &'static str,
    },
    /// A durable event sink backend failed to persist the event.
    #[error("durable event sink backend failed")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}

impl DurableEventSinkError {
    /// Wraps a durable sink backend failure.
    pub fn backend(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Backend(Box::new(source))
    }
}
