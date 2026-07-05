//! Tool-call types normalized across LLM providers.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wyse_core::ToolId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub tool_id: ToolId,
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Tool(ToolId),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    pub call_id: String,
    pub tool_id: ToolId,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCallDelta {
    pub index: usize,
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub arguments_delta: String,
}
