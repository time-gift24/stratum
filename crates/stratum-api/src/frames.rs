//! API-owned realtime protocol: `AgentStreamFrameV1` and the safe product
//! event union `AgentProductEventV1`.
//!
//! Frames are the only public realtime wire shape. Durable frames carry the
//! Agent-wide `event_seq` as a decimal string; telemetry frames carry the
//! call-local `(llm_call_id, telemetry_seq)` identity plus a decimal-string PG
//! ordering watermark. Raw durable payloads, runtime snapshots, hook journal
//! rows, `ToolExecutionStarted`, and internal error sources are never
//! serialized into a frame.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use stratum_core::{
    AgentId, AgentTelemetryEvent, ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel,
    DurableAgentEvent, LlmCallId, SessionId, TokenUsage, ToolKind, ToolName, TurnId,
};
use stratum_postgres::{DurableEventRow, HistoryItem, encode_event_seq};
use utoipa::ToSchema;

use crate::error::PersistedVariantError;

/// Protocol version of every frame this binary emits.
pub const PROTOCOL_VERSION_V1: u8 = 1;

/// Store rows are `#[non_exhaustive]`, so the dispatcher and tests work with
/// this local projection of one scanned durable row.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedRow {
    /// Agent-wide sequence.
    pub event_seq: u64,
    /// Durable payload version of the row.
    pub event_version: i32,
    /// Session owning the row.
    pub session_id: SessionId,
    /// Turn owning the row.
    pub turn_id: TurnId,
    /// Commit timestamp.
    pub created_at: DateTime<Utc>,
    /// Materialized typed event.
    pub event: DurableAgentEvent,
}

impl From<DurableEventRow> for ScannedRow {
    fn from(row: DurableEventRow) -> Self {
        Self {
            event_seq: row.event_seq,
            event_version: row.event_version,
            session_id: row.session_id,
            turn_id: row.turn_id,
            created_at: row.created_at,
            event: row.event,
        }
    }
}

/// One frame of the Agent-scoped realtime stream (SSE `data` payload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentStreamFrameV1 {
    /// Locally produced control signal; never carries an SSE id.
    Control {
        /// Protocol version; always 1.
        protocol_version: u8,
        /// Owning Agent.
        agent_id: AgentId,
        /// Bound Session, when one exists.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        session_id: Option<SessionId>,
        /// Current Turn, when one exists.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<TurnId>,
        /// Frame creation time.
        created_at: DateTime<Utc>,
        /// Control payload.
        event: ControlEventV1,
    },
    /// One committed product event of the durable ledger.
    Durable {
        /// Protocol version; always 1.
        protocol_version: u8,
        /// Owning Agent.
        agent_id: AgentId,
        /// Session owning the durable row.
        session_id: SessionId,
        /// Turn owning the durable row.
        turn_id: TurnId,
        /// Frame creation time.
        created_at: DateTime<Utc>,
        /// Agent-wide event sequence as a decimal string.
        event_seq: String,
        /// Durable event payload version from the row.
        event_version: i32,
        /// Safe typed product event.
        event: AgentProductEventV1,
    },
    /// One volatile LLM telemetry event.
    Telemetry {
        /// Protocol version; always 1.
        protocol_version: u8,
        /// Owning Agent.
        agent_id: AgentId,
        /// Session owning the active Turn.
        session_id: SessionId,
        /// Turn owning the active LLM call.
        turn_id: TurnId,
        /// Frame creation time.
        created_at: DateTime<Utc>,
        /// Highest durable PG event sequence known before this telemetry was
        /// enqueued, encoded as a decimal string. This is an ordering
        /// watermark, not part of the telemetry identity.
        durable_before_event_seq: String,
        /// LLM call identity.
        llm_call_id: LlmCallId,
        /// Call-local telemetry sequence, assigned from 0 per call.
        telemetry_seq: u64,
        /// Typed LLM telemetry payload.
        event: LlmTelemetryEventV1,
    },
}

impl AgentStreamFrameV1 {
    /// Builds the `stream_ready` control frame; Session/Turn may be absent for
    /// an idle Agent.
    #[must_use]
    pub fn stream_ready(
        agent_id: AgentId,
        session_id: Option<SessionId>,
        turn_id: Option<TurnId>,
    ) -> Self {
        Self::Control {
            protocol_version: PROTOCOL_VERSION_V1,
            agent_id,
            session_id,
            turn_id,
            created_at: Utc::now(),
            event: ControlEventV1::StreamReady,
        }
    }

    /// Builds the connection-local `stream_reset` control frame. It is never
    /// written to Postgres or published to NATS and never carries an SSE id.
    #[must_use]
    pub fn stream_reset(agent_id: AgentId) -> Self {
        Self::Control {
            protocol_version: PROTOCOL_VERSION_V1,
            agent_id,
            session_id: None,
            turn_id: None,
            created_at: Utc::now(),
            event: ControlEventV1::StreamReset {
                reason: StreamResetReason::BufferOverflow,
            },
        }
    }

    /// Builds one durable frame from a committed ledger row and its mapped
    /// product event. The Agent identity comes from the scoped context; the
    /// row itself does not carry it.
    #[must_use]
    pub fn durable(
        agent_id: AgentId,
        row: &ScannedRow,
        event: AgentProductEventV1,
        event_version: i32,
    ) -> Self {
        Self::Durable {
            protocol_version: PROTOCOL_VERSION_V1,
            agent_id,
            session_id: row.session_id,
            turn_id: row.turn_id,
            created_at: row.created_at,
            event_seq: encode_event_seq(row.event_seq),
            event_version,
            event,
        }
    }

    /// Builds one telemetry frame when the v1 protocol has an explicit
    /// projection for the typed event. A future unknown event is omitted;
    /// it is never disguised as a known v1 event.
    #[must_use]
    pub fn telemetry(
        agent_id: AgentId,
        session_id: SessionId,
        turn_id: TurnId,
        durable_before_event_seq: u64,
        llm_call_id: LlmCallId,
        telemetry_seq: u64,
        event: &AgentTelemetryEvent,
    ) -> Option<Self> {
        Some(Self::Telemetry {
            protocol_version: PROTOCOL_VERSION_V1,
            agent_id,
            session_id,
            turn_id,
            created_at: Utc::now(),
            durable_before_event_seq: encode_event_seq(durable_before_event_seq),
            llm_call_id,
            telemetry_seq,
            event: project_telemetry_event(event)?,
        })
    }

    /// Serializes the frame for transport.
    ///
    /// # Errors
    ///
    /// Returns the serialization failure; frame shapes always serialize, so a
    /// failure indicates a bug.
    pub fn to_bytes(&self) -> Result<bytes::Bytes, serde_json::Error> {
        serde_json::to_vec(self).map(bytes::Bytes::from)
    }
}

/// Control payload of a connection-level frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ControlEventV1 {
    /// The subscription is established and buffering; the client may read the
    /// Postgres snapshot now.
    StreamReady,
    /// The server-side per-connection buffer overflowed; the connection
    /// closes after this frame and the client must cold-bootstrap without a
    /// cursor.
    StreamReset {
        /// Why the stream was reset.
        reason: StreamResetReason,
    },
}

/// Reason a stream was reset.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StreamResetReason {
    /// The bounded per-connection buffer overflowed.
    BufferOverflow,
}

/// Typed LLM telemetry payload of a telemetry frame; the call identity lives
/// on the frame, not inside the event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum LlmTelemetryEventV1 {
    /// An LLM call started.
    LlmStarted,
    /// Visible text fragment.
    TextDelta {
        /// Text fragment.
        delta: String,
    },
    /// Reasoning text fragment.
    ReasoningDelta {
        /// Reasoning fragment.
        delta: String,
    },
    /// Tool-call update fragment.
    ToolCallDelta {
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
        /// Why the call finished.
        finish_reason: String,
        /// Token usage reported by the provider, when available.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
    },
}

fn project_telemetry_event(event: &AgentTelemetryEvent) -> Option<LlmTelemetryEventV1> {
    match event {
        AgentTelemetryEvent::LlmStarted { .. } => Some(LlmTelemetryEventV1::LlmStarted),
        AgentTelemetryEvent::TextDelta { delta, .. } => Some(LlmTelemetryEventV1::TextDelta {
            delta: delta.clone(),
        }),
        AgentTelemetryEvent::ReasoningDelta { delta, .. } => {
            Some(LlmTelemetryEventV1::ReasoningDelta {
                delta: delta.clone(),
            })
        }
        AgentTelemetryEvent::ToolCallDelta {
            call_id,
            name,
            arguments_delta,
            ..
        } => Some(LlmTelemetryEventV1::ToolCallDelta {
            call_id: call_id.clone(),
            name: name.clone(),
            arguments_delta: arguments_delta.clone(),
        }),
        AgentTelemetryEvent::LlmFinished {
            finish_reason,
            usage,
            ..
        } => Some(LlmTelemetryEventV1::LlmFinished {
            finish_reason: finish_reason.clone(),
            usage: *usage,
        }),
        // `AgentTelemetryEvent` is non-exhaustive across crates. Protocol v1
        // fails closed: a future variant has no public representation until
        // an explicit projection is added.
        _ => None,
    }
}

/// Safe public projection of one durable product event.
///
/// History and SSE durable frames share this union. Internal events
/// (`ToolExecutionStarted`, hook journal rows) have no projection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentProductEventV1 {
    /// A Turn started.
    LoopStarted,
    /// A complete message was committed (user, assistant, or tool result).
    MessageAppended {
        /// Complete committed message.
        message: ChatMessage,
    },
    /// A tool call requires a human decision.
    ToolApprovalRequested {
        /// Approval request identity.
        approval_id: ApprovalId,
        /// Tool call identity.
        call_id: CallId,
        /// Provider-visible tool name.
        tool_name: ToolName,
        /// Final durable-safe arguments.
        arguments: serde_json::Value,
        /// Whether the tool observes or mutates state.
        tool_kind: ToolKind,
        /// Declared danger of the tool.
        danger_level: DangerLevel,
    },
    /// A tool approval request was decided.
    ToolApprovalResolved {
        /// Approval request identity.
        approval_id: ApprovalId,
        /// User decision.
        decision: ApprovalDecision,
    },
    /// The transcript prefix was compacted; original messages stay readable
    /// through earlier history pages.
    TranscriptCompacted {
        /// Full summary marker.
        summary: ChatMessage,
        /// Iteration whose prepare boundary compacted.
        compacted_iteration: u64,
    },
    /// One loop iteration reached its durable boundary.
    IterationCompleted {
        /// Iteration number.
        iteration: u64,
        /// Token usage of the most recent model response.
        usage: TokenUsage,
    },
    /// The Turn finished successfully.
    LoopFinished {
        /// Why the loop finished.
        finish_reason: String,
        /// Token usage of the most recent model response.
        usage: TokenUsage,
    },
    /// The Turn failed; the marker text is safe to expose.
    LoopFailed {
        /// Safe failure marker text.
        error_text: String,
        /// Token usage of the most recent model response.
        usage: TokenUsage,
    },
    /// The Turn was cancelled.
    LoopCancelled {
        /// Token usage of the most recent model response.
        usage: TokenUsage,
    },
}

/// Maps one typed durable event to its safe public projection.
///
/// Returns `Ok(None)` for internal events that must never be published:
/// `ToolExecutionStarted` and the hook invocation journal.
///
/// # Errors
///
/// Returns [`PersistedVariantError`] when a future persisted variant has no
/// explicit v1 projection. Callers must stop at that durable row rather than
/// treating it as an internal event.
pub(crate) fn product_event(
    event: &DurableAgentEvent,
) -> Result<Option<AgentProductEventV1>, PersistedVariantError> {
    match event {
        DurableAgentEvent::LoopStarted { .. } => Ok(Some(AgentProductEventV1::LoopStarted)),
        DurableAgentEvent::MessageAppended { message } => {
            Ok(Some(AgentProductEventV1::MessageAppended {
                message: message.clone(),
            }))
        }
        DurableAgentEvent::ToolApprovalRequested {
            approval_id,
            call_id,
            tool_name,
            arguments,
            tool_kind,
            danger_level,
        } => Ok(Some(AgentProductEventV1::ToolApprovalRequested {
            approval_id: *approval_id,
            call_id: call_id.clone(),
            tool_name: tool_name.clone(),
            arguments: arguments.clone(),
            tool_kind: *tool_kind,
            danger_level: *danger_level,
        })),
        DurableAgentEvent::ToolApprovalResolved {
            approval_id,
            decision,
        } => {
            let decision = match decision {
                ApprovalDecision::Approve => ApprovalDecision::Approve,
                ApprovalDecision::Reject => ApprovalDecision::Reject,
                _ => return Err(PersistedVariantError::UnsupportedApprovalDecision),
            };
            Ok(Some(AgentProductEventV1::ToolApprovalResolved {
                approval_id: *approval_id,
                decision,
            }))
        }
        DurableAgentEvent::TranscriptCompacted {
            summary,
            compacted_iteration,
            ..
        } => Ok(Some(AgentProductEventV1::TranscriptCompacted {
            summary: summary.clone(),
            compacted_iteration: *compacted_iteration,
        })),
        DurableAgentEvent::IterationCompleted { iteration, usage } => {
            Ok(Some(AgentProductEventV1::IterationCompleted {
                iteration: *iteration,
                usage: *usage,
            }))
        }
        DurableAgentEvent::LoopFinished {
            finish_reason,
            usage,
            ..
        } => Ok(Some(AgentProductEventV1::LoopFinished {
            finish_reason: finish_reason.clone(),
            usage: *usage,
        })),
        DurableAgentEvent::LoopFailed {
            error_text, usage, ..
        } => Ok(Some(AgentProductEventV1::LoopFailed {
            error_text: error_text.clone(),
            usage: *usage,
        })),
        DurableAgentEvent::LoopCancelled { usage } => {
            Ok(Some(AgentProductEventV1::LoopCancelled { usage: *usage }))
        }
        // Internal facts never become product events.
        DurableAgentEvent::ToolExecutionStarted { .. }
        | DurableAgentEvent::HookInvocationPending { .. }
        | DurableAgentEvent::HookInvocationCompleted { .. }
        | DurableAgentEvent::HookInvocationFailed { .. } => Ok(None),
        // Persisted non-exhaustive variants must never masquerade as internal
        // events: the dispatcher stops before advancing their sequence.
        _ => Err(PersistedVariantError::UnsupportedDurableProductEvent),
    }
}

/// Maps one product-visible history row into its API item shape.
///
/// # Errors
///
/// Returns `Ok(None)` when the row's event has no product projection; the
/// store's history filter already guarantees one, so `Ok(None)` indicates an
/// invariant violation. Future persisted variants retain their typed error.
pub(crate) fn history_item_event(
    item: &HistoryItem,
) -> Result<Option<AgentProductEventV1>, PersistedVariantError> {
    product_event(&item.event)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use stratum_core::{ChatMessage, ExtensionSetVersionId, HookInvocationId, HookPoint};

    use super::*;

    fn frame_json(frame: &AgentStreamFrameV1) -> Value {
        serde_json::to_value(frame).expect("frame serializes")
    }

    #[test]
    fn durable_frame_encodes_event_seq_as_decimal_string() {
        let frame = AgentStreamFrameV1::Durable {
            protocol_version: PROTOCOL_VERSION_V1,
            agent_id: AgentId::new(),
            session_id: SessionId::new(),
            turn_id: TurnId::new(),
            created_at: Utc::now(),
            event_seq: encode_event_seq(u64::MAX),
            event_version: 1,
            event: AgentProductEventV1::LoopStarted,
        };
        let json = frame_json(&frame);
        assert_eq!(json["kind"], "durable");
        assert_eq!(json["protocol_version"], 1);
        assert_eq!(json["event_seq"], json!(u64::MAX.to_string()));
        assert_eq!(json["event"], json!({ "type": "loop_started" }));
    }

    #[test]
    fn telemetry_frame_requires_a_decimal_durable_watermark_without_changing_identity() {
        let llm_call_id = LlmCallId::from("call-1");
        let event = AgentTelemetryEvent::TextDelta {
            llm_call_id: llm_call_id.clone(),
            delta: "hello".to_owned(),
        };
        let frame = AgentStreamFrameV1::telemetry(
            AgentId::new(),
            SessionId::new(),
            TurnId::new(),
            u64::MAX,
            llm_call_id.clone(),
            7,
            &event,
        )
        .expect("known telemetry projects");
        let json = frame_json(&frame);

        assert_eq!(json["kind"], "telemetry");
        assert_eq!(
            json["durable_before_event_seq"],
            json!(u64::MAX.to_string())
        );
        assert_eq!(json["llm_call_id"], json!(llm_call_id));
        assert_eq!(json["telemetry_seq"], 7);
        assert_eq!(json["event"]["type"], "text_delta");
        assert_eq!(
            serde_json::from_value::<AgentStreamFrameV1>(json.clone())
                .expect("complete telemetry frame decodes"),
            frame
        );

        let mut missing_watermark = json;
        missing_watermark
            .as_object_mut()
            .expect("frame is an object")
            .remove("durable_before_event_seq");
        serde_json::from_value::<AgentStreamFrameV1>(missing_watermark)
            .expect_err("v1 telemetry requires its durable watermark");
    }

    #[test]
    fn stream_reset_carries_no_session_or_turn_identity() {
        let frame = AgentStreamFrameV1::stream_reset(AgentId::new());
        let json = frame_json(&frame);
        assert_eq!(json["kind"], "control");
        assert_eq!(
            json["event"],
            json!({ "type": "stream_reset", "reason": "buffer_overflow" })
        );
        assert!(json.get("session_id").is_none());
        assert!(json.get("turn_id").is_none());
    }

    #[test]
    fn compaction_marker_never_exposes_upto_or_retained_pointer() {
        let event = DurableAgentEvent::TranscriptCompacted {
            upto: 7,
            summary: ChatMessage::system("summary"),
            compacted_iteration: 3,
        };
        let product = product_event(&event)
            .expect("known event projects")
            .expect("compaction is a product event");
        let json = serde_json::to_value(product).expect("product serializes");
        assert_eq!(json["type"], "transcript_compacted");
        let text = json.to_string();
        assert!(!text.contains("upto"));
        assert!(!text.contains("retained_from_event_seq"));
        assert_eq!(json["data"]["compacted_iteration"], 3);
    }

    #[test]
    fn internal_events_have_no_product_projection() {
        let internal = [
            DurableAgentEvent::ToolExecutionStarted {
                call_id: CallId::from("call-1"),
                tool_name: ToolName::from("echo"),
            },
            DurableAgentEvent::HookInvocationPending {
                invocation_id: HookInvocationId::new(),
                point: HookPoint::DecideToolCall,
                iteration: 0,
                call_id: Some(CallId::from("call-1")),
                input_digest: "ab".repeat(32).parse().expect("digest parses"),
            },
            DurableAgentEvent::HookInvocationCompleted {
                invocation_id: HookInvocationId::new(),
                decision: stratum_core::HookDecisionRecord::PrepareNextTurn(
                    stratum_core::PrepareNextTurnDecisionRecord::Continue,
                ),
            },
            DurableAgentEvent::HookInvocationFailed {
                invocation_id: HookInvocationId::new(),
                failure: stratum_core::HookFailure::TimedOut,
            },
        ];
        for event in internal {
            assert!(
                product_event(&event)
                    .expect("known internal event classifies")
                    .is_none(),
                "{} must never be published",
                event.event_type()
            );
        }
    }

    #[test]
    fn loop_started_projection_drops_the_extension_set_version() {
        let event = DurableAgentEvent::LoopStarted {
            extension_set_version_id: Some(ExtensionSetVersionId::new()),
        };
        let product = product_event(&event)
            .expect("known event projects")
            .expect("loop_started is a product event");
        let text = serde_json::to_string(&product).expect("product serializes");
        assert!(!text.contains("extension_set_version_id"));
    }

    #[test]
    fn approval_request_projection_carries_only_safe_fields() {
        let event = DurableAgentEvent::ToolApprovalRequested {
            approval_id: ApprovalId::new(),
            call_id: CallId::from("call-1"),
            tool_name: ToolName::from("echo"),
            arguments: json!({ "text": "hi" }),
            tool_kind: ToolKind::Read,
            danger_level: DangerLevel::Low,
        };
        let product = product_event(&event)
            .expect("known event projects")
            .expect("approval request is a product event");
        let json = serde_json::to_value(product).expect("product serializes");
        assert_eq!(json["type"], "tool_approval_requested");
        assert!(json["data"].get("hook_invocation_id").is_none());
    }

    #[test]
    fn approval_resolution_projects_only_explicit_v1_decisions() {
        for decision in [ApprovalDecision::Approve, ApprovalDecision::Reject] {
            let event = DurableAgentEvent::ToolApprovalResolved {
                approval_id: ApprovalId::new(),
                decision,
            };

            let product = product_event(&event)
                .expect("known decision projects")
                .expect("approval resolution is a product event");
            assert!(matches!(
                product,
                AgentProductEventV1::ToolApprovalResolved {
                    decision: projected,
                    ..
                } if projected == decision
            ));
        }
    }
}
