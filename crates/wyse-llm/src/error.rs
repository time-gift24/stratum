//! Error types for LLM operations.

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LlmError {
    #[error("unsupported capability: {0}")]
    UnsupportedCapability(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatusError {
    pub status: u16,
    pub code: Option<String>,
    pub message: String,
    pub request_id: Option<String>,
}
