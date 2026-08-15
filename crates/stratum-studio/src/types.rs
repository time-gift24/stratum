use std::{fmt, str::FromStr};

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use stratum_core::{AgentName, AgentVersionTag, ModelConfig, ModelId, ToolName};
use thiserror::Error;
use uuid::Uuid;

/// Closed set of LLM providers supported by the Studio catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKind {
    /// OpenAI-compatible OpenAI provider.
    Openai,
    /// DeepSeek provider.
    Deepseek,
}

impl ProviderKind {
    /// Returns the protocol provider name used by [`ModelId`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Deepseek => "deepseek",
        }
    }

    /// Returns all catalog-supported providers in stable display order.
    #[must_use]
    pub const fn all() -> [Self; 2] {
        [Self::Openai, Self::Deepseek]
    }

    /// Returns the provider that owns `model`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderKindParseError`] if the model uses a provider outside
    /// the Studio-supported set.
    pub fn from_model_id(model: &ModelId) -> Result<Self, ProviderKindParseError> {
        model.provider_name().parse()
    }
}

impl fmt::Display for ProviderKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(formatter)
    }
}

impl FromStr for ProviderKind {
    type Err = ProviderKindParseError;

    /// Parses a Studio-supported provider name.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderKindParseError`] for unsupported provider names.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openai" => Ok(Self::Openai),
            "deepseek" => Ok(Self::Deepseek),
            _ => Err(ProviderKindParseError),
        }
    }
}

/// Error returned for an unsupported Studio provider name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("provider is not supported by studio")]
pub struct ProviderKindParseError;

/// Opaque strong version of one Studio resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceVersion(Uuid);

impl ResourceVersion {
    pub(crate) const fn new(value: Uuid) -> Self {
        Self(value)
    }

    /// Returns a strong HTTP entity tag for this version.
    #[must_use]
    pub fn etag(self) -> String {
        format!("\"{}\"", self.0)
    }
}

impl FromStr for ResourceVersion {
    type Err = ResourceVersionParseError;

    /// Parses the exact quoted form produced by [`ResourceVersion::etag`].
    ///
    /// # Errors
    ///
    /// Returns [`ResourceVersionParseError`] for malformed or weak tags.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some(value) = value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
        else {
            return Err(ResourceVersionParseError);
        };
        value
            .parse()
            .map(Self)
            .map_err(|_| ResourceVersionParseError)
    }
}

/// Error returned for malformed Studio resource versions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("resource version must be a quoted UUID")]
pub struct ResourceVersionParseError;

/// A Studio value with its HTTP-safe optimistic concurrency version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Versioned<T> {
    /// The persisted value.
    pub value: T,
    /// Current version of that value.
    pub version: ResourceVersion,
}

/// Read-only projection of a managed Provider.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSummary {
    /// Provider identity.
    pub kind: ProviderKind,
    /// Whether a nonblank credential exists. The credential is never exposed.
    pub credential_configured: bool,
    /// Number of models managed under this provider.
    pub models_count: usize,
    /// Most recent Provider or credential update.
    pub updated_at: DateTime<Utc>,
}

/// Read-only projection of a managed model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedModel {
    /// Canonical Provider-scoped model id.
    pub model: ModelId,
    /// Owning provider.
    pub provider: ProviderKind,
    /// Provider-local model name.
    pub name: String,
    /// Most recent change to this model.
    pub updated_at: DateTime<Utc>,
}

/// Mutable authoring definition for future AgentRuntime creation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinition {
    /// Stable Agent definition name.
    pub agent_name: AgentName,
    /// Author-supplied immutable template version tag.
    pub agent_version: AgentVersionTag,
    /// Model selection and provider parameters.
    pub model: ModelConfig,
    /// Named tools exposed to the Agent.
    pub tools: Vec<ToolName>,
    /// System prompt.
    pub prompt: String,
    /// Most recent change to this mutable authoring record.
    pub updated_at: DateTime<Utc>,
}

/// Input used to create or replace an Agent definition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDefinitionInput {
    /// Stable Agent definition name.
    pub agent_name: AgentName,
    /// New immutable template version tag.
    pub agent_version: AgentVersionTag,
    /// Model selection and provider parameters.
    pub model: ModelConfig,
    /// Named tools exposed to the Agent.
    pub tools: Vec<ToolName>,
    /// System prompt.
    pub prompt: String,
}

/// Provider data used once to bootstrap an empty Studio catalog.
pub struct ProviderSeed {
    /// Provider identity.
    pub kind: ProviderKind,
    /// Credential copied into the isolated Studio database.
    pub api_key: SecretString,
    /// Provider-local model names.
    pub models: Vec<String>,
}

/// Immutable source data used only when the Studio catalog is empty.
#[derive(Default)]
pub struct StudioCatalogSeed {
    /// Initial provider records.
    pub providers: Vec<ProviderSeed>,
    /// Initial Agent authoring definitions.
    pub agent_definitions: Vec<AgentDefinitionInput>,
}

/// Secret-bearing provider data only for the trusted runtime assembly boundary.
pub struct RuntimeProvider {
    /// Provider identity.
    pub kind: ProviderKind,
    /// Provider credential. This must not be logged, serialized, or returned by HTTP.
    pub api_key: SecretString,
    /// Provider-local models available for future Turn creation.
    pub models: Vec<String>,
}
