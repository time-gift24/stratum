//! Chat message types.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wyse_core::ToolId;

use crate::ToolCall;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChatRole {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChatContent {
    Text(String),
    Json(Value),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: ChatContent,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_id: Option<ToolId>,
}

impl ChatMessage {
    #[must_use]
    pub fn system(content: impl Into<String>) -> Self {
        Self::text(ChatRole::System, content)
    }

    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self::text(ChatRole::User, content)
    }

    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self::text(ChatRole::Assistant, content)
    }

    #[must_use]
    pub fn text(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: ChatContent::Text(content.into()),
            tool_calls: Vec::new(),
            tool_call_id: None,
            tool_id: None,
        }
    }
}
