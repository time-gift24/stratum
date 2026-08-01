//! Persisted agent state.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stratum_core::{
    AgentId, AgentLocation, AgentVersionId, ExtensionSetVersionId, HookHandlerVersionId,
    ModelConfig, SessionId, SkillSetVersionId, TokenUsage, TurnId, TurnRuntimeSnapshot,
};
use utoipa::ToSchema;

/// Current serialized agent-state schema version.
pub const AGENT_STATE_VERSION: u32 = 3;

/// Maximum number of messages returned by one history page.
pub const MAX_HISTORY_PAGE_SIZE: usize = 256;

/// Persisted runtime status of an agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgentStatus {
    /// The agent is ready for work.
    Idle,
    /// The agent is actively processing a turn.
    Running,
    /// The agent finished its work.
    Finished,
    /// The agent failed and cannot retry automatically.
    Failed,
    /// The agent was cancelled.
    Cancelled,
}

/// Strict persisted state for one agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentState {
    /// Serialized state schema version.
    pub state_version: u32,
    /// Agent identity.
    pub agent_id: AgentId,
    /// Human-readable agent name.
    pub name: String,
    /// Immutable version of the current agent definition.
    pub agent_version_id: AgentVersionId,
    /// Immutable version of the current ordered skill set.
    pub skill_set_version_id: SkillSetVersionId,
    /// Immutable version of the current ordered extension set.
    pub extension_set_version_id: ExtensionSetVersionId,
    /// Exact current hook handler order.
    pub hook_handler_versions: Vec<HookHandlerVersionId>,
    /// Stable model configuration, when persisted by a host-aware caller.
    #[serde(default)]
    pub model_config: Option<ModelConfig>,
    /// Current runtime status.
    pub status: AgentStatus,
    /// Long-lived session containing the current or most recent turn.
    pub session_id: Option<SessionId>,
    /// Active resumable turn, when any.
    pub turn_id: Option<TurnId>,
    /// Agent execution location for the current or most recent turn.
    pub location: Option<AgentLocation>,
    /// Exact runtime pinned for the active resumable turn.
    pub turn_runtime_snapshot: Option<TurnRuntimeSnapshot>,
    /// Next LLM loop iteration that has not reached a durable boundary.
    pub next_iteration: u64,
    /// Cumulative model token usage.
    pub usage: TokenUsage,
    /// Last committed message sequence.
    pub last_seq: u64,
    /// Last state update time.
    pub updated_at: DateTime<Utc>,
}

impl AgentState {
    /// Creates idle state for a new agent.
    #[must_use]
    pub fn new(agent_id: AgentId, name: String) -> Self {
        Self {
            state_version: AGENT_STATE_VERSION,
            agent_id,
            name,
            agent_version_id: AgentVersionId::new(),
            skill_set_version_id: SkillSetVersionId::new(),
            extension_set_version_id: ExtensionSetVersionId::new(),
            hook_handler_versions: Vec::new(),
            model_config: None,
            status: AgentStatus::Idle,
            session_id: None,
            turn_id: None,
            location: None,
            turn_runtime_snapshot: None,
            next_iteration: 0,
            usage: TokenUsage::default(),
            last_seq: 0,
            updated_at: Utc::now(),
        }
    }

    /// Creates idle state for a new host-configured agent.
    #[must_use]
    pub fn new_configured(agent_id: AgentId, name: String, model_config: ModelConfig) -> Self {
        Self {
            state_version: AGENT_STATE_VERSION,
            agent_id,
            name,
            agent_version_id: AgentVersionId::new(),
            skill_set_version_id: SkillSetVersionId::new(),
            extension_set_version_id: ExtensionSetVersionId::new(),
            hook_handler_versions: Vec::new(),
            model_config: Some(model_config),
            status: AgentStatus::Idle,
            session_id: None,
            turn_id: None,
            location: None,
            turn_runtime_snapshot: None,
            next_iteration: 0,
            usage: TokenUsage::default(),
            last_seq: 0,
            updated_at: Utc::now(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;
    use stratum_core::{AgentId, ModelConfig, ModelId};

    use super::*;

    fn test_model_config() -> ModelConfig {
        ModelConfig::new(
            ModelId::new("openai", "test-model").expect("static model is valid"),
            serde_json::Map::new(),
        )
    }

    #[test]
    fn agent_state_serializes_model_config() {
        let state =
            AgentState::new_configured(AgentId::new(), "writer".to_owned(), test_model_config());

        assert_eq!(
            serde_json::to_value(state).expect("state serializes")["model_config"]["model"],
            "openai:test-model"
        );
    }

    #[test]
    fn agent_state_serializes_only_approved_fields() {
        let state = AgentState::new(AgentId::new(), "writer".to_owned());
        assert_eq!(state.next_iteration, 0);
        let value = serde_json::to_value(state).expect("serialize state");
        let keys = value
            .as_object()
            .expect("state object")
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();

        assert_eq!(
            keys,
            BTreeSet::from([
                "agent_id".to_owned(),
                "agent_version_id".to_owned(),
                "extension_set_version_id".to_owned(),
                "hook_handler_versions".to_owned(),
                "last_seq".to_owned(),
                "location".to_owned(),
                "model_config".to_owned(),
                "name".to_owned(),
                "next_iteration".to_owned(),
                "session_id".to_owned(),
                "skill_set_version_id".to_owned(),
                "state_version".to_owned(),
                "status".to_owned(),
                "turn_runtime_snapshot".to_owned(),
                "turn_id".to_owned(),
                "updated_at".to_owned(),
                "usage".to_owned(),
            ])
        );
    }

    #[test]
    fn agent_state_rejects_unknown_fields() {
        let mut value = serde_json::to_value(AgentState::new(AgentId::new(), "writer".to_owned()))
            .expect("serialize state");
        value
            .as_object_mut()
            .expect("state object")
            .insert("owner_id".to_owned(), json!("x"));

        assert!(serde_json::from_value::<AgentState>(value).is_err());
    }
}
