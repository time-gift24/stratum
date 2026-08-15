//! Durable Studio management catalog for Stratum.
//!
//! This crate owns the PostgreSQL persistence of mutable Provider credentials,
//! Models, and Agent definition authoring state. It is deliberately separate
//! from the execution ledger in `stratum-postgres`: management writes never
//! create execution events or mutate existing AgentRuntime state.

mod error;
mod store;
mod types;

pub use error::{DeletionBlocker, StudioError};
pub use store::StudioStore;
pub use types::{
    AgentDefinition, AgentDefinitionInput, ManagedModel, ProviderKind, ProviderKindParseError,
    ProviderSeed, ProviderSummary, ResourceVersion, ResourceVersionParseError, RuntimeProvider,
    StudioCatalogSeed, Versioned,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn studio_error_does_not_expose_database_details() {
        let error = StudioError::Database(sqlx::Error::PoolTimedOut);

        assert_eq!(error.to_string(), "studio database operation failed");
    }
}
