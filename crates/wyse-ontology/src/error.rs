//! Error types for ontology domain operations.

use thiserror::Error;

/// Error returned by ontology domain operations.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
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
}
