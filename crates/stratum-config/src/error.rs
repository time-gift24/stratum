//! Configuration errors.

use thiserror::Error;

/// Error returned while parsing or validating Stratum configuration.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// TOML input could not be decoded.
    #[error("invalid TOML configuration")]
    Toml(#[source] toml::de::Error),
    /// A configured CORS origin was a wildcard or invalid HTTP header value.
    #[error("api allowed origin is invalid")]
    InvalidAllowedOrigin,
    /// An API operational timeout was zero.
    #[error("invalid api configuration field `{field}`")]
    InvalidApiConfig { field: &'static str },
    /// A caller required an optional section that was not configured.
    #[error("missing required configuration section `{section}`")]
    MissingSection { section: &'static str },
    /// A NATS field was not valid for the short AgentRuntime-scoped tail.
    #[error("invalid nats configuration field `{field}`")]
    InvalidNatsConfig { field: &'static str },
    /// A Postgres field was not valid.
    #[error("invalid postgres configuration field `{field}`")]
    InvalidPostgresConfig { field: &'static str },
    /// An Ontology persistence setting was invalid.
    #[error("invalid ontology configuration field `{field}`")]
    InvalidOntologyConfig { field: &'static str },
    /// A Studio catalog setting was invalid.
    #[error("invalid studio configuration field `{field}`")]
    InvalidStudioConfig { field: &'static str },
    /// A tool execution setting was invalid.
    #[error("invalid tool configuration field `{field}`")]
    InvalidToolConfig { field: &'static str },
    /// Two bounded contexts were configured to use the same PostgreSQL database.
    #[error("execution, ontology, and studio must use distinct PostgreSQL databases")]
    DatabaseIdentityConflict,
}

impl From<toml::de::Error> for ConfigError {
    fn from(mut source: toml::de::Error) -> Self {
        source.set_input(None);
        Self::Toml(source)
    }
}
