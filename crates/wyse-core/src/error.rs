//! Error types for Wyse core values.

use thiserror::Error;

/// Error returned when a model reference is not canonical.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelRefParseError {
    /// The value is not exactly `provider:model`.
    #[error("model reference must use provider:model")]
    InvalidFormat,
}
