//! Shared, strictly validated configuration for Stratum applications.

mod error;

use std::{fmt, net::SocketAddr};

pub use error::ConfigError;
use serde::Deserialize;
use url::Url;

/// Top-level Stratum configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Config {
    /// HTTP API configuration, when the API is enabled.
    #[serde(default)]
    pub api: Option<ApiConfig>,
    /// NATS configuration for the short AgentRuntime-scoped realtime tail.
    #[serde(default)]
    pub nats: Option<NatsConfig>,
    /// Postgres execution-storage configuration, required by the API host.
    #[serde(default)]
    pub postgres: Option<PostgresConfig>,
    /// Ontology PostgreSQL configuration, required by the API host.
    #[serde(default)]
    pub ontology: Option<OntologyConfig>,
    /// Studio catalog configuration, required by the API host.
    #[serde(default)]
    pub studio: Option<StudioConfig>,
}

/// HTTP API configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ApiConfig {
    /// Socket address on which the API listens.
    #[serde(default = "default_api_bind")]
    pub bind: SocketAddr,
    /// Browser origins allowed to call the API.
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    /// Maximum graceful drain time for managed and background tasks.
    #[serde(default = "default_shutdown_drain_timeout_seconds")]
    pub shutdown_drain_timeout_seconds: u64,
    /// SSE keep-alive interval.
    #[serde(default = "default_sse_keep_alive_seconds")]
    pub sse_keep_alive_seconds: u64,
    /// Idle interval after which an AgentRuntime dispatcher with no external
    /// handles releases its map entry and task.
    #[serde(default = "default_dispatcher_idle_timeout_seconds")]
    pub dispatcher_idle_timeout_seconds: u64,
    /// Maximum time allowed for the complete readiness dependency probe.
    #[serde(default = "default_readiness_timeout_ms")]
    pub readiness_timeout_ms: u64,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind: default_api_bind(),
            allowed_origins: Vec::new(),
            shutdown_drain_timeout_seconds: default_shutdown_drain_timeout_seconds(),
            sse_keep_alive_seconds: default_sse_keep_alive_seconds(),
            dispatcher_idle_timeout_seconds: default_dispatcher_idle_timeout_seconds(),
            readiness_timeout_ms: default_readiness_timeout_ms(),
        }
    }
}

fn default_api_bind() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 8080))
}

const fn default_shutdown_drain_timeout_seconds() -> u64 {
    10
}

const fn default_sse_keep_alive_seconds() -> u64 {
    15
}

const fn default_dispatcher_idle_timeout_seconds() -> u64 {
    60
}

const fn default_readiness_timeout_ms() -> u64 {
    1000
}

/// NATS short-tail configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct NatsConfig {
    /// NATS server URL.
    pub url: String,
    /// JetStream stream name.
    pub stream_name: String,
    /// Subject prefix for AgentRuntime events.
    pub subject_prefix: String,
    /// Number of stream replicas.
    pub replicas: usize,
    /// Maximum retained event age in seconds.
    pub max_age_seconds: u64,
    /// Maximum retained stream size in bytes.
    pub max_bytes: i64,
    /// Maximum retained event count.
    pub max_messages: i64,
    /// Maximum time allowed for the initial NATS connection.
    #[serde(default = "default_nats_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
}

const fn default_nats_connect_timeout_seconds() -> u64 {
    5
}

/// Postgres execution-storage configuration.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PostgresConfig {
    /// Postgres connection URL.
    pub url: String,
}

/// PostgreSQL settings for canonical Ontology metadata.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct OntologyConfig {
    /// PostgreSQL connection URL. This value is never emitted in logs or errors.
    pub database_url: String,
}

/// PostgreSQL settings for the mutable Studio catalog.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct StudioConfig {
    /// Enables loopback-only management routes.
    #[serde(default)]
    pub management_enabled: bool,
    /// Dedicated Studio database URL used by every API runtime.
    pub database_url: String,
}

impl fmt::Debug for OntologyConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("OntologyConfig")
            .field("database_url", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for PostgresConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PostgresConfig")
            .field("url", &"[REDACTED]")
            .finish()
    }
}

impl fmt::Debug for StudioConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StudioConfig")
            .field("management_enabled", &self.management_enabled)
            .field("database_url", &"[REDACTED]")
            .finish()
    }
}

impl Config {
    /// Parses and validates strict TOML configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if TOML decoding or configuration validation fails.
    pub fn parse(input: &str) -> Result<Self, ConfigError> {
        let config: Self = toml::from_str(input)?;
        config.validate()?;
        Ok(config)
    }

    /// Returns the configured API section.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MissingSection`] when no API section was provided.
    pub fn require_api(&self) -> Result<&ApiConfig, ConfigError> {
        self.api
            .as_ref()
            .ok_or(ConfigError::MissingSection { section: "api" })
    }

    /// Returns the configured NATS section.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MissingSection`] when no NATS section was provided.
    pub fn require_nats(&self) -> Result<&NatsConfig, ConfigError> {
        self.nats
            .as_ref()
            .ok_or(ConfigError::MissingSection { section: "nats" })
    }

    /// Returns the configured Postgres section.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MissingSection`] when no Postgres section was provided.
    pub fn require_postgres(&self) -> Result<&PostgresConfig, ConfigError> {
        self.postgres.as_ref().ok_or(ConfigError::MissingSection {
            section: "postgres",
        })
    }

    /// Returns the configured Ontology persistence section.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MissingSection`] when no Ontology configuration was provided.
    pub fn require_ontology(&self) -> Result<&OntologyConfig, ConfigError> {
        self.ontology.as_ref().ok_or(ConfigError::MissingSection {
            section: "ontology",
        })
    }

    /// Returns the Studio catalog configuration used by the API runtime.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::MissingSection`] when Studio configuration was
    /// not supplied. Management route exposure does not affect this runtime
    /// dependency.
    pub fn require_studio(&self) -> Result<&StudioConfig, ConfigError> {
        self.studio
            .as_ref()
            .ok_or(ConfigError::MissingSection { section: "studio" })
    }

    /// Revalidates a decoded or programmatically mutated configuration.
    ///
    /// Host assembly calls this at the security boundary so callers cannot
    /// bypass loopback management and database-isolation invariants by using
    /// `Deserialize` directly or mutating public fields.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when any configured value violates a runtime
    /// invariant.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let execution_database = self
            .postgres
            .as_ref()
            .map(|postgres| validate_postgres_database_url(&postgres.url))
            .transpose()?;
        let ontology_database = self
            .ontology
            .as_ref()
            .map(|ontology| validate_ontology_database_url(&ontology.database_url))
            .transpose()?;
        let studio_database = self
            .studio
            .as_ref()
            .map(|studio| validate_studio_database_url(&studio.database_url))
            .transpose()?;
        ensure_distinct_databases(
            execution_database.as_ref(),
            ontology_database.as_ref(),
            studio_database.as_ref(),
        )?;
        if let Some(studio) = &self.studio
            && studio.management_enabled
        {
            let api = self
                .api
                .as_ref()
                .ok_or(ConfigError::MissingSection { section: "api" })?;
            if !api.bind.ip().is_loopback() {
                return Err(ConfigError::InvalidStudioConfig {
                    field: "management_enabled",
                });
            }
        }
        if let Some(nats) = &self.nats {
            validate_nats(nats)?;
        }
        if let Some(api) = &self.api {
            for origin in &api.allowed_origins {
                if origin == "*" || http::HeaderValue::from_str(origin).is_err() {
                    return Err(ConfigError::InvalidAllowedOrigin);
                }
            }
            for (field, value) in [
                (
                    "shutdown_drain_timeout_seconds",
                    api.shutdown_drain_timeout_seconds,
                ),
                ("sse_keep_alive_seconds", api.sse_keep_alive_seconds),
                (
                    "dispatcher_idle_timeout_seconds",
                    api.dispatcher_idle_timeout_seconds,
                ),
                ("readiness_timeout_ms", api.readiness_timeout_ms),
            ] {
                if value == 0 {
                    return Err(ConfigError::InvalidApiConfig { field });
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DatabaseIdentity {
    host: String,
    port: u16,
    path: String,
}

fn validate_postgres_database_url(value: &str) -> Result<DatabaseIdentity, ConfigError> {
    database_identity(value, true).ok_or(ConfigError::InvalidPostgresConfig { field: "url" })
}

fn validate_ontology_database_url(value: &str) -> Result<DatabaseIdentity, ConfigError> {
    database_identity(value, false).ok_or(ConfigError::InvalidOntologyConfig {
        field: "database_url",
    })
}

fn validate_studio_database_url(value: &str) -> Result<DatabaseIdentity, ConfigError> {
    database_identity(value, false).ok_or(ConfigError::InvalidStudioConfig {
        field: "database_url",
    })
}

fn database_identity(value: &str, query_allowed: bool) -> Option<DatabaseIdentity> {
    if value != value.trim() {
        return None;
    }
    // SQLx parses PostgreSQL URLs through `url::Url`; use the same parser so
    // authority canonicalization and path dot-segment normalization cannot
    // make two apparently different configuration strings reach one database.
    let url = Url::parse(value).ok()?;
    let port = url.port()?;
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || url.host_str().is_none_or(str::is_empty)
        || !url.path().starts_with('/')
        || url.fragment().is_some()
        || (!query_allowed && url.query().is_some())
        || (query_allowed && !execution_query_is_safe(&url))
    {
        return None;
    }
    // SQLx rejects invalid UTF-8 percent encodings in these components.
    percent_decode_utf8(url.username())?;
    if let Some(password) = url.password() {
        percent_decode_utf8(password)?;
    }
    let host = percent_decode_utf8(url.host_str()?)?;
    // Match SQLx's PostgreSQL URL parser: every leading slash is syntax, not
    // part of the database name.
    let path = url.path().trim_start_matches('/');
    if path.is_empty() {
        return None;
    }
    Some(DatabaseIdentity {
        host: host.to_ascii_lowercase(),
        port,
        path: percent_decode_utf8(path)?,
    })
}

fn execution_query_is_safe(url: &Url) -> bool {
    url.query() != Some("")
        && url.query_pairs().all(|(key, _value)| {
            matches!(
                key.as_ref(),
                "sslmode"
                    | "ssl-mode"
                    | "sslrootcert"
                    | "ssl-root-cert"
                    | "ssl-ca"
                    | "sslcert"
                    | "ssl-cert"
                    | "sslkey"
                    | "ssl-key"
                    | "statement-cache-capacity"
                    | "application_name"
                    | "options"
            ) || (key.starts_with("options[")
                && key.ends_with(']')
                && key.len() > "options[]".len())
        })
}

fn ensure_distinct_databases(
    execution: Option<&DatabaseIdentity>,
    ontology: Option<&DatabaseIdentity>,
    studio: Option<&DatabaseIdentity>,
) -> Result<(), ConfigError> {
    if same_database(execution, ontology)
        || same_database(execution, studio)
        || same_database(ontology, studio)
    {
        Err(ConfigError::DatabaseIdentityConflict)
    } else {
        Ok(())
    }
}

fn same_database(left: Option<&DatabaseIdentity>, right: Option<&DatabaseIdentity>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left == right)
}

fn percent_decode_utf8(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let digits = bytes.get(index + 1..index + 3)?;
            let high = hex_value(digits[0])?;
            let low = hex_value(digits[1])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

const fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn validate_nats(config: &NatsConfig) -> Result<(), ConfigError> {
    for (field, candidate) in [
        ("url", config.url.as_str()),
        ("stream_name", config.stream_name.as_str()),
        ("subject_prefix", config.subject_prefix.as_str()),
    ] {
        if candidate.trim().is_empty() {
            return Err(ConfigError::InvalidNatsConfig { field });
        }
    }
    if !(1..=5).contains(&config.replicas) {
        return Err(ConfigError::InvalidNatsConfig { field: "replicas" });
    }
    if config.max_age_seconds == 0 {
        return Err(ConfigError::InvalidNatsConfig {
            field: "max_age_seconds",
        });
    }
    if config.max_bytes <= 0 {
        return Err(ConfigError::InvalidNatsConfig { field: "max_bytes" });
    }
    if config.max_messages <= 0 {
        return Err(ConfigError::InvalidNatsConfig {
            field: "max_messages",
        });
    }
    if config.connect_timeout_seconds == 0 {
        return Err(ConfigError::InvalidNatsConfig {
            field: "connect_timeout_seconds",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use super::{Config, ConfigError};

    const EXECUTION_DATABASE_URL: &str =
        "postgres://stratum:execution-secret@localhost:5432/stratum";
    const ONTOLOGY_DATABASE_URL: &str =
        "postgres://ontology:ontology-secret@localhost:5432/stratum_ontology";
    const STUDIO_DATABASE_URL: &str =
        "postgresql://studio:studio-secret@localhost:5432/stratum_studio";
    const STUDIO_SECTION: &str = r#"
[studio]
management_enabled = false
database_url = "postgresql://studio:studio-secret@localhost:5432/stratum_studio"
"#;
    const VALID_CONFIG: &str = r#"
[api]
bind = "127.0.0.1:8080"
allowed_origins = ["http://localhost:5173"]

[nats]
url = "nats://127.0.0.1:4222"
stream_name = "AGENT_EVENTS"
subject_prefix = "events.agent"
replicas = 1
max_age_seconds = 3600
max_bytes = 268435456
max_messages = 100000

[postgres]
url = "postgres://stratum:execution-secret@localhost:5432/stratum"

[ontology]
database_url = "postgres://ontology:ontology-secret@localhost:5432/stratum_ontology"

[studio]
management_enabled = false
database_url = "postgresql://studio:studio-secret@localhost:5432/stratum_studio"
"#;

    #[test]
    fn parses_complete_db_only_config() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");

        let api = config.require_api().expect("api exists");
        assert_eq!(api.bind.port(), 8080);
        assert_eq!(api.shutdown_drain_timeout_seconds, 10);
        assert_eq!(api.sse_keep_alive_seconds, 15);
        assert_eq!(api.dispatcher_idle_timeout_seconds, 60);
        assert_eq!(api.readiness_timeout_ms, 1000);
        let nats = config.require_nats().expect("nats exists");
        assert_eq!(nats.replicas, 1);
        assert_eq!(nats.connect_timeout_seconds, 5);
        assert_eq!(
            config
                .require_postgres()
                .expect("execution postgres exists")
                .url,
            EXECUTION_DATABASE_URL
        );
        assert_eq!(
            config
                .require_ontology()
                .expect("ontology postgres exists")
                .database_url,
            ONTOLOGY_DATABASE_URL
        );
        assert_eq!(
            config
                .require_studio()
                .expect("studio postgres exists")
                .database_url,
            STUDIO_DATABASE_URL
        );
    }

    #[test]
    fn rejects_legacy_llm_and_agent_sections() {
        for legacy in [
            "\n[llm]\ndefault = \"deepseek:deepseek-chat\"\n",
            "\n[agent]\ntemplates_root = \"./templates\"\n",
        ] {
            let input = format!("{VALID_CONFIG}{legacy}");
            assert!(matches!(Config::parse(&input), Err(ConfigError::Toml(_))));
        }
    }

    #[test]
    fn studio_database_url_is_required_inside_section() {
        let input = VALID_CONFIG.replace(
            "database_url = \"postgresql://studio:studio-secret@localhost:5432/stratum_studio\"\n",
            "",
        );

        assert!(matches!(Config::parse(&input), Err(ConfigError::Toml(_))));
    }

    #[test]
    fn missing_studio_section_fails_when_api_runtime_requires_it() {
        let input = VALID_CONFIG.replace(STUDIO_SECTION, "");
        let config = Config::parse(&input).expect("generic config parses");

        assert!(matches!(
            config.require_studio(),
            Err(ConfigError::MissingSection { section: "studio" })
        ));
    }

    #[test]
    fn management_disabled_still_requires_and_returns_studio() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let studio = config.require_studio().expect("studio remains required");

        assert!(!studio.management_enabled);
        assert_eq!(studio.database_url, STUDIO_DATABASE_URL);
    }

    #[test]
    fn management_disabled_allows_non_loopback_api_bind() {
        let input = VALID_CONFIG.replace("127.0.0.1:8080", "0.0.0.0:8080");
        let config = Config::parse(&input).expect("runtime-only API may bind publicly");

        assert!(
            !config
                .require_studio()
                .expect("studio exists")
                .management_enabled
        );
    }

    #[test]
    fn management_enabled_requires_loopback_api_bind() {
        let enabled =
            VALID_CONFIG.replace("management_enabled = false", "management_enabled = true");
        Config::parse(&enabled).expect("loopback management parses");

        let public = enabled.replace("127.0.0.1:8080", "0.0.0.0:8080");
        assert!(matches!(
            Config::parse(&public),
            Err(ConfigError::InvalidStudioConfig {
                field: "management_enabled"
            })
        ));

        let without_api = r#"
[studio]
management_enabled = true
database_url = "postgres://studio:secret@localhost:5432/studio"
"#;
        assert!(matches!(
            Config::parse(without_api),
            Err(ConfigError::MissingSection { section: "api" })
        ));
    }

    #[test]
    fn revalidation_rejects_management_enabled_after_programmatic_mutation() {
        let mut config = Config::parse(&VALID_CONFIG.replace("127.0.0.1:8080", "0.0.0.0:8080"))
            .expect("public bind is valid while management routes are disabled");
        config
            .studio
            .as_mut()
            .expect("Studio is configured")
            .management_enabled = true;

        assert!(matches!(
            config.validate(),
            Err(ConfigError::InvalidStudioConfig {
                field: "management_enabled"
            })
        ));
    }

    #[test]
    fn rejects_invalid_studio_database_urls() {
        for database_url in [
            "   ",
            "http://studio:password@localhost/studio",
            "postgres:///studio",
            "postgres://localhost/",
            "postgres://localhost/studio",
            "postgres://localhost/%zz",
            "postgres://localhost/%FF",
            "postgres://localhost:not-a-port/studio",
            "postgres://localhost:70000/studio",
            "postgres://localhost/studio?token=supersecret",
            "not a URL",
        ] {
            let input = VALID_CONFIG.replace(STUDIO_DATABASE_URL, database_url);
            assert!(matches!(
                Config::parse(&input),
                Err(ConfigError::InvalidStudioConfig {
                    field: "database_url"
                })
            ));
        }
    }

    #[test]
    fn studio_database_url_is_redacted_from_debug() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let debug = format!("{config:?}");

        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains(STUDIO_DATABASE_URL));
        assert!(!debug.contains("studio-secret"));
    }

    #[test]
    fn rejects_studio_and_execution_using_the_same_database() {
        let input = VALID_CONFIG.replace(
            STUDIO_DATABASE_URL,
            "postgresql://other:collision-secret@LOCALHOST:5432/str%61tum",
        );

        assert_database_identity_conflict(&input, &["collision-secret", "stratum"]);
    }

    #[test]
    fn accepts_the_same_database_path_on_distinct_authorities() {
        let input = VALID_CONFIG.replace(
            STUDIO_DATABASE_URL,
            "postgresql://other:secret@studio.example:6543/stratum",
        );

        Config::parse(&input).expect("different database authorities are isolated");
    }

    #[test]
    fn rejects_execution_query_parameters_that_override_database_identity() {
        for execution_url in [
            "postgres://user:secret@localhost/execution?dbname=stratum_studio",
            "postgres://user:secret@localhost/execution?host=remote.example",
            "postgres://user:secret@localhost/execution?hostaddr=127.0.0.2",
            "postgres://user:secret@localhost/execution?port=6543",
            "postgres://user:secret@localhost/execution?%64bname=stratum_studio",
        ] {
            let input = VALID_CONFIG.replace(EXECUTION_DATABASE_URL, execution_url);
            assert!(matches!(
                Config::parse(&input),
                Err(ConfigError::InvalidPostgresConfig { field: "url" })
            ));
        }
    }

    #[test]
    fn sqlx_equivalent_leading_slashes_cannot_bypass_database_isolation() {
        let input = VALID_CONFIG.replace(
            EXECUTION_DATABASE_URL,
            "postgres://user:secret@localhost:5432//stratum_studio",
        );

        assert_database_identity_conflict(&input, &["stratum_studio"]);
    }

    #[test]
    fn sqlx_normalized_dot_segments_cannot_bypass_database_isolation() {
        for execution_url in [
            "postgres://user:secret@localhost:5432/a/../stratum_studio",
            "postgres://user:secret@localhost:5432/a/%2e%2e/stratum_studio",
        ] {
            let input = VALID_CONFIG.replace(EXECUTION_DATABASE_URL, execution_url);
            assert_database_identity_conflict(&input, &["stratum_studio"]);
        }
    }

    #[test]
    fn rejects_studio_and_ontology_using_the_same_database() {
        let input = VALID_CONFIG.replace(STUDIO_DATABASE_URL, ONTOLOGY_DATABASE_URL);

        assert_database_identity_conflict(&input, &["ontology-secret", "stratum_ontology"]);
    }

    #[test]
    fn rejects_execution_and_ontology_using_the_same_database() {
        let input = VALID_CONFIG.replace(EXECUTION_DATABASE_URL, ONTOLOGY_DATABASE_URL);

        assert_database_identity_conflict(&input, &["ontology-secret", "stratum_ontology"]);
    }

    #[test]
    fn accepts_distinct_execution_ontology_and_studio_databases() {
        Config::parse(VALID_CONFIG).expect("three distinct database paths parse");

        Config::parse(
            "[postgres]\nurl = \"postgres://user:secret@localhost:5432/shared?sslmode=require\"\n",
        )
        .expect("a standalone execution section parses");
        Config::parse(
            "[studio]\ndatabase_url = \"postgres://user:secret@localhost:5432/shared\"\n",
        )
        .expect("a standalone Studio section parses");
    }

    #[test]
    fn api_bind_defaults_to_loopback_when_omitted() {
        let input = VALID_CONFIG.replace("bind = \"127.0.0.1:8080\"\n", "");
        let config = Config::parse(&input).expect("config parses");

        assert_eq!(
            config.require_api().expect("api exists").bind,
            "127.0.0.1:8080".parse().expect("default bind parses")
        );
    }

    #[test]
    fn rejects_zero_operational_timeouts() {
        let cases = [
            (
                VALID_CONFIG.replace("[api]\n", "[api]\nshutdown_drain_timeout_seconds = 0\n"),
                "api",
            ),
            (
                VALID_CONFIG.replace("[nats]\n", "[nats]\nconnect_timeout_seconds = 0\n"),
                "nats",
            ),
        ];

        for (input, kind) in cases {
            let error = Config::parse(&input).expect_err("zero timeout is rejected");
            assert!(
                matches!(
                    (kind, error),
                    ("api", ConfigError::InvalidApiConfig { .. })
                        | ("nats", ConfigError::InvalidNatsConfig { .. })
                ),
                "unexpected timeout error for {kind}"
            );
        }
    }

    #[test]
    fn rejects_wildcard_and_invalid_allowed_origins() {
        for origin in ["*", "bad\norigin"] {
            let input = VALID_CONFIG.replace(
                "allowed_origins = [\"http://localhost:5173\"]",
                &format!("allowed_origins = [{origin:?}]"),
            );

            assert!(matches!(
                Config::parse(&input),
                Err(ConfigError::InvalidAllowedOrigin)
            ));
        }
    }

    #[test]
    fn rejects_unknown_config_and_removed_storage_sections() {
        for section in [
            "\n[unknown]\nenabled = true\n",
            "\n[storage]\nbackend = \"postgres\"\n",
        ] {
            let input = format!("{VALID_CONFIG}{section}");
            assert!(matches!(Config::parse(&input), Err(ConfigError::Toml(_))));
        }
    }

    #[test]
    fn malformed_and_unknown_toml_errors_redact_input_source_chain() {
        let secret = "malformed-secret-key";
        let malformed = format!("[studio]\ndatabase_url = \"{secret}");
        let error = Config::parse(&malformed).expect_err("malformed TOML is rejected");
        assert_error_chain_redacts(&error, secret);

        let unknown = format!("[unknown]\ncredential = \"{secret}\"\n");
        let error = Config::parse(&unknown).expect_err("unknown field is rejected");
        assert_error_chain_redacts(&error, secret);
    }

    #[test]
    fn missing_optional_sections_are_reported_when_required() {
        let config = Config::parse("").expect("empty generic config parses");

        for (result, section) in [
            (config.require_api().map(|_| ()), "api"),
            (config.require_nats().map(|_| ()), "nats"),
            (config.require_postgres().map(|_| ()), "postgres"),
            (config.require_ontology().map(|_| ()), "ontology"),
            (config.require_studio().map(|_| ()), "studio"),
        ] {
            assert!(matches!(
                result,
                Err(ConfigError::MissingSection { section: actual }) if actual == section
            ));
        }
    }

    #[test]
    fn parses_and_redacts_valid_ontology_database_url() {
        let database_url = "postgresql://ontology:secret-password@localhost:5432/ontology";
        let input = format!("[ontology]\ndatabase_url = \"{database_url}\"\n");
        let config = Config::parse(&input).expect("configured ontology parses");

        assert_eq!(
            config
                .require_ontology()
                .expect("ontology exists")
                .database_url,
            database_url
        );
        let debug = format!("{config:?}");
        assert!(!debug.contains(database_url));
        assert!(!debug.contains("secret-password"));
        assert!(debug.contains("[REDACTED]"));
    }

    #[test]
    fn rejects_invalid_ontology_database_urls() {
        for database_url in [
            "   ",
            "http://ontology:password@localhost/ontology",
            "postgres:///ontology",
            "postgres://localhost/",
            "postgres://localhost/ontology",
            "postgres://localhost/%zz",
            "postgres://localhost/%FF",
            "postgres://localhost:not-a-port/ontology",
            "postgres://localhost:70000/ontology",
            "postgres://localhost/ontology?token=supersecret",
            "not a URL",
        ] {
            let input = format!("[ontology]\ndatabase_url = {database_url:?}\n");
            assert!(matches!(
                Config::parse(&input),
                Err(ConfigError::InvalidOntologyConfig {
                    field: "database_url"
                })
            ));
        }
    }

    #[test]
    fn parses_and_validates_postgres_config() {
        let config =
            Config::parse("[postgres]\nurl = \"postgres://stratum:secret@db:5432/stratum\"\n")
                .expect("config parses");
        assert_eq!(
            config.require_postgres().expect("postgres exists").url,
            "postgres://stratum:secret@db:5432/stratum"
        );

        assert!(matches!(
            Config::parse("[postgres]\nurl = \"  \"\n"),
            Err(ConfigError::InvalidPostgresConfig { field: "url" })
        ));
        assert!(matches!(
            Config::parse("[postgres]\nurl = \"http://localhost/stratum\"\n"),
            Err(ConfigError::InvalidPostgresConfig { field: "url" })
        ));
        assert!(matches!(
            Config::parse("[postgres]\nurl = \"postgres://stratum:secret@db/stratum\"\n"),
            Err(ConfigError::InvalidPostgresConfig { field: "url" })
        ));

        let debug = format!("{config:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains("stratum:secret"));
    }

    #[test]
    fn rejects_invalid_nats_config() {
        for (from, to, field) in [
            ("url = \"nats://127.0.0.1:4222\"", "url = \"  \"", "url"),
            ("replicas = 1", "replicas = 0", "replicas"),
            ("replicas = 1", "replicas = 6", "replicas"),
            (
                "max_age_seconds = 3600",
                "max_age_seconds = 0",
                "max_age_seconds",
            ),
            ("max_bytes = 268435456", "max_bytes = 0", "max_bytes"),
            ("max_messages = 100000", "max_messages = -1", "max_messages"),
        ] {
            let input = VALID_CONFIG.replace(from, to);

            assert!(
                matches!(
                    Config::parse(&input),
                    Err(ConfigError::InvalidNatsConfig { field: actual }) if actual == field
                ),
                "case {from} -> {to}"
            );
        }
    }

    fn assert_error_chain_redacts(error: &ConfigError, secret: &str) {
        assert!(!format!("{error:?}").contains(secret));
        assert!(!error.to_string().contains(secret));

        let mut source = StdError::source(error);
        while let Some(error) = source {
            assert!(!format!("{error:?}").contains(secret));
            assert!(!error.to_string().contains(secret));
            source = error.source();
        }
    }

    fn assert_database_identity_conflict(input: &str, sensitive_values: &[&str]) {
        let error = Config::parse(input).expect_err("duplicate database path is rejected");
        assert!(matches!(&error, ConfigError::DatabaseIdentityConflict));
        for sensitive in sensitive_values {
            assert_error_chain_redacts(&error, sensitive);
        }
    }
}
