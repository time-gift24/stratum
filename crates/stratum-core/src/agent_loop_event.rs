//! Typed events emitted by the foundational agent loop.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel, ExtensionSetVersionId,
    HookFailure, HookInputDigest, HookInvocationId, HookPoint, LlmCallId, TokenUsage, ToolKind,
    ToolName,
};

/// Durable agent-loop events that require persistence acknowledgement.
///
/// The persistence format is JSON (`{"type": ..., "data": ...}` per event).
/// Deserialization is strict: unknown variants or malformed shapes fail
/// closed; no legacy shapes are upgraded (the beta filesystem backend that
/// produced them is deleted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DurableAgentEvent {
    /// Agent loop started.
    LoopStarted {
        /// Immutable ordered extension set version reported by the hook
        /// runtime for this run, or `None` when the runtime pins no handler
        /// chain. Resume compares a recorded version against the version the
        /// re-injected runtime reports and fails closed on a mismatch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        extension_set_version_id: Option<ExtensionSetVersionId>,
    },
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
    /// A hook invocation was journaled before calling the hook runtime.
    ///
    /// The address is the kernel-minimal `(iteration, HookPoint, Option<CallId>)`
    /// shape: tool hooks distinguish same-iteration calls by `call_id`, while
    /// `transform_context` and `prepare_next_turn` are uniquely identified by
    /// `(iteration, point)`.
    HookInvocationPending {
        /// Logical invocation identity; retries of the same logical invocation
        /// reuse this id instead of creating a second record.
        invocation_id: HookInvocationId,
        /// Decision point being invoked.
        point: HookPoint,
        /// Zero-based model iteration the invocation belongs to.
        iteration: u64,
        /// Tool call identity for tool hooks; `None` for context hooks.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        call_id: Option<CallId>,
        /// Payload-level digest of the hook input.
        input_digest: HookInputDigest,
    },
    /// A hook invocation produced a validated decision, journaled before the
    /// decision's affected action is applied.
    HookInvocationCompleted {
        /// Identity of the pending invocation this decision completes.
        invocation_id: HookInvocationId,
        /// Typed decision record; the payload is the decision itself.
        decision: HookDecisionRecord,
    },
    /// A hook invocation reached a typed terminal failure.
    HookInvocationFailed {
        /// Identity of the pending invocation this failure terminates.
        invocation_id: HookInvocationId,
        /// Safe typed failure classification.
        failure: HookFailure,
    },
    /// The committed transcript prefix was durably compacted.
    ///
    /// The kernel commits this event at an iteration boundary after a
    /// `prepare_next_turn` hook returned a validated compact decision, before
    /// that iteration's `IterationCompleted`. The event log keeps every
    /// original message; only the rebuilt view changes: replay replaces the
    /// rebuilt prefix `[0, upto)` with `summary`. `upto` is a zero-based,
    /// left-closed/right-open index into the committed context exactly as the
    /// prepare snapshot presented it.
    TranscriptCompacted {
        /// Exclusive end index of the replaced committed-context prefix.
        upto: u64,
        /// Kernel-owned system marker message replacing the prefix.
        summary: ChatMessage,
        /// Iteration whose prepare boundary executed the compaction.
        compacted_iteration: u64,
    },
    /// One loop iteration reached its durable boundary.
    IterationCompleted {
        /// Iteration number.
        iteration: u64,
        /// Token usage reported by the most recent model response up to this
        /// boundary, zero-filled when no response reported usage.
        usage: TokenUsage,
    },
    /// Agent loop finished successfully.
    LoopFinished {
        /// Why the loop finished.
        finish_reason: String,
        /// Token usage reported by the most recent model response, zero-filled
        /// when no response reported usage.
        usage: TokenUsage,
    },
    /// Agent loop failed.
    LoopFailed {
        /// Error text safe to expose to callers.
        error_text: String,
        /// Token usage reported by the most recent model response, zero-filled
        /// when no response reported usage.
        usage: TokenUsage,
    },
    /// Agent loop was cancelled.
    LoopCancelled {
        /// Token usage reported by the most recent model response, zero-filled
        /// when no response reported usage.
        usage: TokenUsage,
    },
}

impl DurableAgentEvent {
    /// Returns the stable serialized event type name.
    #[must_use]
    pub const fn event_type(&self) -> &'static str {
        match self {
            Self::LoopStarted { .. } => "loop_started",
            Self::MessageAppended { .. } => "message_appended",
            Self::ToolApprovalRequested { .. } => "tool_approval_requested",
            Self::ToolApprovalResolved { .. } => "tool_approval_resolved",
            Self::ToolExecutionStarted { .. } => "tool_execution_started",
            Self::HookInvocationPending { .. } => "hook_invocation_pending",
            Self::HookInvocationCompleted { .. } => "hook_invocation_completed",
            Self::HookInvocationFailed { .. } => "hook_invocation_failed",
            Self::TranscriptCompacted { .. } => "transcript_compacted",
            Self::IterationCompleted { .. } => "iteration_completed",
            Self::LoopFinished { .. } => "loop_finished",
            Self::LoopFailed { .. } => "loop_failed",
            Self::LoopCancelled { .. } => "loop_cancelled",
        }
    }
}

/// Incremental patch a `transform_context` hook decision applies to the
/// current model request view.
///
/// Patches are request-scoped view adjustments: they never write back to the
/// committed transcript, never become durable messages, and never appear in
/// the loop outcome. `upto` indexes the committed `messages` with a zero-based,
/// left-closed/right-open interval: the patch rewrites the prefix
/// `messages[..upto]`. The kernel rejects a patch whose `upto` is out of bounds
/// or cuts a tool_call/tool_result pair (an assistant message's `tool_calls`
/// and their results must stay on the same side of the cut) as
/// [`HookFailure::InvalidOutput`]. A `Composite` applies its sub-patches in
/// order; the first sub-patch indexes the committed `messages`, and each later
/// sub-patch indexes the view produced by the previous ones, validated against
/// that evolving view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ContextPatch {
    /// Replace the system prompt of the current request view.
    ReplaceSystemPrompt(String),
    /// Drop the committed history prefix `messages[..upto]` from the current
    /// request view.
    DropHistory {
        /// Exclusive end index into the committed `messages`.
        upto: usize,
    },
    /// Replace the committed history prefix `messages[..upto]` with one
    /// summary message in the current request view.
    RewriteHistory {
        /// Exclusive end index into the committed `messages`.
        upto: usize,
        /// Summary message taking the place of the dropped prefix.
        summary: ChatMessage,
    },
    /// Apply several patches in order; each sub-patch addresses the request
    /// view produced by the previous ones.
    ///
    /// Hook handler chains produce a composition when more than one handler
    /// patches the same request; a single-handler patch stays unwrapped. An
    /// empty composition is rejected as [`HookFailure::InvalidOutput`], and so
    /// is a nested composition: a sub-patch must not itself be a `Composite`.
    Composite(Vec<ContextPatch>),
}

/// Journaled representation of one hook decision at one hook point.
///
/// Every payload is the small inline decision itself; there is no overflow or
/// blob storage form. Resume replays a matching record instead of calling the
/// hook runtime again.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "point", content = "decision", rename_all = "snake_case")]
#[non_exhaustive]
pub enum HookDecisionRecord {
    /// Decision of a `transform_context` invocation.
    TransformContext(TransformContextDecisionRecord),
    /// Decision of a `transform_tool_call` invocation.
    TransformToolCall(TransformToolCallDecisionRecord),
    /// Decision of a `decide_tool_call` invocation.
    DecideToolCall(DecideToolCallDecisionRecord),
    /// Decision of an `after_tool_call` invocation.
    AfterToolCall(AfterToolCallDecisionRecord),
    /// Decision of a `prepare_next_turn` invocation.
    PrepareNextTurn(PrepareNextTurnDecisionRecord),
}

/// Journaled `transform_context` decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TransformContextDecisionRecord {
    /// The request view was used unchanged.
    Unchanged,
    /// An incremental patch was applied to the request view.
    Patch(ContextPatch),
}

/// Journaled `transform_tool_call` decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum TransformToolCallDecisionRecord {
    /// The original arguments and authorization were kept.
    Continue,
    /// Arguments and/or the effective authorization were modified.
    Modify(TransformToolCallModificationRecord),
}

/// Journaled per-call modification; `None` fields kept their original values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TransformToolCallModificationRecord {
    /// Replacement arguments, when modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
    /// Effective authorization override, when modified.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorization: Option<AuthorizationOverrideRecord>,
}

impl TransformToolCallModificationRecord {
    /// Creates a modification record from optional replacement arguments and an
    /// optional authorization override.
    #[must_use]
    pub const fn new(
        arguments: Option<Value>,
        authorization: Option<AuthorizationOverrideRecord>,
    ) -> Self {
        Self {
            arguments,
            authorization,
        }
    }
}

/// Journaled per-call authorization override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuthorizationOverrideRecord {
    /// The call was marked pre-authorized.
    PreAuthorize,
    /// The declared authorization metadata was replaced.
    Set {
        /// Replacement tool kind.
        kind: ToolKind,
        /// Replacement danger level.
        danger: DangerLevel,
    },
}

/// Journaled `decide_tool_call` decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DecideToolCallDecisionRecord {
    /// The call was committed and executed.
    Execute,
    /// The call was blocked with a model-visible reason.
    Block {
        /// Model-visible block reason.
        reason: String,
    },
}

/// Journaled `after_tool_call` decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AfterToolCallDecisionRecord {
    /// The produced tool result was committed unchanged.
    Keep,
    /// A replacement JSON result was committed under the same call identity.
    ReplaceResult {
        /// Replacement JSON tool result.
        result: Value,
    },
}

/// Journaled `prepare_next_turn` decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum PrepareNextTurnDecisionRecord {
    /// The iteration boundary was committed and the next iteration started.
    Continue,
    /// The iteration boundary was committed and the loop finished hook-stopped.
    Stop,
    /// Plain user messages were injected into the next request view only.
    Inject {
        /// Non-empty plain user messages for the next request view.
        messages: Vec<ChatMessage>,
    },
    /// The committed transcript prefix `[0, upto)` was compacted into a
    /// durable summary marker at this iteration boundary.
    Compact {
        /// Exclusive end index of the replaced prefix, in the committed-context
        /// coordinates of the prepare snapshot.
        upto: usize,
        /// Handler-supplied summary message; validated as a plain system
        /// message before journaling.
        summary: ChatMessage,
    },
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentTelemetryEvent, DecideToolCallDecisionRecord, DurableAgentEvent, HookDecisionRecord,
    };
    use crate::{
        ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel, ExtensionSetVersionId,
        HookFailure, HookInvocationId, HookPoint, LlmCallId, TokenUsage, ToolKind, ToolName,
    };
    use serde_json::json;

    #[test]
    fn loop_started_carries_an_optional_extension_set_version() -> serde_json::Result<()> {
        let version = ExtensionSetVersionId::new();
        let event = DurableAgentEvent::LoopStarted {
            extension_set_version_id: Some(version),
        };

        assert_eq!(event.event_type(), "loop_started");
        let serialized = serde_json::to_value(&event)?;
        assert_eq!(
            serialized,
            json!({
                "type": "loop_started",
                "data": {
                    "extension_set_version_id": version.to_string(),
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<DurableAgentEvent>(serialized)?,
            event
        );

        let unpinned = DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        };
        let serialized = serde_json::to_value(&unpinned)?;
        assert!(serialized["data"].get("extension_set_version_id").is_none());
        assert_eq!(
            serde_json::from_value::<DurableAgentEvent>(serialized)?,
            unpinned
        );

        Ok(())
    }

    #[test]
    fn legacy_loop_started_without_data_is_rejected() {
        // The beta filesystem backend that wrote data-less `loop_started`
        // lines is deleted; the shape must fail closed, never be upgraded.
        let legacy = json!({ "type": "loop_started" });

        assert!(serde_json::from_value::<DurableAgentEvent>(legacy).is_err());
    }

    #[test]
    fn durable_message_event_serializes_with_stable_snake_case_type() -> serde_json::Result<()> {
        let event = DurableAgentEvent::MessageAppended {
            message: ChatMessage::user("hello"),
        };

        assert_eq!(event.event_type(), "message_appended");
        let serialized = serde_json::to_value(&event)?;
        assert_eq!(
            serialized,
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
        assert_eq!(
            serde_json::from_value::<DurableAgentEvent>(serialized)?,
            event
        );

        Ok(())
    }

    #[test]
    fn hook_journal_events_serialize_with_stable_snake_case_type() -> serde_json::Result<()> {
        let invocation_id = HookInvocationId::new();
        let digest = "b".repeat(64).parse().expect("valid digest");
        let pending = DurableAgentEvent::HookInvocationPending {
            invocation_id,
            point: HookPoint::TransformContext,
            iteration: 3,
            call_id: None,
            input_digest: digest,
        };

        assert_eq!(pending.event_type(), "hook_invocation_pending");
        let serialized = serde_json::to_value(&pending)?;
        assert_eq!(
            serialized,
            json!({
                "type": "hook_invocation_pending",
                "data": {
                    "invocation_id": invocation_id.to_string(),
                    "point": "transform_context",
                    "iteration": 3,
                    "input_digest": "b".repeat(64),
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<DurableAgentEvent>(serialized)?,
            pending
        );

        let completed = DurableAgentEvent::HookInvocationCompleted {
            invocation_id,
            decision: HookDecisionRecord::TransformContext(
                super::TransformContextDecisionRecord::Patch(super::ContextPatch::DropHistory {
                    upto: 2,
                }),
            ),
        };
        assert_eq!(completed.event_type(), "hook_invocation_completed");
        let serialized = serde_json::to_value(&completed)?;
        assert_eq!(
            serialized["data"]["decision"],
            json!({
                "point": "transform_context",
                "decision": {
                    "type": "patch",
                    "data": {
                        "type": "drop_history",
                        "data": { "upto": 2 }
                    }
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<DurableAgentEvent>(serialized)?,
            completed
        );

        Ok(())
    }

    #[test]
    fn transcript_compacted_serializes_with_stable_snake_case_type() -> serde_json::Result<()> {
        let event = DurableAgentEvent::TranscriptCompacted {
            upto: 3,
            summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary so far"),
            compacted_iteration: 1,
        };

        assert_eq!(event.event_type(), "transcript_compacted");
        let serialized = serde_json::to_value(&event)?;
        assert_eq!(
            serialized,
            json!({
                "type": "transcript_compacted",
                "data": {
                    "upto": 3,
                    "summary": {
                        "role": "system",
                        "content": {
                            "type": "text",
                            "data": "[stratum:transcript-compacted]\nsummary so far"
                        }
                    },
                    "compacted_iteration": 1,
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<DurableAgentEvent>(serialized)?,
            event
        );

        Ok(())
    }

    #[test]
    fn prepare_compact_record_serializes_with_stable_snake_case_type() -> serde_json::Result<()> {
        let record =
            HookDecisionRecord::PrepareNextTurn(super::PrepareNextTurnDecisionRecord::Compact {
                upto: 2,
                summary: ChatMessage::system("summary so far"),
            });

        let serialized = serde_json::to_value(&record)?;
        assert_eq!(
            serialized,
            json!({
                "point": "prepare_next_turn",
                "decision": {
                    "type": "compact",
                    "data": {
                        "upto": 2,
                        "summary": {
                            "role": "system",
                            "content": {
                                "type": "text",
                                "data": "summary so far"
                            }
                        }
                    }
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<HookDecisionRecord>(serialized)?,
            record
        );

        Ok(())
    }

    #[test]
    fn telemetry_delta_event_serializes_with_stable_snake_case_type() -> serde_json::Result<()> {
        let event = AgentTelemetryEvent::TextDelta {
            llm_call_id: LlmCallId::from("llm-call-1"),
            delta: "hel".to_owned(),
        };

        assert_eq!(event.event_type(), "text_delta");
        let serialized = serde_json::to_value(&event)?;
        assert_eq!(
            serialized,
            json!({
                "type": "text_delta",
                "data": {
                    "llm_call_id": "llm-call-1",
                    "delta": "hel"
                }
            })
        );
        assert_eq!(
            serde_json::from_value::<AgentTelemetryEvent>(serialized)?,
            event
        );

        Ok(())
    }

    #[test]
    fn telemetry_none_fields_are_omitted() -> serde_json::Result<()> {
        let tool_delta = serde_json::to_value(AgentTelemetryEvent::ToolCallDelta {
            llm_call_id: LlmCallId::from("llm-call-1"),
            call_id: CallId::from("tool-call-1"),
            name: None,
            arguments_delta: "{}".to_owned(),
        })?;
        let llm_finished = serde_json::to_value(AgentTelemetryEvent::LlmFinished {
            llm_call_id: LlmCallId::from("llm-call-1"),
            finish_reason: "stop".to_owned(),
            usage: None,
        })?;

        assert!(tool_delta["data"].get("name").is_none());
        assert!(llm_finished["data"].get("usage").is_none());

        Ok(())
    }

    #[test]
    fn every_durable_event_type_matches_its_serialized_type() -> serde_json::Result<()> {
        let usage = TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            total_tokens: 3,
        };
        let events = vec![
            DurableAgentEvent::LoopStarted {
                extension_set_version_id: None,
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("hello"),
            },
            DurableAgentEvent::ToolApprovalRequested {
                approval_id: ApprovalId::new(),
                call_id: CallId::from("tool-call-1"),
                tool_name: ToolName::from("echo"),
                arguments: json!({ "text": "hello" }),
                tool_kind: ToolKind::Read,
                danger_level: DangerLevel::Low,
            },
            DurableAgentEvent::ToolApprovalResolved {
                approval_id: ApprovalId::new(),
                decision: ApprovalDecision::Approve,
            },
            DurableAgentEvent::ToolExecutionStarted {
                call_id: CallId::from("tool-call-1"),
                tool_name: ToolName::from("echo"),
            },
            DurableAgentEvent::HookInvocationPending {
                invocation_id: HookInvocationId::new(),
                point: HookPoint::DecideToolCall,
                iteration: 0,
                call_id: Some(CallId::from("tool-call-1")),
                input_digest: "a".repeat(64).parse().expect("valid digest"),
            },
            DurableAgentEvent::HookInvocationCompleted {
                invocation_id: HookInvocationId::new(),
                decision: HookDecisionRecord::DecideToolCall(DecideToolCallDecisionRecord::Execute),
            },
            DurableAgentEvent::HookInvocationFailed {
                invocation_id: HookInvocationId::new(),
                failure: HookFailure::TimedOut,
            },
            DurableAgentEvent::TranscriptCompacted {
                upto: 4,
                summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary so far"),
                compacted_iteration: 1,
            },
            DurableAgentEvent::IterationCompleted {
                iteration: 1,
                usage,
            },
            DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage,
            },
            DurableAgentEvent::LoopFailed {
                error_text: "provider unavailable".to_owned(),
                usage,
            },
            DurableAgentEvent::LoopCancelled { usage },
        ];

        for event in events {
            let serialized = serde_json::to_value(&event)?;
            assert_eq!(serialized["type"], json!(event.event_type()));
            // Round-tripping every variant through the manual `Deserialize`
            // front door keeps the private wire mirror in sync.
            assert_eq!(
                serde_json::from_value::<DurableAgentEvent>(serialized)?,
                event
            );
        }

        Ok(())
    }

    #[test]
    fn every_telemetry_event_type_matches_its_serialized_type() -> serde_json::Result<()> {
        let llm_call_id = LlmCallId::from("llm-call-1");
        let events = vec![
            AgentTelemetryEvent::LlmStarted {
                llm_call_id: llm_call_id.clone(),
            },
            AgentTelemetryEvent::TextDelta {
                llm_call_id: llm_call_id.clone(),
                delta: "hello".to_owned(),
            },
            AgentTelemetryEvent::ReasoningDelta {
                llm_call_id: llm_call_id.clone(),
                delta: "thinking".to_owned(),
            },
            AgentTelemetryEvent::ToolCallDelta {
                llm_call_id: llm_call_id.clone(),
                call_id: CallId::from("tool-call-1"),
                name: Some("echo".to_owned()),
                arguments_delta: "{}".to_owned(),
            },
            AgentTelemetryEvent::LlmFinished {
                llm_call_id,
                finish_reason: "stop".to_owned(),
                usage: Some(TokenUsage {
                    input_tokens: 1,
                    output_tokens: 2,
                    total_tokens: 3,
                }),
            },
        ];

        for event in events {
            assert_eq!(
                serde_json::to_value(&event)?["type"],
                json!(event.event_type())
            );
        }

        Ok(())
    }
}
