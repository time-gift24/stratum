//! Error types for Stratum core values.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Error returned when an Agent definition name is outside its stable boundary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AgentNameParseError {
    /// The name is empty.
    #[error("agent name must not be empty")]
    Empty,
    /// The name exceeds the durable protocol limit.
    #[error("agent name must be at most 64 bytes")]
    TooLong,
    /// The first byte is not an ASCII letter or digit.
    #[error("agent name must start with an ASCII letter or digit")]
    InvalidStart,
    /// A byte is not an ASCII letter, digit, underscore, or hyphen.
    #[error("agent name contains an invalid character")]
    InvalidCharacter,
}

/// Error returned when a model id is not canonical.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ModelIdParseError {
    /// The value is not exactly `provider:model`.
    #[error("model id must use provider:model")]
    InvalidFormat,
}

/// Error returned when an Agent template version tag is invalid.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AgentVersionTagParseError {
    /// The tag is empty.
    #[error("agent version tag must not be empty")]
    Empty,
    /// The UTF-8 representation exceeds the durable protocol limit.
    #[error("agent version tag must be at most 128 bytes")]
    TooLong,
    /// The tag contains a Unicode control character.
    #[error("agent version tag must not contain control characters")]
    ControlCharacter,
    /// The tag starts or ends with whitespace.
    #[error("agent version tag must not have leading or trailing whitespace")]
    SurroundingWhitespace,
}

/// Error returned when a SHA-256 fingerprint is not canonical lowercase hexadecimal.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum FingerprintParseError {
    /// The value is not exactly 64 lowercase hexadecimal characters.
    #[error("fingerprint must be 64 lowercase hexadecimal characters")]
    InvalidFormat,
}

/// Typed failure produced by a decision-affecting hook invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Error)]
#[serde(rename_all = "snake_case")]
pub enum HookFailure {
    /// The semantic invocation address does not match the persisted record.
    #[error("hook invocation address mismatch")]
    AddressMismatch,
    /// The pinned handler version does not match the persisted record.
    #[error("hook handler version mismatch")]
    VersionMismatch,
    /// The invocation input digest does not match the persisted record.
    #[error("hook invocation input mismatch")]
    InputMismatch,
    /// The pinned handler cannot be resolved.
    #[error("pinned hook handler unavailable")]
    HandlerUnavailable,
    /// The handler returned an invalid decision.
    #[error("invalid hook output")]
    InvalidOutput,
    /// The handler failed without exposing sensitive details.
    #[error("hook handler failed")]
    HandlerFailed,
    /// The invocation exceeded its deadline.
    #[error("hook invocation timed out")]
    TimedOut,
    /// The invocation was cancelled.
    #[error("hook invocation cancelled")]
    Cancelled,
}
