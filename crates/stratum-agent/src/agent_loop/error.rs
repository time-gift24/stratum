//! Typed failures that stop the agent loop kernel.

use stratum_core::{CallId, ChatRole, ExtensionSetVersionId, HookFailure, HookPoint};
use stratum_infra::DurableEventSinkError;
use stratum_llm::LlmError;
use thiserror::Error;

use crate::ToolExecutorError;

/// Failure to construct an [`AgentLoop`](super::AgentLoop).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum AgentLoopBuildError {
    /// The model provider was not supplied.
    #[error("missing agent loop field llm_provider")]
    MissingLlmProvider,
    /// The tool executor was not supplied.
    #[error("missing agent loop field tool_executor")]
    MissingToolExecutor,
    /// The telemetry sink was not supplied.
    #[error("missing agent loop field telemetry")]
    MissingTelemetry,
}

/// Agent-loop protocol invariant that a provider response violated.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProtocolError {
    /// A loop run did not receive any new user prompt.
    #[error("agent loop prompts are empty")]
    EmptyPrompts,
    /// A new prompt had a role other than user.
    #[error("agent loop prompt has invalid role {role:?}")]
    InvalidPromptRole {
        /// Role rejected at the prompt boundary.
        role: ChatRole,
    },
    /// The provider stream ended without a terminal finish event.
    #[error("stream ended without a finish event")]
    StreamEndedWithoutFinish,
    /// Tool-call indices skipped an earlier position.
    #[error("tool call index {actual} is sparse; expected {expected}")]
    SparseToolCallIndex {
        /// Next contiguous index required by the protocol.
        expected: usize,
        /// Provider index that skipped the expected position.
        actual: usize,
    },
    /// One streamed index changed its provider call identity.
    #[error("tool call index {index} changed call id from {existing} to {received}")]
    ConflictingToolCallId {
        /// Provider position of the conflicting call.
        index: usize,
        /// First identity received for the position.
        existing: CallId,
        /// Later conflicting identity.
        received: CallId,
    },
    /// One streamed index changed its provider-visible tool name.
    #[error("tool call index {index} changed name from {existing} to {received}")]
    ConflictingToolCallName {
        /// Provider position of the conflicting call.
        index: usize,
        /// First name received for the position.
        existing: String,
        /// Later conflicting name.
        received: String,
    },
    /// A finalized tool call reused an identity from its batch or committed loop context.
    #[error("duplicate tool call id {call_id}")]
    DuplicateToolCallId {
        /// Duplicated provider call identity.
        call_id: CallId,
    },
    /// A streamed tool call did not contain every required field.
    #[error("tool call at index {index} is incomplete")]
    IncompleteToolCall {
        /// Provider position of the incomplete call.
        index: usize,
        /// Provider identity when it was received.
        call_id: Option<CallId>,
    },
    /// Tool-call argument fragments did not form valid JSON.
    #[error("tool call {call_id} arguments are invalid")]
    MalformedToolCallArguments {
        /// Provider identity of the malformed tool call.
        call_id: CallId,
        /// JSON parsing failure.
        #[source]
        source: serde_json::Error,
    },
}

/// Failure to rebuild a resumable run state from a durable event stream.
///
/// Every variant fails closed: the resume is refused before any model, tool,
/// or hook action starts.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum ResumeError {
    /// The stream did not contain its initial `LoopStarted` event.
    #[error("event stream is missing loop_started")]
    MissingLoopStarted,
    /// The stream contained a second `LoopStarted` event.
    #[error("event stream contains a duplicate loop_started")]
    UnexpectedLoopStarted,
    /// The stream contained a terminal event; finished runs cannot resume.
    #[error("event stream contains a terminal loop event")]
    TerminalEvent,
    /// Committed tool results are not the exact ordered prefix of the
    /// immediately preceding assistant `tool_calls` (unknown, duplicated,
    /// sparse, or out-of-order results).
    #[error(
        "committed tool results do not form the exact ordered prefix of the preceding assistant tool calls"
    )]
    ToolResultMismatch,
    /// Two pending records share one hook invocation address, or one
    /// invocation was completed twice.
    #[error("hook journal contains a duplicate invocation")]
    DuplicateHookInvocation,
    /// A completion or failure record references an unknown invocation.
    #[error("hook journal references an unknown invocation")]
    UnknownHookInvocation,
    /// A journaled decision does not belong to its hook point or fails
    /// re-validation against the rebuilt run state.
    #[error("hook journal record does not match the rebuilt invocation")]
    HookRecordMismatch,
    /// A journaled hook address matches the current invocation but its input
    /// digest does not.
    #[error("hook journal input digest mismatch at {}", hook_point_name(*point))]
    HookDigestMismatch {
        /// Decision point whose input changed across the crash boundary.
        point: HookPoint,
    },
    /// The extension set version recorded at `LoopStarted` differs from the
    /// version the currently injected hook runtime reports.
    #[error(
        "hook extension set version mismatch: stream recorded {recorded}, runtime reports {current}"
    )]
    ExtensionSetVersionMismatch {
        /// Version durably recorded with the run's `LoopStarted`.
        recorded: ExtensionSetVersionId,
        /// Version reported by the re-injected hook runtime.
        current: ExtensionSetVersionId,
    },
}

/// Failure that prevents the agent loop from preserving its invariants.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AgentLoopError {
    /// A required durable event was not acknowledged.
    #[error("durable agent event was not acknowledged")]
    Durability {
        /// Durable event sink failure.
        #[source]
        source: DurableEventSinkError,
    },
    /// Recording a terminal event failed after another loop operation had already failed.
    #[error("durable terminal agent event was not acknowledged")]
    TerminalDurability {
        /// Operation failure that initiated terminal recording.
        operation: Box<AgentLoopError>,
        /// Durable terminal event sink failure, which is the primary error source.
        #[source]
        source: DurableEventSinkError,
    },
    /// The model provider failed before producing a recoverable response.
    #[error("llm operation failed")]
    Llm {
        /// Model provider failure.
        #[source]
        source: LlmError,
    },
    /// Tool execution orchestration failed before producing a model-visible tool result.
    #[error("tool execution orchestration failed")]
    ToolExecution {
        /// Tool executor failure.
        #[source]
        source: ToolExecutorError,
    },
    /// A provider response violated the loop protocol.
    #[error("invalid agent loop protocol: {reason}")]
    InvalidProtocol {
        /// Typed protocol violation.
        #[source]
        reason: ProtocolError,
    },
    /// A hook runtime invocation failed or violated its decision contract.
    ///
    /// The failure is a safe classification: it never carries hook inputs, tool
    /// payloads, or internal runtime error text.
    #[error("hook at {} failed: {failure}", hook_point_name(*point))]
    Hook {
        /// Decision point whose invocation failed.
        point: HookPoint,
        /// Safe typed failure classification.
        failure: HookFailure,
    },
    /// A durable event stream could not be rebuilt into a consistent run
    /// state, or a journaled hook invocation contradicts the rebuilt state.
    #[error("agent loop resume failed: {reason}")]
    Resume {
        /// Typed resume failure.
        #[source]
        reason: ResumeError,
    },
    /// The caller cancelled the loop before a terminal outcome was committed.
    #[error("agent loop cancelled")]
    Cancelled,
    /// The run reached its model-iteration bound.
    #[error("maximum of {maximum} agent loop iterations reached")]
    IterationLimitExceeded {
        /// Configured maximum number of iterations.
        maximum: usize,
    },
    /// One model response exceeded its tool-call bound.
    #[error("maximum of {maximum} tool calls per iteration exceeded")]
    ToolCallLimitExceeded {
        /// Configured maximum number of tool calls per iteration.
        maximum: usize,
    },
    /// Streamed assistant text exceeded its byte bound.
    #[error("maximum of {maximum} streamed assistant text bytes exceeded")]
    TextByteLimitExceeded {
        /// Configured maximum number of bytes.
        maximum: usize,
    },
    /// Streamed reasoning exceeded its byte bound.
    #[error("maximum of {maximum} streamed reasoning bytes exceeded")]
    ReasoningByteLimitExceeded {
        /// Configured maximum number of bytes.
        maximum: usize,
    },
    /// One streamed tool call exceeded its argument byte bound.
    #[error("maximum of {maximum} streamed tool argument bytes exceeded")]
    ToolArgumentByteLimitExceeded {
        /// Configured maximum number of bytes.
        maximum: usize,
    },
}

impl From<DurableEventSinkError> for AgentLoopError {
    fn from(source: DurableEventSinkError) -> Self {
        Self::Durability { source }
    }
}

impl From<LlmError> for AgentLoopError {
    fn from(source: LlmError) -> Self {
        Self::Llm { source }
    }
}

impl From<ProtocolError> for AgentLoopError {
    fn from(reason: ProtocolError) -> Self {
        Self::InvalidProtocol { reason }
    }
}

impl From<ResumeError> for AgentLoopError {
    fn from(reason: ResumeError) -> Self {
        Self::Resume { reason }
    }
}

fn hook_point_name(point: HookPoint) -> &'static str {
    match point {
        HookPoint::TransformContext => "transform_context",
        HookPoint::TransformToolCall => "transform_tool_call",
        HookPoint::DecideToolCall => "decide_tool_call",
        HookPoint::AfterToolCall => "after_tool_call",
        HookPoint::PrepareNextTurn => "prepare_next_turn",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use super::*;

    #[test]
    fn durability_conversion_preserves_the_source_chain() {
        let error = AgentLoopError::from(DurableEventSinkError::UnsupportedEvent {
            event_type: "future_event",
        });

        assert!(matches!(&error, AgentLoopError::Durability { .. }));
        assert!(matches!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<DurableEventSinkError>()),
            Some(DurableEventSinkError::UnsupportedEvent {
                event_type: "future_event"
            })
        ));
    }

    #[test]
    fn llm_conversion_preserves_the_source_chain() {
        let error = AgentLoopError::from(LlmError::MockExhausted);

        assert!(matches!(&error, AgentLoopError::Llm { .. }));
        assert!(matches!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<LlmError>()),
            Some(LlmError::MockExhausted)
        ));
    }

    #[test]
    fn hook_error_exposes_only_the_point_and_safe_classification() {
        let error = AgentLoopError::Hook {
            point: HookPoint::DecideToolCall,
            failure: HookFailure::TimedOut,
        };

        assert_eq!(
            error.to_string(),
            "hook at decide_tool_call failed: hook invocation timed out"
        );
    }

    #[test]
    fn protocol_conversion_is_typed() {
        let error = AgentLoopError::from(ProtocolError::StreamEndedWithoutFinish);

        assert!(matches!(
            &error,
            AgentLoopError::InvalidProtocol {
                reason: ProtocolError::StreamEndedWithoutFinish
            }
        ));
        assert!(matches!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<ProtocolError>()),
            Some(ProtocolError::StreamEndedWithoutFinish)
        ));
    }
}
