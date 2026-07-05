//! Public LLM provider definitions.

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wyse_core::ModelId;

    use crate::{ChatMessage, ChatRequest, StructuredOutput};

    #[test]
    fn chat_request_uses_model_id_and_messages() {
        let request = ChatRequest::new(ModelId::from("gpt-4.1-mini"))
            .with_message(ChatMessage::user("hello"))
            .with_structured_output(StructuredOutput::JsonSchema {
                name: "answer".to_owned(),
                schema: json!({"type": "object"}),
                strict: true,
            });

        assert_eq!(request.model.as_str(), "gpt-4.1-mini");
        assert_eq!(request.messages.len(), 1);
        assert!(request.structured_output.is_some());
    }
}

use std::{future::Future, pin::Pin};

use futures_core::Stream;
use serde::{Deserialize, Serialize};
use wyse_core::{ModelId, TokenUsage};

use crate::{
    ChatMessage, LlmError, StructuredOutput, ToolCall, ToolCallDelta, ToolChoice, ToolSpec,
};

pub type ChatStream =
    Pin<Box<dyn Stream<Item = Result<ChatStreamEvent, LlmError>> + Send + 'static>>;

pub trait LlmProvider: Send + Sync {
    fn chat(
        &self,
        request: ChatRequest,
    ) -> impl Future<Output = Result<ChatResponse, LlmError>> + Send;

    fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> impl Future<Output = Result<ChatStream, LlmError>> + Send;
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: ModelId,
    pub messages: Vec<ChatMessage>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<StructuredOutput>,
}

impl ChatRequest {
    #[must_use]
    pub fn new(model: ModelId) -> Self {
        Self {
            model,
            messages: Vec::new(),
            tools: Vec::new(),
            tool_choice: None,
            structured_output: None,
        }
    }

    #[must_use]
    pub fn with_message(mut self, message: ChatMessage) -> Self {
        self.messages.push(message);
        self
    }

    #[must_use]
    pub fn with_structured_output(mut self, structured_output: StructuredOutput) -> Self {
        self.structured_output = Some(structured_output);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub message: ChatMessage,
    pub tool_calls: Vec<ToolCall>,
    pub finish_reason: FinishReason,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ChatStreamEvent {
    TextDelta {
        delta: String,
    },
    ToolCallDelta(ToolCallDelta),
    Finished {
        finish_reason: FinishReason,
        usage: Option<TokenUsage>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Unknown,
}
