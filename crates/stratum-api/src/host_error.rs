//! Host assembly errors.
//!
//! Startup failures cross no HTTP boundary; they are logged by `main` and
//! abort the process. Messages stay safe: no credentials, no host paths.

use stratum_config::ConfigError;
use stratum_core::{ModelId, ModelIdParseError};
use stratum_llm::LlmError;
use stratum_ontology::OntologyStoreError;
use stratum_postgres::PostgresError;
use stratum_studio::StudioError;
use thiserror::Error;

/// Error returned while assembling or running the API host.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum HostError {
    /// Listener or signal I/O failed.
    #[error("host io operation failed")]
    Io(#[from] std::io::Error),
    /// Shared configuration is invalid or incomplete.
    #[error("configuration error")]
    Config(#[from] ConfigError),
    /// Postgres (the core readiness dependency) could not connect or migrate.
    #[error("postgres execution store failed to initialize")]
    Postgres(#[from] PostgresError),
    /// Ontology PostgreSQL could not connect or migrate.
    #[error("ontology store failed to initialize")]
    Ontology(#[from] OntologyStoreError),
    /// The isolated Studio catalog could not connect, migrate, or be read.
    #[error("studio management catalog failed to initialize")]
    Studio(#[from] StudioError),
    /// LLM provider registration failed.
    #[error("llm provider registration failed")]
    Llm(#[from] LlmError),
    /// An injected adapter registry does not match Studio Model identities.
    #[error("provider registry does not match studio catalog")]
    ProviderCatalogMismatch,
    /// A Studio-managed Provider model name could not form a model id.
    #[error("invalid managed model for provider {provider}")]
    InvalidManagedModel {
        /// Provider whose model failed to parse.
        provider: &'static str,
        /// Persisted Provider-local model name.
        model: String,
        /// Parse failure.
        #[source]
        source: ModelIdParseError,
    },
    /// A persisted DeepSeek model is unsupported by the built-in adapter.
    #[error("unsupported deepseek model: {model}")]
    UnsupportedDeepSeekModel {
        /// Persisted model identity.
        model: ModelId,
    },
    /// The tracing subscriber or OTLP exporter could not be initialized.
    #[error("telemetry initialization failed")]
    Telemetry(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}
