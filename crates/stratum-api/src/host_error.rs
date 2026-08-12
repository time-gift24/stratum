//! Host assembly errors.
//!
//! Startup failures cross no HTTP boundary; they are logged by `main` and
//! abort the process. Messages stay safe: no credentials, no host paths.

use stratum_config::ConfigError;
use stratum_core::{ModelId, ModelIdParseError};
use stratum_filesystem::FilesystemError;
use stratum_llm::LlmError;
use stratum_postgres::PostgresError;
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
    /// The template catalog root is missing, not a directory, or unreadable.
    #[error("agent template catalog root is not a readable directory")]
    TemplatesRoot(#[from] FilesystemError),
    /// LLM provider registration failed.
    #[error("llm provider registration failed")]
    Llm(#[from] LlmError),
    /// A configured provider model name could not form a model id.
    #[error("invalid configured model for provider {provider}")]
    InvalidConfiguredModel {
        /// Provider whose model failed to parse.
        provider: &'static str,
        /// Configured model name.
        model: String,
        /// Parse failure.
        #[source]
        source: ModelIdParseError,
    },
    /// A DeepSeek model is configured but unsupported by the adapter.
    #[error("unsupported deepseek model: {model}")]
    UnsupportedDeepSeekModel {
        /// Configured model identity.
        model: ModelId,
    },
    /// The tracing subscriber or OTLP exporter could not be initialized.
    #[error("telemetry initialization failed")]
    Telemetry(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
}
