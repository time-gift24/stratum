//! Errors returned by the AgentRuntime-scoped NATS tail transport.

use std::error::Error;

use thiserror::Error;

use super::AgentRuntimeTailCursor;

/// Error returned by AgentRuntime tail publish, subscribe, or connection operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AgentRuntimeTailError {
    /// The requested cursor position has been discarded by retention.
    ///
    /// The caller must fall back to a cold Postgres recovery path; this is
    /// never silently downgraded to a full tail replay.
    #[error("agent runtime tail cursor {cursor} is no longer retained")]
    CursorExpired {
        /// Cursor requested by the subscriber.
        cursor: AgentRuntimeTailCursor,
    },
    /// AgentRuntime tail configuration is invalid.
    #[error("invalid agent runtime tail configuration: {reason}")]
    InvalidConfig {
        /// Invalid configuration condition.
        reason: &'static str,
    },
    /// The underlying NATS operation failed.
    #[error("agent runtime tail nats operation failed")]
    Nats {
        /// Underlying NATS error.
        #[source]
        source: Box<dyn Error + Send + Sync + 'static>,
    },
}

impl AgentRuntimeTailError {
    pub(crate) fn nats(source: impl Into<Box<dyn Error + Send + Sync + 'static>>) -> Self {
        Self::Nats {
            source: source.into(),
        }
    }
}

/// Error returned when parsing an opaque tail cursor from its string form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid agent runtime tail cursor")]
pub struct AgentRuntimeTailCursorParseError;
