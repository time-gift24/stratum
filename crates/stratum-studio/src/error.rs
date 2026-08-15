use thiserror::Error;

use crate::ProviderKind;

/// Failures while reading or changing the Studio management catalog.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StudioError {
    /// The Studio database could not be queried or migrated.
    #[error("studio database operation failed")]
    Database(#[from] sqlx::Error),
    /// The Studio database migration history could not be applied.
    #[error("studio database migration failed")]
    Migration(#[from] sqlx::migrate::MigrateError),
    /// The catalog has not been bootstrapped from the configured read-only sources.
    #[error("studio catalog has not been initialized")]
    NotInitialized,
    /// A requested management resource does not exist.
    #[error("studio resource was not found")]
    NotFound,
    /// A management resource already exists.
    #[error("studio resource already exists")]
    AlreadyExists,
    /// The caller's resource version is stale.
    #[error("studio resource version does not match")]
    PreconditionFailed,
    /// A definition attempted to select a model absent from the Studio catalog.
    #[error("studio model is not configured")]
    ModelNotConfigured,
    /// A definition's version must change whenever its behavior changes.
    #[error("agent definition updates require a new version tag")]
    AgentVersionUnchanged,
    /// A destructive operation would leave a live management reference dangling.
    #[error("studio resource is still referenced")]
    DeletionBlocked {
        /// Resources that must be changed or deleted first.
        blockers: Vec<DeletionBlocker>,
    },
    /// Input failed a stable Studio validation rule.
    #[error("invalid studio field `{field}`")]
    InvalidInput {
        /// Stable path of the rejected input field.
        field: &'static str,
    },
    /// A persisted value no longer satisfies its typed boundary.
    #[error("studio catalog contains an invalid `{field}` value")]
    CatalogCorrupt {
        /// Stable field name that could not be decoded.
        field: &'static str,
    },
}

/// A managed resource that prevents a delete operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeletionBlocker {
    /// Resource category, such as `model` or `agent_definition`.
    pub resource: &'static str,
    /// Stable resource identifier.
    pub name: String,
}

impl DeletionBlocker {
    pub(crate) fn model(kind: ProviderKind, name: String) -> Self {
        Self {
            resource: "model",
            name: format!("{}:{name}", kind.as_str()),
        }
    }

    pub(crate) fn agent_definition(name: String) -> Self {
        Self {
            resource: "agent_definition",
            name,
        }
    }
}
