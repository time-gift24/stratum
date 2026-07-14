//! Typed events emitted by the foundational agent loop.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel, LlmCallId, TokenUsage,
    ToolKind, ToolName,
};

/// Durable agent-loop events that require persistence acknowledgement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DurableAgentEvent {
    /// Agent loop started.
    LoopStarted,
    /// A complete message was appended to committed loop context.
    MessageAppended {
        /// Complete message payload.
        message: ChatMessage,
    },
    /// A tool call requires user approval.
    ToolApprovalRequested {
        /// Approval request identity.
        approval_id: ApprovalId,
        /// Tool call identity.
        call_id: CallId,
        /// Provider-visible tool name.
        tool_name: ToolName,
        /// Tool call arguments.
        arguments: Value,
        /// Whether the tool observes or mutates state.
        tool_kind: ToolKind,
        /// Declared danger of the tool.
        danger_level: DangerLevel,
    },
    /// A tool approval request was resolved.
    ToolApprovalResolved {
        /// Approval request identity.
        approval_id: ApprovalId,
        /// User decision.
        decision: ApprovalDecision,
    },
    /// A tool began executing after validation and approval.
    ToolExecutionStarted {
        /// Tool call identity.
        call_id: CallId,
        /// Provider-visible tool name.
        tool_name: ToolName,
    },
    /// One loop iteration reached its durable boundary.
    IterationCompleted {
        /// Iteration number.
        iteration: u64,
        /// Token usage accumulated through this iteration.
        usage: TokenUsage,
    },
    /// Agent loop finished successfully.
    LoopFinished {
        /// Why the loop finished.
        finish_reason: String,
        /// Token usage accumulated by the loop.
        usage: TokenUsage,
    },
    /// Agent loop failed.
    LoopFailed {
        /// Error text safe to expose to callers.
        error_text: String,
        /// Token usage accumulated by the loop.
        usage: TokenUsage,
    },
    /// Agent loop was cancelled.
    LoopCancelled {
        /// Token usage accumulated by the loop.
        usage: TokenUsage,
    },
}

impl DurableAgentEvent {
    /// Returns the stable serialized event type name.
    #[must_use]
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::LoopStarted => "loop_started",
            Self::MessageAppended { .. } => "message_appended",
            Self::ToolApprovalRequested { .. } => "tool_approval_requested",
            Self::ToolApprovalResolved { .. } => "tool_approval_resolved",
            Self::ToolExecutionStarted { .. } => "tool_execution_started",
            Self::IterationCompleted { .. } => "iteration_completed",
            Self::LoopFinished { .. } => "loop_finished",
            Self::LoopFailed { .. } => "loop_failed",
            Self::LoopCancelled { .. } => "loop_cancelled",
        }
    }
}

/// Best-effort agent-loop telemetry that does not control loop progress.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentTelemetryEvent {
    /// An LLM call started.
    LlmStarted {
        /// LLM call identity.
        llm_call_id: LlmCallId,
    },
    /// An LLM call emitted visible text.
    TextDelta {
        /// LLM call identity.
        llm_call_id: LlmCallId,
        /// Visible text fragment.
        delta: String,
    },
    /// An LLM call emitted reasoning text.
    ReasoningDelta {
        /// LLM call identity.
        llm_call_id: LlmCallId,
        /// Reasoning text fragment.
        delta: String,
    },
    /// An LLM call emitted a tool-call update.
    ToolCallDelta {
        /// LLM call identity.
        llm_call_id: LlmCallId,
        /// Tool call identity.
        call_id: CallId,
        /// Provider-visible tool name when known.
        name: Option<String>,
        /// Raw argument text fragment.
        arguments_delta: String,
    },
    /// An LLM call finished.
    LlmFinished {
        /// LLM call identity.
        llm_call_id: LlmCallId,
        /// Why the LLM call finished.
        finish_reason: String,
        /// Token usage reported by the provider, when available.
        usage: Option<TokenUsage>,
    },
    /// A tool emitted an execution progress update.
    ToolExecutionProgress {
        /// Tool call identity.
        call_id: CallId,
        /// Tool-specific progress payload.
        update: Value,
    },
}

impl AgentTelemetryEvent {
    /// Returns the stable serialized event type name.
    #[must_use]
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::LlmStarted { .. } => "llm_started",
            Self::TextDelta { .. } => "text_delta",
            Self::ReasoningDelta { .. } => "reasoning_delta",
            Self::ToolCallDelta { .. } => "tool_call_delta",
            Self::LlmFinished { .. } => "llm_finished",
            Self::ToolExecutionProgress { .. } => "tool_execution_progress",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AgentTelemetryEvent, DurableAgentEvent};
    use crate::{ChatMessage, LlmCallId};
    use serde_json::json;

    fn accept_durable(_: &DurableAgentEvent) {}

    fn accept_telemetry(_: &AgentTelemetryEvent) {}

    #[test]
    fn durable_message_event_serializes_with_stable_snake_case_type() -> serde_json::Result<()> {
        let event = DurableAgentEvent::MessageAppended {
            message: ChatMessage::user("hello"),
        };

        accept_durable(&event);
        assert_eq!(event.event_type(), "message_appended");
        assert_eq!(
            serde_json::to_value(event)?,
            json!({
                "type": "message_appended",
                "data": {
                    "message": {
                        "role": "user",
                        "content": {
                            "type": "text",
                            "data": "hello"
                        }
                    }
                }
            })
        );

        Ok(())
    }

    #[test]
    fn telemetry_delta_event_serializes_with_stable_snake_case_type() -> serde_json::Result<()> {
        let event = AgentTelemetryEvent::TextDelta {
            llm_call_id: LlmCallId::from("llm-call-1"),
            delta: "hel".to_owned(),
        };

        accept_telemetry(&event);
        assert_eq!(event.event_type(), "text_delta");
        assert_eq!(
            serde_json::to_value(event)?,
            json!({
                "type": "text_delta",
                "data": {
                    "llm_call_id": "llm-call-1",
                    "delta": "hel"
                }
            })
        );

        Ok(())
    }
}
