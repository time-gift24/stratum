//! Typed failures that prevent safe tool execution.

use stratum_infra::DurableEventSinkError;
use thiserror::Error;

/// Failure that prevents the tool executor from preserving its durable ordering.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ToolExecutorError {
    /// A required pre-execution event was not acknowledged.
    #[error("durable tool event was not acknowledged")]
    Durability {
        /// Durable event sink failure.
        #[source]
        source: DurableEventSinkError,
    },
    /// Cancellation prevented the execution boundary.
    #[error("tool execution cancelled")]
    Cancelled,
}

impl From<DurableEventSinkError> for ToolExecutorError {
    fn from(source: DurableEventSinkError) -> Self {
        Self::Durability { source }
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn durability_conversion_preserves_typed_source() {
        let error = ToolExecutorError::from(DurableEventSinkError::UnsupportedEvent {
            event_type: "future_event",
        });

        assert!(matches!(&error, ToolExecutorError::Durability { .. }));
        assert!(matches!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<DurableEventSinkError>()),
            Some(DurableEventSinkError::UnsupportedEvent {
                event_type: "future_event"
            })
        ));
    }
}
