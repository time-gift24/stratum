//! Shared, strictly validated configuration for Stratum applications.

mod error;

use std::{collections::HashSet, fmt, net::SocketAddr, path::PathBuf, str::FromStr};

pub use error::ConfigError;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use stratum_core::{AgentVersionTag, ModelId, ToolName};

/// Top-level Stratum configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct Config {
    /// Agent template catalog configuration.
    pub agent: AgentConfig,
    /// LLM provider configuration.
    pub llm: LlmConfig,
    /// HTTP API configuration, when the API is enabled.
    #[serde(default)]
    pub api: Option<ApiConfig>,
    /// NATS configuration for the short AgentRuntime-scoped realtime tail.
    #[serde(default)]
    pub nats: Option<NatsConfig>,
    /// Postgres execution-storage configuration, required by the API host.
    #[serde(default)]
    pub postgres: Option<PostgresConfig>,
}

/// Agent template catalog configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct AgentConfig {
    /// Read-only root directory of the agent template catalog.
    pub templates_root: PathBuf,
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
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind: default_api_bind(),
            allowed_origins: Vec::new(),
            shutdown_drain_timeout_seconds: default_shutdown_drain_timeout_seconds(),
            sse_keep_alive_seconds: default_sse_keep_alive_seconds(),
            dispatcher_idle_timeout_seconds: default_dispatcher_idle_timeout_seconds(),
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PostgresConfig {
    /// Postgres connection URL.
    pub url: String,
}

/// Stable name used to identify an agent definition.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AgentName(String);

impl AgentName {
    /// Returns the validated name as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for AgentName {
    type Err = ConfigError;

    /// Parses an ASCII agent name matching `[A-Za-z0-9][A-Za-z0-9_-]{0,63}`.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::InvalidAgentName`] if the value is empty, too long, or not
    /// the documented ASCII pattern.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut bytes = value.bytes();
        let valid = value.len() <= 64
            && bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));
        if !valid {
            return Err(ConfigError::InvalidAgentName {
                value: value.to_owned(),
            });
        }
        Ok(Self(value.to_owned()))
    }
}

impl TryFrom<String> for AgentName {
    type Error = ConfigError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

impl From<AgentName> for String {
    fn from(value: AgentName) -> Self {
        value.0
    }
}

/// LLM defaults and supported providers.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct LlmConfig {
    /// Model used when an agent template does not override it.
    pub default: ModelId,
    /// DeepSeek provider configuration.
    #[serde(default)]
    pub deepseek: Option<ProviderConfig>,
    /// OpenAI provider configuration.
    #[serde(default)]
    pub openai: Option<ProviderConfig>,
}

/// Credentials and allowed models for one LLM provider.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ProviderConfig {
    /// Provider API key; held as a secret in memory (§6) and transferred to
    /// the provider wrapper without creating an ordinary plaintext `String`.
    pub api_key: SecretString,
    /// Provider-local model names available to agents.
    pub models: Vec<String>,
    /// Optional base URL override; when absent the provider's well-known
    /// public endpoint is used by the assembly layer.
    #[serde(default)]
    pub base_url: Option<String>,
    /// TCP connect timeout for provider egress.
    #[serde(default = "default_llm_connect_timeout_seconds")]
    pub connect_timeout_seconds: u64,
    /// Total timeout for a non-streaming chat request.
    #[serde(default = "default_llm_request_timeout_seconds")]
    pub request_timeout_seconds: u64,
    /// Maximum wait for streaming response headers.
    #[serde(default = "default_llm_first_response_timeout_seconds")]
    pub first_response_timeout_seconds: u64,
    /// Maximum silence between streaming response body chunks.
    #[serde(default = "default_llm_stream_idle_timeout_seconds")]
    pub stream_idle_timeout_seconds: u64,
}

const fn default_llm_connect_timeout_seconds() -> u64 {
    10
}

const fn default_llm_request_timeout_seconds() -> u64 {
    120
}

const fn default_llm_first_response_timeout_seconds() -> u64 {
    30
}

const fn default_llm_stream_idle_timeout_seconds() -> u64 {
    60
}

// Equality compares the revealed key value in code only; the secret is never
// Debug/Display exposed (§6).
impl PartialEq for ProviderConfig {
    fn eq(&self, other: &Self) -> bool {
        self.api_key.expose_secret() == other.api_key.expose_secret()
            && self.models == other.models
            && self.base_url == other.base_url
            && self.connect_timeout_seconds == other.connect_timeout_seconds
            && self.request_timeout_seconds == other.request_timeout_seconds
            && self.first_response_timeout_seconds == other.first_response_timeout_seconds
            && self.stream_idle_timeout_seconds == other.stream_idle_timeout_seconds
    }
}

impl Eq for ProviderConfig {}

// The API key is a credential: Debug never exposes it (§6).
impl fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("api_key", &"[redacted]")
            .field("models", &self.models)
            .field("base_url", &self.base_url)
            .field("connect_timeout_seconds", &self.connect_timeout_seconds)
            .field("request_timeout_seconds", &self.request_timeout_seconds)
            .field(
                "first_response_timeout_seconds",
                &self.first_response_timeout_seconds,
            )
            .field(
                "stream_idle_timeout_seconds",
                &self.stream_idle_timeout_seconds,
            )
            .finish()
    }
}

/// Validated, self-contained agent definition without provider credentials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct ResolvedAgentDefinition {
    /// Agent name.
    pub agent_name: AgentName,
    /// Author-provided immutable template version tag.
    pub agent_version: AgentVersionTag,
    /// Selected model.
    pub model: ModelId,
    /// Tools exposed to the agent.
    pub tools: Vec<ToolName>,
    /// System prompt.
    pub prompt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentTemplate {
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    model: Option<ModelId>,
    #[serde(default)]
    tools: Vec<ToolName>,
    prompt: String,
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

    /// Resolves a strict TOML agent template against this configuration.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] if the template is invalid or selects an unconfigured model.
    pub fn resolve_template(
        &self,
        agent_name: AgentName,
        input: &str,
    ) -> Result<ResolvedAgentDefinition, ConfigError> {
        let template: AgentTemplate = toml::from_str(input)?;
        let version = template
            .version
            .ok_or(ConfigError::MissingAgentVersion)?
            .parse()
            .map_err(ConfigError::InvalidAgentVersion)?;
        let prompt = template.prompt.trim();
        if prompt.is_empty() {
            return Err(ConfigError::EmptyPrompt);
        }
        validate_tools(&template.tools)?;
        let model = template.model.unwrap_or_else(|| self.llm.default.clone());
        self.validate_model_configured(&model)?;

        Ok(ResolvedAgentDefinition {
            agent_name,
            agent_version: version,
            model,
            tools: template.tools,
            prompt: prompt.to_owned(),
        })
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

    fn validate(&self) -> Result<(), ConfigError> {
        if self.agent.templates_root.as_os_str().is_empty() {
            return Err(ConfigError::InvalidTemplatesRoot);
        }
        validate_provider("deepseek", self.llm.deepseek.as_ref())?;
        validate_provider("openai", self.llm.openai.as_ref())?;
        if let Some(postgres) = &self.postgres
            && postgres.url.trim().is_empty()
        {
            return Err(ConfigError::InvalidPostgresConfig { field: "url" });
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
            ] {
                if value == 0 {
                    return Err(ConfigError::InvalidApiConfig { field });
                }
            }
        }
        self.validate_model_configured(&self.llm.default)
    }

    /// Validates that a model is declared by its configured provider.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::ModelNotConfigured`] when the provider is absent or does not list
    /// the model.
    pub fn validate_model_configured(&self, model: &ModelId) -> Result<(), ConfigError> {
        let provider = match model.provider_name() {
            "deepseek" => self.llm.deepseek.as_ref(),
            "openai" => self.llm.openai.as_ref(),
            _ => None,
        };
        if provider.is_some_and(|config| {
            config
                .models
                .iter()
                .any(|configured| configured == model.model_name())
        }) {
            return Ok(());
        }
        Err(ConfigError::ModelNotConfigured {
            model: model.clone(),
        })
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

fn validate_provider(
    provider: &'static str,
    config: Option<&ProviderConfig>,
) -> Result<(), ConfigError> {
    let Some(config) = config else {
        return Ok(());
    };
    if config.api_key.expose_secret().trim().is_empty() {
        return Err(ConfigError::EmptyApiKey { provider });
    }
    if config.models.is_empty() {
        return Err(ConfigError::EmptyModels { provider });
    }
    if config
        .base_url
        .as_deref()
        .is_some_and(|base_url| base_url.trim().is_empty())
    {
        return Err(ConfigError::InvalidProviderBaseUrl { provider });
    }
    for (field, value) in [
        ("connect_timeout_seconds", config.connect_timeout_seconds),
        ("request_timeout_seconds", config.request_timeout_seconds),
        (
            "first_response_timeout_seconds",
            config.first_response_timeout_seconds,
        ),
        (
            "stream_idle_timeout_seconds",
            config.stream_idle_timeout_seconds,
        ),
    ] {
        if value == 0 {
            return Err(ConfigError::InvalidProviderTimeout { provider, field });
        }
    }
    let mut models = HashSet::with_capacity(config.models.len());
    for model in &config.models {
        if !models.insert(model.as_str()) {
            return Err(ConfigError::DuplicateModel {
                provider,
                model: model.clone(),
            });
        }
    }
    Ok(())
}

fn validate_tools(tools: &[ToolName]) -> Result<(), ConfigError> {
    let mut names = HashSet::with_capacity(tools.len());
    for tool in tools {
        if !names.insert(tool.as_str()) {
            return Err(ConfigError::DuplicateTool {
                tool: tool.as_str().to_owned(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::error::Error as StdError;

    use super::{AgentName, Config, ConfigError};

    const VALID_CONFIG: &str = r#"
[agent]
templates_root = "./templates"

[llm]
default = "deepseek:deepseek-v4-flash"

[llm.deepseek]
api_key = "secret-key"
models = ["deepseek-v4-flash", "deepseek-v4-pro"]

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
"#;

    const VALID_TEMPLATE_WITHOUT_MODEL: &str = r#"
version = "release-1"
tools = ["read_file", "apply_patch"]
prompt = "  You are a coding agent.  "
"#;

    #[test]
    fn parses_complete_config() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");

        assert_eq!(config.agent.templates_root.to_string_lossy(), "./templates");
        assert_eq!(config.llm.default.as_str(), "deepseek:deepseek-v4-flash");
        assert_eq!(config.require_api().expect("api exists").bind.port(), 8080);
        assert_eq!(config.require_nats().expect("nats exists").replicas, 1);
        let api = config.require_api().expect("api exists");
        assert_eq!(api.shutdown_drain_timeout_seconds, 10);
        assert_eq!(api.sse_keep_alive_seconds, 15);
        assert_eq!(api.dispatcher_idle_timeout_seconds, 60);
        let nats = config.require_nats().expect("nats exists");
        assert_eq!(nats.connect_timeout_seconds, 5);
        let provider = config.llm.deepseek.as_ref().expect("provider exists");
        assert_eq!(provider.connect_timeout_seconds, 10);
        assert_eq!(provider.request_timeout_seconds, 120);
        assert_eq!(provider.first_response_timeout_seconds, 30);
        assert_eq!(provider.stream_idle_timeout_seconds, 60);
    }

    #[test]
    fn rejects_zero_operational_timeouts() {
        let cases = [
            (
                VALID_CONFIG.replace(
                    "[api]\n",
                    "[api]\nshutdown_drain_timeout_seconds = 0\n",
                ),
                "api",
            ),
            (
                VALID_CONFIG.replace(
                    "[nats]\n",
                    "[nats]\nconnect_timeout_seconds = 0\n",
                ),
                "nats",
            ),
            (
                VALID_CONFIG.replace(
                    "models = [\"deepseek-v4-flash\", \"deepseek-v4-pro\"]",
                    "models = [\"deepseek-v4-flash\", \"deepseek-v4-pro\"]\nstream_idle_timeout_seconds = 0",
                ),
                "provider",
            ),
        ];

        for (input, kind) in cases {
            let error = Config::parse(&input).expect_err("zero timeout is rejected");
            assert!(
                matches!(
                    (kind, error),
                    ("api", ConfigError::InvalidApiConfig { .. })
                        | ("nats", ConfigError::InvalidNatsConfig { .. })
                        | ("provider", ConfigError::InvalidProviderTimeout { .. })
                ),
                "unexpected timeout error for {kind}"
            );
        }
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
    fn rejects_unknown_config_field() {
        let input = VALID_CONFIG.replace("[agent]", "[agent]\nunknown = true");
        assert!(matches!(Config::parse(&input), Err(ConfigError::Toml(_))));
    }

    #[test]
    fn rejects_removed_storage_section() {
        let input = format!("{VALID_CONFIG}\n[storage]\nbackend = \"postgres\"\n");
        assert!(matches!(Config::parse(&input), Err(ConfigError::Toml(_))));
    }

    #[test]
    fn malformed_toml_error_redacts_input_from_entire_source_chain() {
        let secret = "malformed-secret-key";
        let input = format!("[agent]\ntemplates_root = \"{secret}");
        let error = Config::parse(&input).expect_err("malformed TOML is rejected");

        assert_error_chain_redacts(&error, secret);
    }

    #[test]
    fn unknown_field_error_redacts_input_from_entire_source_chain() {
        let secret = "secret-key";
        let input = VALID_CONFIG.replace("[agent]", "[agent]\nunknown = true");
        let error = Config::parse(&input).expect_err("unknown field is rejected");

        assert_error_chain_redacts(&error, secret);
    }

    #[test]
    fn rejects_duplicate_models() {
        let input = VALID_CONFIG.replace(
            "models = [\"deepseek-v4-flash\", \"deepseek-v4-pro\"]",
            "models = [\"deepseek-v4-flash\", \"deepseek-v4-flash\"]",
        );
        assert!(matches!(
            Config::parse(&input),
            Err(ConfigError::DuplicateModel { .. })
        ));
    }

    #[test]
    fn rejects_default_model_not_in_provider_list() {
        let input = VALID_CONFIG.replace(
            "default = \"deepseek:deepseek-v4-flash\"",
            "default = \"deepseek:not-configured\"",
        );
        assert!(matches!(
            Config::parse(&input),
            Err(ConfigError::ModelNotConfigured { .. })
        ));
    }

    #[test]
    fn rejects_empty_templates_root() {
        let input = VALID_CONFIG.replace("./templates", "");
        assert!(matches!(
            Config::parse(&input),
            Err(ConfigError::InvalidTemplatesRoot)
        ));
    }

    #[test]
    fn rejects_empty_provider_api_key() {
        let input = VALID_CONFIG.replace("api_key = \"secret-key\"", "api_key = \"  \"");
        assert!(matches!(
            Config::parse(&input),
            Err(ConfigError::EmptyApiKey { .. })
        ));
    }

    #[test]
    fn rejects_empty_provider_models() {
        let input = VALID_CONFIG.replace(
            "models = [\"deepseek-v4-flash\", \"deepseek-v4-pro\"]",
            "models = []",
        );
        assert!(matches!(
            Config::parse(&input),
            Err(ConfigError::EmptyModels { .. })
        ));
    }

    #[test]
    fn parses_provider_base_url_override() {
        let input = VALID_CONFIG.replace(
            "api_key = \"secret-key\"",
            "api_key = \"secret-key\"\nbase_url = \"https://llm.internal.example/v1\"",
        );
        let config = Config::parse(&input).expect("config parses");

        assert_eq!(
            config
                .llm
                .deepseek
                .as_ref()
                .expect("provider exists")
                .base_url
                .as_deref(),
            Some("https://llm.internal.example/v1")
        );
    }

    #[test]
    fn rejects_blank_provider_base_url() {
        let input = VALID_CONFIG.replace(
            "api_key = \"secret-key\"",
            "api_key = \"secret-key\"\nbase_url = \"  \"",
        );
        assert!(matches!(
            Config::parse(&input),
            Err(ConfigError::InvalidProviderBaseUrl { .. })
        ));
    }

    #[test]
    fn provider_config_debug_redacts_api_key() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let provider = config.llm.deepseek.as_ref().expect("provider exists");
        let debug = format!("{provider:?}");

        assert!(debug.contains("[redacted]"));
        assert!(!debug.contains("secret-key"));
        assert!(!format!("{config:?}").contains("secret-key"));
    }

    #[test]
    fn parses_valid_agent_name() {
        let name: AgentName = "coding-agent-2".parse().expect("name parses");
        assert_eq!(name.as_str(), "coding-agent-2");
    }

    #[test]
    fn agent_name_accepts_uppercase_underscore_and_flexible_hyphens() {
        for value in ["CodingAgent", "coding_agent", "a--b", "coding-"] {
            let name: AgentName = value.parse().expect("name parses");
            assert_eq!(name.as_str(), value);
        }
    }

    #[test]
    fn rejects_invalid_agent_names() {
        for value in ["", "éagent", "_coding", "-coding"] {
            assert!(matches!(
                value.parse::<AgentName>(),
                Err(ConfigError::InvalidAgentName { .. })
            ));
        }
    }

    #[test]
    fn resolves_author_version_tag_without_normalizing_it() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let definition = config
            .resolve_template(
                "coding-agent".parse().expect("agent name parses"),
                VALID_TEMPLATE_WITHOUT_MODEL,
            )
            .expect("template resolves");

        assert_eq!(definition.agent_version.as_str(), "release-1");
        assert_eq!(definition.prompt, "You are a coding agent.");
        assert_eq!(definition.tools.len(), 2);
    }

    #[test]
    fn rejects_missing_or_invalid_author_version_tag() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let cases = [
            ("tools = []\nprompt = \"hello\"", true),
            ("version = \"\"\ntools = []\nprompt = \"hello\"", false),
            (
                "version = \" release-1\"\ntools = []\nprompt = \"hello\"",
                false,
            ),
            (
                "version = \"release-1 \"\ntools = []\nprompt = \"hello\"",
                false,
            ),
        ];

        for (template, missing) in cases {
            let error = config
                .resolve_template("coding-agent".parse().expect("agent name parses"), template)
                .expect_err("invalid version is rejected");
            assert!(
                if missing {
                    matches!(error, ConfigError::MissingAgentVersion)
                } else {
                    matches!(error, ConfigError::InvalidAgentVersion(_))
                },
                "unexpected error: {error}"
            );
        }

        let oversized = format!(
            "version = {:?}\ntools = []\nprompt = \"hello\"",
            "x".repeat(129)
        );
        assert!(matches!(
            config.resolve_template(
                "coding-agent".parse().expect("agent name parses"),
                &oversized,
            ),
            Err(ConfigError::InvalidAgentVersion(_))
        ));
    }

    #[test]
    fn rejects_agent_name_longer_than_64_bytes() {
        let value = "a".repeat(65);
        assert!(matches!(
            value.parse::<AgentName>(),
            Err(ConfigError::InvalidAgentName { .. })
        ));
    }

    #[test]
    fn template_without_model_uses_system_default() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let name: AgentName = "coding-agent".parse().expect("name parses");
        let definition = config
            .resolve_template(name, VALID_TEMPLATE_WITHOUT_MODEL)
            .expect("template resolves");
        assert_eq!(definition.model.as_str(), "deepseek:deepseek-v4-flash");
        assert_eq!(definition.prompt, "You are a coding agent.");
    }

    #[test]
    fn template_model_overrides_system_default() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let name: AgentName = "coding-agent".parse().expect("name parses");
        let template = r#"
version = "release-2"
model = "deepseek:deepseek-v4-pro"
tools = ["read_file"]
prompt = "Use the requested model."
"#;

        let definition = config
            .resolve_template(name, template)
            .expect("template resolves");
        assert_eq!(definition.model.as_str(), "deepseek:deepseek-v4-pro");
    }

    #[test]
    fn rejects_unconfigured_template_model() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let name: AgentName = "coding-agent".parse().expect("name parses");
        let template = r#"
version = "release-invalid-model"
model = "deepseek:not-configured"
tools = []
prompt = "Use the requested model."
"#;

        assert!(matches!(
            config.resolve_template(name, template),
            Err(ConfigError::ModelNotConfigured { .. })
        ));
    }

    #[test]
    fn rejects_unknown_template_field() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let name: AgentName = "coding-agent".parse().expect("name parses");
        let template = format!("{VALID_TEMPLATE_WITHOUT_MODEL}\nunknown = true");

        assert!(matches!(
            config.resolve_template(name, &template),
            Err(ConfigError::Toml(_))
        ));
    }

    #[test]
    fn rejects_duplicate_template_tools() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let name: AgentName = "coding-agent".parse().expect("name parses");
        let template = r#"
version = "release-duplicate-tools"
tools = ["read_file", "read_file"]
prompt = "Use tools."
"#;

        assert!(matches!(
            config.resolve_template(name, template),
            Err(ConfigError::DuplicateTool { .. })
        ));
    }

    #[test]
    fn rejects_empty_template_prompt() {
        let config = Config::parse(VALID_CONFIG).expect("config parses");
        let name: AgentName = "coding-agent".parse().expect("name parses");
        let template = "version = \"release-empty-prompt\"\ntools = []\nprompt = \"  \"";

        assert!(matches!(
            config.resolve_template(name, template),
            Err(ConfigError::EmptyPrompt)
        ));
    }

    #[test]
    fn missing_optional_sections_are_reported_when_required() {
        let input = VALID_CONFIG
            .split("\n[api]")
            .next()
            .expect("config has api section");
        let config = Config::parse(input).expect("base config parses");

        assert!(matches!(
            config.require_api(),
            Err(ConfigError::MissingSection { section: "api" })
        ));
        assert!(matches!(
            config.require_nats(),
            Err(ConfigError::MissingSection { section: "nats" })
        ));
        assert!(matches!(
            config.require_postgres(),
            Err(ConfigError::MissingSection {
                section: "postgres"
            })
        ));
    }

    #[test]
    fn parses_postgres_config() {
        let input = format!(
            "{VALID_CONFIG}\n[postgres]\nurl = \"postgres://stratum:secret@db:5432/stratum\"\n"
        );
        let config = Config::parse(&input).expect("config parses");

        assert_eq!(
            config.require_postgres().expect("postgres exists").url,
            "postgres://stratum:secret@db:5432/stratum"
        );
    }

    #[test]
    fn rejects_blank_postgres_url() {
        let input = format!("{VALID_CONFIG}\n[postgres]\nurl = \"  \"\n");

        assert!(matches!(
            Config::parse(&input),
            Err(ConfigError::InvalidPostgresConfig { field: "url" })
        ));
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

    #[test]
    fn validation_error_does_not_contain_api_key() {
        let input = VALID_CONFIG.replace(
            "models = [\"deepseek-v4-flash\", \"deepseek-v4-pro\"]",
            "models = [\"deepseek-v4-flash\", \"deepseek-v4-flash\"]",
        );
        let error = Config::parse(&input).expect_err("duplicate model is rejected");

        assert!(!format!("{error:?}").contains("secret-key"));
        assert!(!error.to_string().contains("secret-key"));
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
}
