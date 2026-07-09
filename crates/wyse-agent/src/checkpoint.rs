//! Agent checkpoint payload helpers.

use serde::{Deserialize, Serialize};
use wyse_core::{AgentId, ChatMessage, TokenUsage};

use crate::AgentError;

pub(crate) const AGENT_CHECKPOINT_STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CheckpointPayload {
    agent_id: AgentId,
    usage: TokenUsage,
    history: Vec<ChatMessage>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DecodedCheckpoint {
    pub(crate) agent_id: AgentId,
    pub(crate) usage: TokenUsage,
    pub(crate) history: Vec<ChatMessage>,
}

pub(crate) fn encode_checkpoint_payload(
    agent_id: AgentId,
    usage: TokenUsage,
    history: &[ChatMessage],
) -> Result<Vec<u8>, AgentError> {
    let payload = CheckpointPayload {
        agent_id,
        usage,
        history: history.to_vec(),
    };
    serde_json::to_vec(&payload).map_err(AgentError::CheckpointEncode)
}

pub(crate) fn decode_checkpoint_payload(
    bytes: &[u8],
    version: u32,
) -> Result<DecodedCheckpoint, AgentError> {
    if version != AGENT_CHECKPOINT_STATE_VERSION {
        return Err(AgentError::UnsupportedCheckpointVersion { version });
    }
    let payload: CheckpointPayload =
        serde_json::from_slice(bytes).map_err(AgentError::CheckpointDecode)?;
    Ok(DecodedCheckpoint {
        agent_id: payload.agent_id,
        usage: payload.usage,
        history: payload.history,
    })
}

#[cfg(test)]
mod tests {
    use wyse_core::{AgentId, ChatMessage, TokenUsage};

    use super::*;

    #[test]
    fn checkpoint_payload_encodes_only_resume_data() {
        let agent_id = AgentId::new();
        let history = vec![ChatMessage::user("hello")];

        let encoded = encode_checkpoint_payload(agent_id, TokenUsage::default(), &history)
            .expect("payload encodes");
        let decoded = decode_checkpoint_payload(&encoded, AGENT_CHECKPOINT_STATE_VERSION)
            .expect("payload decodes");
        let encoded_json: serde_json::Value =
            serde_json::from_slice(&encoded).expect("payload should encode as json");

        assert_eq!(decoded.agent_id, agent_id);
        assert_eq!(decoded.usage, TokenUsage::default());
        assert_eq!(decoded.history, history);
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
    fn checkpoint_payload_decodes_legacy_v1_extra_fields() {
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
        let bytes = serde_json::to_vec(&state).expect("legacy payload should encode");

        let decoded = decode_checkpoint_payload(&bytes, AGENT_CHECKPOINT_STATE_VERSION)
            .expect("legacy payload should decode");

        assert_eq!(decoded.agent_id, agent_id);
        assert_eq!(decoded.history, vec![ChatMessage::user("hello")]);
        assert_eq!(decoded.usage, TokenUsage::default());
    }
}
