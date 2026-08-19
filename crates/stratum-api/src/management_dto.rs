//! HTTP DTOs for loopback-only Studio management resources.

use chrono::{DateTime, Utc};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use stratum_core::{AgentVersionTag, ModelId, ToolName};
use utoipa::ToSchema;

/// Closed Provider kind accepted by the management API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ProviderKindDto {
    /// OpenAI.
    Openai,
    /// DeepSeek.
    Deepseek,
}

/// Pagination metadata shared by management list responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct PaginationView {
    /// One-based page number.
    pub page: usize,
    /// Requested page size.
    pub per_page: usize,
    /// Total resources before paging.
    pub total: usize,
}

/// Request for creating one Agent definition.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAgentDefinitionRequest {
    /// Stable Agent name.
    pub agent_name: String,
    /// Author-supplied immutable template version tag.
    pub agent_version: String,
    /// Canonical model identity.
    pub model: ModelId,
    /// Provider parameters.
    #[serde(default)]
    pub model_parameters: serde_json::Map<String, serde_json::Value>,
    /// Tool names.
    #[serde(default)]
    pub tools: Vec<ToolName>,
    /// System prompt.
    pub prompt: String,
}

/// Complete replacement request for one Agent definition.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateAgentDefinitionRequest {
    /// New author-supplied immutable template version tag.
    pub agent_version: String,
    /// Canonical model identity.
    pub model: ModelId,
    /// Provider parameters.
    #[serde(default)]
    pub model_parameters: serde_json::Map<String, serde_json::Value>,
    /// Tool names.
    #[serde(default)]
    pub tools: Vec<ToolName>,
    /// System prompt.
    pub prompt: String,
}

/// Canonical Agent definition projection.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AgentDefinitionView {
    /// Stable Agent name.
    pub agent_name: String,
    /// Author-supplied immutable template version tag.
    pub agent_version: AgentVersionTag,
    /// Canonical model identity.
    pub model: ModelId,
    /// Provider parameters.
    pub model_parameters: serde_json::Map<String, serde_json::Value>,
    /// Tool names.
    pub tools: Vec<ToolName>,
    /// System prompt.
    pub prompt: String,
    /// Persisted Studio record modification time.
    pub updated_at: DateTime<Utc>,
}

/// One Agent definition page.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AgentDefinitionsPage {
    /// Page data.
    pub data: Vec<AgentDefinitionView>,
    /// Page metadata.
    pub pagination: PaginationView,
}

/// Request for configuring one supported Provider.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateProviderRequest {
    /// Supported Provider kind.
    pub provider: ProviderKindDto,
    /// Credential written once and never echoed.
    #[schema(value_type = String, write_only)]
    pub api_key: SecretString,
}

/// Request for replacing a Provider credential; omission preserves it.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct UpdateProviderRequest {
    /// New credential, when replacement is intended.
    #[serde(default)]
    #[schema(value_type = Option<String>, write_only)]
    pub api_key: Option<SecretString>,
}

/// Sanitized Provider projection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProviderView {
    /// Provider kind.
    pub provider: ProviderKindDto,
    /// Whether a non-empty credential is configured.
    pub credential_configured: bool,
    /// Number of models under this Provider.
    pub models_count: usize,
    /// Persisted Studio record modification time.
    pub updated_at: DateTime<Utc>,
}

/// One Provider page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProvidersPage {
    /// Page data.
    pub data: Vec<ProviderView>,
    /// Page metadata.
    pub pagination: PaginationView,
}

/// Request for creating a Provider-local model.
#[derive(Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateModelRequest {
    /// Provider-local model name.
    pub name: String,
}

/// Canonical Model projection.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModelView {
    /// Canonical Provider-scoped identity.
    pub model_id: ModelId,
    /// Owning Provider.
    pub provider: ProviderKindDto,
    /// Provider-local name.
    pub name: String,
    /// Read-only adapter parameter schema.
    pub parameter_schema: serde_json::Value,
    /// Persisted Studio record modification time.
    pub updated_at: DateTime<Utc>,
}

/// One Model page.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModelsPage {
    /// Page data.
    pub data: Vec<ModelView>,
    /// Page metadata.
    pub pagination: PaginationView,
}

/// One tool actually available from this host binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ToolView {
    /// Provider-visible tool name.
    pub name: String,
    /// Safe provider-visible description.
    pub description: String,
    /// Whether the tool reads or writes external state.
    pub kind: ToolKindDto,
    /// Approval risk classification.
    pub danger_level: DangerLevelDto,
}

/// Closed wire projection of [`stratum_core::ToolKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum ToolKindDto {
    Read,
    Write,
}

/// Closed wire projection of [`stratum_core::DangerLevel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum DangerLevelDto {
    Low,
    Medium,
    High,
}

/// Result of one transient Provider credential probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, ToSchema)]
pub struct ProviderTestResult {
    /// Always true for this success response; failures use the error envelope.
    pub success: bool,
    /// Server completion time of this one probe.
    pub completed_at: DateTime<Utc>,
}
