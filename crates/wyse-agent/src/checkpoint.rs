//! Agent checkpoint state.

use serde::{Deserialize, Serialize};
use wyse_core::{AgentId, ChatMessage, TokenUsage};

use crate::AgentError;

pub(crate) const AGENT_CHECKPOINT_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct AgentCheckpointState {
    pub(crate) agent_id: AgentId,
    pub(crate) usage: TokenUsage,
    pub(crate) history: Vec<ChatMessage>,
}

impl AgentCheckpointState {
    pub(crate) fn encode(&self) -> Result<Vec<u8>, AgentError> {
        serde_json::to_vec(self).map_err(AgentError::CheckpointEncode)
    }

    pub(crate) fn decode(bytes: &[u8], version: u32) -> Result<Self, AgentError> {
        if version != AGENT_CHECKPOINT_STATE_VERSION {
            return Err(AgentError::UnsupportedCheckpointVersion { version });
        }
        serde_json::from_slice(bytes).map_err(AgentError::CheckpointDecode)
    }
}

#[cfg(test)]
mod tests {
    use wyse_core::{AgentId, ChatMessage, TokenUsage};

    use super::*;

    #[test]
    fn agent_checkpoint_state_encodes_only_resume_state() {
        let state = AgentCheckpointState {
            agent_id: AgentId::new(),
            usage: TokenUsage::default(),
            history: vec![ChatMessage::user("hello")],
        };

        let encoded = state.encode().expect("state encodes");
        let decoded = AgentCheckpointState::decode(&encoded, AGENT_CHECKPOINT_STATE_VERSION)
            .expect("state decodes");
        let encoded_json: serde_json::Value =
            serde_json::from_slice(&encoded).expect("state should encode as json");

        assert_eq!(decoded, state);
        assert!(encoded_json.get("agent_id").is_some());
        assert!(encoded_json.get("usage").is_some());
        assert!(encoded_json.get("history").is_some());
        assert!(encoded_json.get("phase").is_none());
        assert!(encoded_json.get("retry_count").is_none());
        assert!(encoded_json.get("last_error_text").is_none());
        assert!(encoded_json.get("pending_tool_calls").is_none());
        assert!(encoded_json.get("next_tool_call_index").is_none());
    }

    #[test]
    fn agent_checkpoint_state_decodes_legacy_v1_extra_fields() {
        let agent_id = AgentId::new();
        let history = vec![ChatMessage::user("hello")];
        let state = serde_json::json!({
            "agent_id": agent_id,
            "phase": {
                "type": "running_llm",
                "data": {
                    "turn_index": 0
                }
            },
            "retry_count": 1,
            "last_error_text": "provider failed",
            "usage": TokenUsage::default(),
            "history": history,
            "pending_tool_calls": [],
            "next_tool_call_index": 0
        });
        let bytes = serde_json::to_vec(&state).expect("legacy state should encode");

        let decoded = AgentCheckpointState::decode(&bytes, AGENT_CHECKPOINT_STATE_VERSION)
            .expect("legacy state should decode");

        assert_eq!(decoded.agent_id, agent_id);
        assert_eq!(decoded.history, vec![ChatMessage::user("hello")]);
        assert_eq!(decoded.usage, TokenUsage::default());
    }
}
