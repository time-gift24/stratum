//! Error types for ontology domain operations.

use thiserror::Error;

use crate::DraftName;
use wyse_filesystem::FilesystemError;

/// Error returned by ontology domain operations.
#[derive(Debug, Error)]
pub enum OntologyError {
    /// A draft name does not satisfy the identifier format.
    #[error(
        "draft name must be 1-64 ASCII letters, digits, underscores, or hyphens and start with a letter or digit"
    )]
    InvalidDraftName,
    /// A tag name does not satisfy the identifier format.
    #[error(
        "tag name must be 1-64 ASCII letters, digits, underscores, or hyphens and start with a letter or digit"
    )]
    InvalidTagName,
    /// A revision id is not a lowercase SHA-256 digest.
    #[error("revision id must be a 64-character lowercase hexadecimal SHA-256 digest")]
    InvalidRevisionId,
    /// A schema violates one or more structural invariants.
    #[error("schema is invalid")]
    SchemaInvalid {
        /// Every discovered validation failure.
        diagnostics: Vec<String>,
    },
    /// A requested draft does not exist.
    #[error("draft {name} does not exist")]
    DraftMissing {
        /// Name of the missing draft.
        name: DraftName,
    },
    /// A draft changed since the caller's expected digest.
    #[error("draft {name} changed concurrently")]
    DraftConflict {
        /// Name of the conflicted draft.
        name: DraftName,
    },
    /// The filesystem backend does not support the required CAS operations.
    #[error("draft filesystem does not support compare-and-swap")]
    DraftCasUnsupported,
    /// A persisted draft schema is not valid JSON.
    #[error("failed to decode draft schema")]
    DecodeSchema(#[source] serde_json::Error),
    /// A schema cannot be encoded as its canonical JSON form.
    #[error("failed to encode schema")]
    EncodeSchema(#[source] serde_json::Error),
    /// A filesystem operation failed.
    #[error("draft filesystem operation failed")]
    Filesystem(#[source] FilesystemError),
}
