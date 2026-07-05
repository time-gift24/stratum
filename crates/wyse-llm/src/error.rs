//! Error types for LLM operations.

use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum LlmError {
    #[error("unsupported capability: {0}")]
    UnsupportedCapability(&'static str),
}
