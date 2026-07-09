//! Agent checkpoint state.

use serde::{Deserialize, Serialize};
use wyse_core::{AgentId, ChatMessage, TokenUsage, ToolCall};

use crate::AgentError;

#[allow(dead_code)]
pub(crate) const AGENT_CHECKPOINT_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[allow(dead_code)]
pub(crate) enum AgentCheckpointPhase {
    ReadyForLlm {
        turn_index: usize,
    },
    RunningLlm {
        turn_index: usize,
    },
    RunningTools {
        turn_index: usize,
        tool_calls: Vec<ToolCall>,
        next_tool_call_index: usize,
    },
    Finished {
        finish_reason: String,
    },
    Failed {
        error_text: String,
    },
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(dead_code)]
pub(crate) struct AgentCheckpointState {
    pub(crate) agent_id: AgentId,
    pub(crate) phase: AgentCheckpointPhase,
    pub(crate) retry_count: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) last_error_text: Option<String>,
    pub(crate) usage: TokenUsage,
    pub(crate) history: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) pending_tool_calls: Vec<ToolCall>,
    pub(crate) next_tool_call_index: usize,
}

#[allow(dead_code)]
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
    fn agent_checkpoint_state_round_trips_json_bytes() {
        let state = AgentCheckpointState {
            agent_id: AgentId::new(),
            phase: AgentCheckpointPhase::ReadyForLlm { turn_index: 0 },
            retry_count: 0,
            last_error_text: None,
            usage: TokenUsage::default(),
            history: vec![ChatMessage::user("hello")],
            pending_tool_calls: Vec::new(),
            next_tool_call_index: 0,
        };

        let encoded = state.encode().expect("state encodes");
        let decoded = AgentCheckpointState::decode(&encoded, AGENT_CHECKPOINT_STATE_VERSION)
            .expect("state decodes");

        assert_eq!(decoded, state);
    }
}
