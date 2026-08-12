//! Typed Ontology domain and persistence failures.

use thiserror::Error;

use crate::Violation;

/// Failure while parsing a typed UUIDv7 identity.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum IdParseError {
    /// The value was not a UUID.
    #[error("id is not a UUID")]
    InvalidUuid {
        /// The UUID parser failure.
        #[source]
        source: uuid::Error,
    },
    /// The UUID did not use version 7.
    #[error("id is not a UUIDv7")]
    NotUuidV7,
}

/// Failure returned by the pure Ontology domain boundary.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OntologyError {
    /// A complete candidate violates one or more schema rules.
    #[error("ontology schema is invalid")]
    Validation {
        /// All independently detectable violations in deterministic order.
        violations: Vec<Violation>,
    },
}

/// Failure returned by [`crate::OntologyStore`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum OntologyStoreError {
    /// Connecting to PostgreSQL failed.
    #[error("ontology store connection failed")]
    Connection {
        /// Original database error, deliberately redacted at API boundaries.
        #[source]
        source: sqlx::Error,
    },
    /// Running the embedded schema migration failed.
    #[error("ontology store migration failed")]
    Migration {
        /// Original migration error, deliberately redacted at API boundaries.
        #[source]
        source: sqlx::migrate::MigrateError,
    },
    /// A root Ontology name conflicts with an existing Ontology.
    #[error("ontology name already exists")]
    NameConflict {
        /// Original unique-constraint error, deliberately redacted at API boundaries.
        #[source]
        source: sqlx::Error,
    },
    /// A typed child identity is already owned by an existing Ontology.
    #[error("ontology entity id already exists")]
    EntityIdConflict {
        /// Original unique-constraint error, deliberately redacted at API boundaries.
        #[source]
        source: sqlx::Error,
    },
    /// The requested Ontology does not exist.
    #[error("ontology not found")]
    NotFound,
    /// The requested Object Type does not belong to the requested Ontology.
    #[error("object type not found")]
    ObjectTypeNotFound,
    /// A neighborhood depth was outside the supported range.
    #[error("neighborhood depth is invalid")]
    InvalidDepth,
    /// The supplied root revision was no longer current.
    #[error("ontology revision is stale")]
    Stale,
    /// Persisted data did not satisfy the crate's canonical invariant.
    #[error("stored ontology data is invalid")]
    CorruptData,
    /// The schema candidate was invalid before persistence began.
    #[error(transparent)]
    Validation(#[from] OntologyError),
    /// PostgreSQL was unavailable while serving a request.
    #[error("ontology store is unavailable")]
    Unavailable {
        /// Original database error, deliberately redacted at API boundaries.
        #[source]
        source: sqlx::Error,
    },
    /// PostgreSQL returned an unexpected error.
    #[error("ontology store operation failed")]
    Database {
        /// Original database error, deliberately redacted at API boundaries.
        #[source]
        source: sqlx::Error,
    },
}
