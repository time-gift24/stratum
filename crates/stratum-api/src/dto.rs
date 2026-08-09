//! HTTP request/response DTOs.
//!
//! Request DTOs deny unknown fields; response DTOs are API-owned shapes that
//! never expose database rows, raw durable payloads, or runtime snapshots.
//! All event sequences on the wire are unsigned decimal strings.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stratum_core::{
    AgentId, ApprovalDecision, ApprovalId, CallId, DangerLevel, ModelConfig, SessionId, TokenUsage,
    ToolKind, ToolName, TurnId,
};
use stratum_llm::ModelDescriptor;
use utoipa::ToSchema;

use crate::frames::AgentProductEventV1;

/// Create-agent request body.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CreateAgentRequest {
    /// Template name of the Agent to create.
    pub agent_name: String,
    /// Full replacement model configuration; the template default is used
    /// when omitted.
    #[serde(default)]
    pub model_config: Option<ModelConfig>,
}

/// Create-agent response body (identical on idempotent replay).
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct CreateAgentResponse {
    /// Created (or replayed) Agent identity.
    pub agent_id: AgentId,
    /// Source template name recorded at creation.
    pub agent_name: String,
    /// Current default model configuration.
    pub model_config: ModelConfig,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
}

/// Message command body.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct MessageRequest {
    /// Raw user text; only trimmed to decide emptiness, persisted verbatim.
    pub text: String,
    /// CAS expectation: explicit `null` for the first Turn, otherwise the
    /// exact most recent Turn. The field itself is required.
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub expected_current_turn_id: Option<TurnId>,
    /// Session to bind on the first Turn; must match the bound Session
    /// afterwards.
    #[serde(default)]
    pub session_id: Option<SessionId>,
    /// Full replacement model configuration for this Turn.
    #[serde(default)]
    pub model_config: Option<ModelConfig>,
}

/// `Option` fields are implicitly defaulted by serde; delegating explicitly
/// keeps the key required while still allowing an explicit `null`.
fn deserialize_required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

/// Resume command body.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ResumeRequest {
    /// Exact current Turn to take over.
    pub turn_id: TurnId,
}

/// Cancel command body.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    /// Exact current Turn to signal.
    pub turn_id: TurnId,
}

/// Approval resolve command body.
#[derive(Debug, Clone, PartialEq, Deserialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub struct ApprovalResolveRequest {
    /// Exact current Turn owning the approval.
    pub turn_id: TurnId,
    /// Human decision.
    pub decision: ApprovalDecision,
}

/// Accepted Turn identities returned by message and resume commands.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct TurnAccepted {
    /// Agent identity.
    pub agent_id: AgentId,
    /// Bound Session identity.
    pub session_id: SessionId,
    /// Current Turn identity.
    pub turn_id: TurnId,
}

/// Durable Agent status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentStatusDto {
    /// Created but never started a Turn.
    Idle,
    /// Current Turn is durably running.
    Running,
    /// Most recent Turn finished successfully.
    Finished,
    /// Most recent Turn failed.
    Failed,
    /// Most recent Turn was cancelled.
    Cancelled,
}

/// One undecided approval of the current Turn.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct PendingApprovalDto {
    /// Sequence of the request row, as a decimal string.
    pub requested_event_seq: String,
    /// Approval identity.
    pub approval_id: ApprovalId,
    /// Tool call identity.
    pub call_id: CallId,
    /// Provider-visible tool name.
    pub tool_name: ToolName,
    /// Final durable-safe arguments.
    pub arguments: serde_json::Value,
    /// Whether the tool observes or mutates state.
    pub tool_kind: ToolKind,
    /// Declared danger of the tool.
    pub danger_level: DangerLevel,
}

/// Cold Agent view at a fixed Postgres barrier.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AgentViewResponse {
    /// Agent identity.
    pub agent_id: AgentId,
    /// Agent name recorded at creation.
    pub agent_name: String,
    /// Durable status of the current or most recent Turn.
    pub status: AgentStatusDto,
    /// Current default model configuration.
    pub model_config: ModelConfig,
    /// Bound Session, when one exists.
    pub session_id: Option<SessionId>,
    /// Current or most recent Turn, when one exists.
    pub current_turn_id: Option<TurnId>,
    /// Snapshot barrier (`agent_state.last_event_seq`) as a decimal string.
    pub snapshot_event_seq: String,
    /// Latest durable assistant-message sequence at the same barrier, as a
    /// decimal string; `"0"` when no assistant message exists.
    pub telemetry_floor_event_seq: String,
    /// Undecided approvals of the current Turn within the barrier.
    pub pending_approvals: Vec<PendingApprovalDto>,
    /// Usage of the most recent usage-carrying event of the current Turn.
    pub latest_usage: Option<TokenUsage>,
    /// Process-local advisory: running but not hosted by this process.
    pub resume_required: bool,
}

/// One safe template catalog entry.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AgentTemplateDto {
    /// Template name.
    pub agent_name: String,
    /// Provider default model configuration of the template's model.
    pub model_config: ModelConfig,
}

/// Template catalog response.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct AgentTemplatesResponse {
    /// All currently valid templates.
    pub templates: Vec<AgentTemplateDto>,
}

/// Model catalog response.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct ModelsResponse {
    /// All configured models with their parameter schemas.
    pub models: Vec<ModelDescriptor>,
}

/// One product-visible history item.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HistoryItemDto {
    /// Agent-wide event sequence as a decimal string.
    pub event_seq: String,
    /// Durable payload version from the row.
    pub event_version: i32,
    /// Session owning the row.
    pub session_id: SessionId,
    /// Turn owning the row.
    pub turn_id: TurnId,
    /// Commit timestamp.
    pub created_at: DateTime<Utc>,
    /// Safe typed product event.
    pub event: AgentProductEventV1,
}

/// One ascending history page.
#[derive(Debug, Clone, PartialEq, Serialize, ToSchema)]
pub struct HistoryResponse {
    /// Items in ascending event order.
    pub items: Vec<HistoryItemDto>,
    /// Inclusive barrier of the fixed window, as a decimal string.
    pub through_event_seq: String,
    /// Exclusive cursor for the next (older) page, as a decimal string.
    pub next_before_event_seq: Option<String>,
    /// Whether older product-visible rows exist beyond this page.
    pub has_more: bool,
}

/// Liveness response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct LivenessResponse {
    /// Always `ok` when the process answers.
    pub status: &'static str,
}

/// Readiness response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ToSchema)]
pub struct ReadinessResponse {
    /// `ok` when Postgres serves; `unavailable` otherwise.
    pub status: &'static str,
    /// `ok` when the NATS tail is connected; `degraded` otherwise.
    pub realtime: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_request_requires_the_expected_turn_key_but_allows_null() {
        let missing: Result<MessageRequest, _> = serde_json::from_str(r#"{"text": "hello"}"#);
        assert!(missing.is_err(), "missing expected_current_turn_id fails");

        let null: MessageRequest =
            serde_json::from_str(r#"{"text": "hello", "expected_current_turn_id": null}"#)
                .expect("explicit null is accepted");
        assert_eq!(null.expected_current_turn_id, None);
    }

    #[test]
    fn request_dtos_reject_unknown_fields() {
        let with_credential: Result<MessageRequest, _> = serde_json::from_str(
            r#"{"text": "hi", "expected_current_turn_id": null, "api_key": "sk-secret"}"#,
        );
        assert!(with_credential.is_err(), "credential fields are rejected");

        let create: Result<CreateAgentRequest, _> = serde_json::from_str(
            r#"{"agent_name": "a", "session_id": "00000000-0000-0000-0000-000000000000"}"#,
        );
        assert!(
            create.is_err(),
            "create accepts no session or turn identity"
        );

        let resume: Result<ResumeRequest, _> = serde_json::from_str(
            r#"{"turn_id": "00000000-0000-0000-0000-000000000000", "model_config": null}"#,
        );
        assert!(resume.is_err(), "resume accepts no model override");
    }

    #[test]
    fn text_is_persisted_verbatim_and_only_trimmed_for_emptiness() {
        let request: MessageRequest = serde_json::from_str(
            r#"{"text": "  padded input  ", "expected_current_turn_id": null}"#,
        )
        .expect("request parses");
        assert_eq!(request.text, "  padded input  ");
    }
}
