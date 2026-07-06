//! Chat message types.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wyse_core::CallId;

use crate::ToolCall;

/// Role of a chat message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChatRole {
    /// System instruction message.
    System,
    /// End-user message.
    User,
    /// Assistant response message.
    Assistant,
    /// Tool result message.
    Tool,
}

/// Content carried by a chat message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChatContent {
    /// Plain text content.
    Text(String),
    /// JSON content.
    Json(Value),
}

/// Message exchanged with an LLM provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    /// Message role.
    pub role: ChatRole,
    /// Message content.
    pub content: ChatContent,
    /// Tool calls requested by an assistant message.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Reasoning content produced by an assistant message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,
    /// Tool call this tool message answers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<CallId>,
}

impl ChatMessage {
    /// Creates a system text message.
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(ChatRole::System, content)
    }

    /// Creates a user text message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(ChatRole::User, content)
    }

    /// Creates an assistant text message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(ChatRole::Assistant, content)
    }

    /// Creates a text message for a role.
    #[must_use]
    pub fn text(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: ChatContent::Text(content.into()),
            tool_calls: Vec::new(),
            reasoning_content: None,
            tool_call_id: None,
        }
    }

    /// Sets assistant reasoning content.
    #[must_use]
    pub fn with_reasoning_content(mut self, content: impl Into<String>) -> Self {
        self.reasoning_content = Some(content.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::ChatMessage;

    #[test]
    fn assistant_message_can_carry_reasoning_content() {
        let message = ChatMessage::assistant("answer").with_reasoning_content("thinking");

        assert_eq!(message.reasoning_content.as_deref(), Some("thinking"));
    }

    #[test]
    fn reasoning_content_is_skipped_when_absent() {
        let value = serde_json::to_value(ChatMessage::assistant("answer"))
            .expect("message should serialize");

        assert!(value.get("reasoning_content").is_none());
    }
}
