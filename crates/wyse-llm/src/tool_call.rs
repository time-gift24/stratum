//! Tool-call types normalized across LLM providers.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wyse_core::{CallId, ToolId};

/// Tool definition exposed to an LLM provider.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    /// Internal tool identity.
    pub tool_id: ToolId,
    /// Provider-visible tool name.
    pub name: String,
    /// Provider-visible tool description.
    pub description: String,
    /// JSON schema for tool input.
    pub input_schema: Value,
}

/// Tool selection mode for a request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolChoice {
    /// Let the provider decide.
    Auto,
    /// Disable tool use.
    None,
    /// Require at least one tool call.
    Required,
    /// Require a specific tool.
    Tool(ToolId),
}

/// Complete tool call emitted by a model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Provider call identity.
    pub call_id: CallId,
    /// Internal tool identity.
    pub tool_id: ToolId,
    /// Provider-visible tool name.
    pub name: String,
    /// Parsed tool arguments.
    pub arguments: Value,
}

/// Incremental tool call update from a stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallDelta {
    /// Position of the tool call in the response.
    pub index: usize,
    /// Provider call identity when known.
    pub call_id: Option<CallId>,
    /// Provider-visible tool name when known.
    pub name: Option<String>,
    /// Raw argument text fragment.
    pub arguments_delta: String,
}
