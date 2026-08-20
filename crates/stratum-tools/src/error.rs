//! Error types for tool operations.

use std::borrow::Cow;
use std::io;

use stratum_core::ToolName;
use stratum_filesystem::VirtualPathError;
use thiserror::Error;

/// Error returned by tool registry or execution operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ToolError {
    /// Tool execution was cancelled.
    #[error("tool execution cancelled")]
    Cancelled,
    /// A tool with this name is already registered.
    #[error("tool is already registered: {name}")]
    DuplicateTool {
        /// Duplicate tool name.
        name: ToolName,
    },
    /// No tool with this name is registered.
    #[error("tool not found: {name}")]
    ToolNotFound {
        /// Missing tool name.
        name: ToolName,
    },
    /// Tool-set fingerprint input could not be encoded canonically.
    #[error("failed to encode tool-set fingerprint input")]
    FingerprintEncoding {
        /// JSON encoding failure source.
        #[source]
        source: serde_json::Error,
    },
    /// Tool input could not be decoded.
    #[error("invalid tool input")]
    InvalidInput {
        /// Decode failure source.
        #[source]
        source: serde_json::Error,
    },
    /// Tool operation type is unknown.
    #[error("invalid tool operation: {operation}")]
    InvalidOperation {
        /// Rejected operation type.
        operation: String,
    },
    /// Tool path is invalid.
    #[error("invalid path: {path}")]
    InvalidPath {
        /// Rejected path.
        path: String,
        /// Path validation source.
        #[source]
        source: VirtualPathError,
    },
    /// Tool argument is semantically invalid.
    #[error("invalid argument {name}: {reason}")]
    InvalidArgument {
        /// Argument name.
        name: &'static str,
        /// Rejection reason.
        reason: Cow<'static, str>,
    },
    /// Tool input schema is not a valid or compilable JSON Schema document.
    #[error("invalid input schema for tool {name}: {reason}")]
    InvalidInputSchema {
        /// Tool carrying the invalid schema.
        name: ToolName,
        /// Schema rejection reason.
        reason: String,
    },
    /// A shell process could not be spawned, waited for, or terminated.
    #[error("shell process operation failed: {operation}")]
    Process {
        /// Process operation that failed.
        operation: &'static str,
        /// Operating-system failure source.
        #[source]
        source: io::Error,
    },
}
