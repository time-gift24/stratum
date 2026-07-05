//! Error types for LLM operations.

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LlmError {
    #[error("unsupported capability: {0}")]
    UnsupportedCapability(&'static str),
}

/// Provider HTTP status error details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatusError {
    /// HTTP status code.
    pub status: u16,
    /// Provider-specific error code.
    pub code: Option<String>,
    /// Provider error message.
    pub message: String,
    /// Provider request id when available.
    pub request_id: Option<String>,
}
