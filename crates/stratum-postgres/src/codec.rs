//! Durable payload codec: typed `DurableAgentEvent` ↔ ledger columns.
//!
//! The ledger stores variant-only payloads: the `{"type", "data"}` envelope is
//! split so `event_type` becomes a constrained text column and `data` becomes
//! the jsonb `payload`. Decoding rebuilds the envelope and dispatches through
//! `stratum-core`'s deserializer. Versions are explicit columns; a declared
//! but unsupported version maps to
//! [`PostgresError::RuntimeIncompatible`], while a supported version that
//! fails to decode or violates an invariant maps to
//! [`PostgresError::DurableStateCorrupt`]. There is no upcasting.

use serde::Deserialize;
use serde_json::{Map, Value, json};
use stratum_core::{
    ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel, DurableAgentEvent,
    HookInvocationId, ToolKind, ToolName, TurnRuntimeSnapshot,
};

use crate::error::{PostgresError, VersionedKind};

/// Only supported durable event payload version.
pub(crate) const EVENT_VERSION_V1: i32 = 1;
/// Only supported runtime snapshot version.
pub(crate) const RUNTIME_SNAPSHOT_VERSION_V1: i32 = 1;
/// Only supported resolved-definition schema version.
pub(crate) const DEFINITION_SCHEMA_VERSION_V1: i32 = 1;

/// Closed set of known durable event types; mirrors the migration CHECK.
pub(crate) const KNOWN_EVENT_TYPES: [&str; 13] = [
    "loop_started",
    "message_appended",
    "tool_approval_requested",
    "tool_approval_resolved",
    "tool_execution_started",
    "hook_invocation_pending",
    "hook_invocation_completed",
    "hook_invocation_failed",
    "transcript_compacted",
    "iteration_completed",
    "loop_finished",
    "loop_failed",
    "loop_cancelled",
];

pub(crate) const TYPE_TRANSCRIPT_COMPACTED: &str = "transcript_compacted";

/// Product-visible event types covered by the filtered history index.
pub(crate) const HISTORY_EVENT_TYPES: [&str; 4] = [
    "message_appended",
    "transcript_compacted",
    "loop_failed",
    "loop_cancelled",
];

/// Usage-carrying event types scanned for `AgentView.latest_usage`.
pub(crate) const USAGE_EVENT_TYPES: [&str; 4] = [
    "iteration_completed",
    "loop_finished",
    "loop_failed",
    "loop_cancelled",
];

/// Ledger column encoding of one typed event.
#[derive(Debug)]
pub(crate) struct EncodedEvent {
    pub(crate) event_type: &'static str,
    pub(crate) event_version: i32,
    pub(crate) payload: Value,
}

/// Companion facts needed to materialize a typed `TranscriptCompacted`.
pub(crate) struct CompanionFacts {
    pub(crate) upto: u64,
    pub(crate) compacted_iteration: u64,
    pub(crate) summary: Value,
}

/// Encodes a typed event into ledger columns.
///
/// `approval_hook_invocation_id` is required for `ToolApprovalRequested` (the
/// kernel event does not carry it; the durable payload must, per the approval
/// identity contract) and rejected for every other variant. The
/// `TranscriptCompacted` payload is always the empty object; its facts live in
/// the companion row written in the same transaction.
pub(crate) fn encode_event(
    event: &DurableAgentEvent,
    approval_hook_invocation_id: Option<HookInvocationId>,
) -> Result<EncodedEvent, PostgresError> {
    let event_type = event.event_type();
    let full = serde_json::to_value(event)
        .map_err(|source| PostgresError::EventEncode { event_type, source })?;
    let mut payload = match full {
        Value::Object(mut envelope) => envelope
            .remove("data")
            .unwrap_or_else(|| Value::Object(Map::new())),
        _ => {
            return Err(PostgresError::corrupt_invariant(
                "typed event did not serialize to an object envelope",
            ));
        }
    };

    match event {
        DurableAgentEvent::TranscriptCompacted { .. } => {
            payload = json!({});
        }
        DurableAgentEvent::ToolApprovalRequested { .. } => {
            let hook_invocation_id =
                approval_hook_invocation_id.ok_or(PostgresError::InvalidCommand(
                    "tool_approval_requested requires approval_hook_invocation_id",
                ))?;
            let Value::Object(ref mut data) = payload else {
                return Err(PostgresError::corrupt_invariant(
                    "approval request payload is not an object",
                ));
            };
            data.insert("hook_invocation_id".to_owned(), json!(hook_invocation_id));
        }
        _ => {
            if approval_hook_invocation_id.is_some() {
                return Err(PostgresError::InvalidCommand(
                    "approval_hook_invocation_id is only valid for tool_approval_requested",
                ));
            }
        }
    }

    Ok(EncodedEvent {
        event_type,
        event_version: EVENT_VERSION_V1,
        payload,
    })
}

/// Guards one ledger row selected by a specialized derivation query before its
/// payload decodes as v1. Specialized queries (approval/hook derivation)
/// select payloads directly, so each row's declared `event_version` must be
/// checked here: an unsupported version fails closed as runtime-incompatible
/// instead of being silently read as v1.
pub(crate) fn ensure_supported_event_version(event_version: i32) -> Result<(), PostgresError> {
    if event_version != EVENT_VERSION_V1 {
        return Err(PostgresError::RuntimeIncompatible {
            kind: VersionedKind::EventPayload,
            version: event_version,
        });
    }
    Ok(())
}

/// Decodes one ledger row into a typed event with strict v1 rules.
///
/// `companion` must be `Some` exactly when `event_type` is
/// `transcript_compacted`; a missing companion is truth corruption, and a
/// non-empty compacted payload violates the single-copy invariant.
pub(crate) fn decode_event(
    event_type: &str,
    event_version: i32,
    payload: Value,
    companion: Option<CompanionFacts>,
) -> Result<DurableAgentEvent, PostgresError> {
    ensure_supported_event_version(event_version)?;
    if !KNOWN_EVENT_TYPES.contains(&event_type) {
        return Err(PostgresError::corrupt_invariant(
            "durable event_type outside the known closed set",
        ));
    }

    let data = if event_type == TYPE_TRANSCRIPT_COMPACTED {
        if payload != json!({}) {
            return Err(PostgresError::corrupt_invariant(
                "transcript_compacted durable payload is not the empty object",
            ));
        }
        let companion = companion.ok_or(PostgresError::corrupt_invariant(
            "transcript_compacted discriminator lacks its companion row",
        ))?;
        json!({
            "upto": companion.upto,
            "summary": companion.summary,
            "compacted_iteration": companion.compacted_iteration,
        })
    } else {
        payload
    };

    serde_json::from_value(json!({ "type": event_type, "data": data }))
        .map_err(|source| PostgresError::corrupt("durable event payload failed v1 decode", source))
}

/// Encodes a v1 runtime snapshot for the `LoopStarted` envelope columns.
pub(crate) fn encode_runtime_snapshot(
    snapshot: &TurnRuntimeSnapshot,
) -> Result<Value, PostgresError> {
    serde_json::to_value(snapshot).map_err(|source| PostgresError::EventEncode {
        event_type: "runtime_snapshot",
        source,
    })
}

/// Decodes the runtime snapshot of a `LoopStarted` row with strict v1 rules.
pub(crate) fn decode_runtime_snapshot(
    version: i32,
    snapshot: Value,
) -> Result<TurnRuntimeSnapshot, PostgresError> {
    if version != RUNTIME_SNAPSHOT_VERSION_V1 {
        return Err(PostgresError::RuntimeIncompatible {
            kind: VersionedKind::RuntimeSnapshot,
            version,
        });
    }
    serde_json::from_value(snapshot)
        .map_err(|source| PostgresError::corrupt("runtime snapshot failed v1 decode", source))
}

/// Postgres SQLSTATE of a unique-constraint violation.
const UNIQUE_VIOLATION_SQLSTATE: &str = "23505";

/// Extracts `(sqlstate, constraint name)` from a database error.
pub(crate) fn database_violation(error: &sqlx::Error) -> Option<(String, Option<String>)> {
    let sqlx::Error::Database(database_error) = error else {
        return None;
    };
    let code = database_error.code().map(|code| code.into_owned())?;
    let constraint = database_error.constraint().map(str::to_owned);
    Some((code, constraint))
}

/// Whether the error is a unique violation on `constraint`.
pub(crate) fn is_unique_violation_on(error: &sqlx::Error, constraint: &str) -> bool {
    match database_violation(error) {
        Some((code, name)) => {
            code == UNIQUE_VIOLATION_SQLSTATE && name.as_deref() == Some(constraint)
        }
        None => false,
    }
}

/// Decodes a `ModelConfig` jsonb column.
pub(crate) fn decode_model_config(
    value: Value,
    context: &'static str,
) -> Result<stratum_core::ModelConfig, PostgresError> {
    serde_json::from_value(value).map_err(|source| PostgresError::corrupt(context, source))
}

/// Decodes an optional `ModelConfig` jsonb column.
pub(crate) fn decode_optional_model_config(
    value: Option<Value>,
    context: &'static str,
) -> Result<Option<stratum_core::ModelConfig>, PostgresError> {
    value
        .map(|value| decode_model_config(value, context))
        .transpose()
}

/// Strict decode of a compaction companion `summary` column.
pub(crate) fn decode_summary(value: Value) -> Result<ChatMessage, PostgresError> {
    serde_json::from_value(value)
        .map_err(|source| PostgresError::corrupt("compaction summary failed v1 decode", source))
}

/// Wire shape of a `tool_approval_requested` payload (core fields plus the
/// store-injected `hook_invocation_id`).
#[derive(Debug, Deserialize)]
pub(crate) struct RequestedApprovalPayload {
    pub(crate) approval_id: ApprovalId,
    pub(crate) hook_invocation_id: HookInvocationId,
    pub(crate) call_id: CallId,
    pub(crate) tool_name: ToolName,
    pub(crate) arguments: Value,
    pub(crate) tool_kind: ToolKind,
    pub(crate) danger_level: DangerLevel,
}

impl RequestedApprovalPayload {
    /// Strictly decodes one selected `tool_approval_requested` row; the row's
    /// declared version must be supported before any v1 field is read.
    pub(crate) fn decode(event_version: i32, payload: Value) -> Result<Self, PostgresError> {
        ensure_supported_event_version(event_version)?;
        serde_json::from_value(payload).map_err(|source| {
            PostgresError::corrupt("approval request payload failed v1 decode", source)
        })
    }
}

/// Wire shape of a `tool_approval_resolved` payload.
#[derive(Debug, Deserialize)]
pub(crate) struct ResolvedApprovalPayload {
    // Kept for the explicit wire shape; rows are always fetched filtered by
    // approval id, so the value is never read back.
    #[allow(dead_code)]
    pub(crate) approval_id: ApprovalId,
    pub(crate) decision: ApprovalDecision,
}

impl ResolvedApprovalPayload {
    /// Strictly decodes one selected `tool_approval_resolved` row; the row's
    /// declared version must be supported before any v1 field is read.
    pub(crate) fn decode(event_version: i32, payload: Value) -> Result<Self, PostgresError> {
        ensure_supported_event_version(event_version)?;
        serde_json::from_value(payload).map_err(|source| {
            PostgresError::corrupt("approval resolution payload failed v1 decode", source)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, json};
    use stratum_core::{
        AgentVersionId, ChatMessage, ExtensionSetVersionId, HookHandlerVersionId, ModelConfig,
        ModelId, SkillSetVersionId, TokenUsage,
    };

    use super::*;

    fn usage() -> TokenUsage {
        TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            total_tokens: 3,
        }
    }

    fn sample_events() -> Vec<DurableAgentEvent> {
        vec![
            DurableAgentEvent::LoopStarted {
                extension_set_version_id: Some(ExtensionSetVersionId::new()),
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("hello 中 🚀"),
            },
            DurableAgentEvent::ToolExecutionStarted {
                call_id: CallId::from("call-1"),
                tool_name: ToolName::from("echo"),
            },
            DurableAgentEvent::IterationCompleted {
                iteration: 1,
                usage: usage(),
            },
            DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage: usage(),
            },
            DurableAgentEvent::LoopFailed {
                error_text: "provider unavailable".to_owned(),
                usage: usage(),
            },
            DurableAgentEvent::LoopCancelled { usage: usage() },
        ]
    }

    #[test]
    fn encode_stores_variant_only_payload_and_round_trips() {
        for event in sample_events() {
            let encoded = encode_event(&event, None).expect("event encodes");
            assert_eq!(encoded.event_type, event.event_type());
            assert_eq!(encoded.event_version, EVENT_VERSION_V1);
            assert!(
                encoded.payload.get("type").is_none(),
                "payload must be variant-only"
            );

            let decoded = decode_event(
                encoded.event_type,
                encoded.event_version,
                encoded.payload,
                None,
            )
            .expect("event decodes");
            assert_eq!(decoded, event);
        }
    }

    #[test]
    fn approval_requested_payload_carries_injected_hook_invocation_id() {
        let hook_invocation_id = HookInvocationId::new();
        let event = DurableAgentEvent::ToolApprovalRequested {
            approval_id: ApprovalId::new(),
            call_id: CallId::from("call-1"),
            tool_name: ToolName::from("echo"),
            arguments: json!({ "text": "hello" }),
            tool_kind: ToolKind::Read,
            danger_level: DangerLevel::Low,
        };

        let encoded = encode_event(&event, Some(hook_invocation_id)).expect("event encodes");
        assert_eq!(
            encoded.payload["hook_invocation_id"],
            json!(hook_invocation_id)
        );
        // The core typed event ignores the store-injected field on decode.
        let decoded = decode_event(
            encoded.event_type,
            encoded.event_version,
            encoded.payload.clone(),
            None,
        )
        .expect("event decodes");
        assert_eq!(decoded, event);

        let requested = RequestedApprovalPayload::decode(encoded.event_version, encoded.payload)
            .expect("payload decodes");
        assert_eq!(requested.hook_invocation_id, hook_invocation_id);
    }

    #[test]
    fn approval_requested_without_invocation_id_is_rejected() {
        let event = DurableAgentEvent::ToolApprovalRequested {
            approval_id: ApprovalId::new(),
            call_id: CallId::from("call-1"),
            tool_name: ToolName::from("echo"),
            arguments: json!({}),
            tool_kind: ToolKind::Read,
            danger_level: DangerLevel::Low,
        };

        let error = encode_event(&event, None).expect_err("missing invocation id fails");
        assert!(matches!(error, PostgresError::InvalidCommand(_)));

        let other = DurableAgentEvent::LoopCancelled { usage: usage() };
        let error = encode_event(&other, Some(HookInvocationId::new()))
            .expect_err("unexpected invocation id fails");
        assert!(matches!(error, PostgresError::InvalidCommand(_)));
    }

    #[test]
    fn transcript_compacted_encodes_empty_payload_and_materializes_from_companion() {
        let event = DurableAgentEvent::TranscriptCompacted {
            upto: 4,
            summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary so far"),
            compacted_iteration: 2,
        };

        let encoded = encode_event(&event, None).expect("event encodes");
        assert_eq!(encoded.payload, json!({}));

        let companion = CompanionFacts {
            upto: 4,
            compacted_iteration: 2,
            summary: json!({
                "role": "system",
                "content": { "type": "text", "data": "[stratum:transcript-compacted]\nsummary so far" }
            }),
        };
        let decoded = decode_event(
            encoded.event_type,
            encoded.event_version,
            encoded.payload,
            Some(companion),
        )
        .expect("event materializes");
        assert_eq!(decoded, event);
    }

    #[test]
    fn transcript_compacted_requires_empty_payload_and_companion() {
        let with_payload = decode_event(
            TYPE_TRANSCRIPT_COMPACTED,
            EVENT_VERSION_V1,
            json!({ "upto": 3 }),
            None,
        )
        .expect_err("non-empty payload is corrupt");
        assert!(matches!(error_kind(&with_payload), "corrupt"));

        let without_companion =
            decode_event(TYPE_TRANSCRIPT_COMPACTED, EVENT_VERSION_V1, json!({}), None)
                .expect_err("missing companion is corrupt");
        assert!(matches!(error_kind(&without_companion), "corrupt"));
    }

    #[test]
    fn unsupported_event_version_maps_to_runtime_incompatible() {
        let error = decode_event("message_appended", 2, json!({}), None)
            .expect_err("newer version is incompatible");
        assert!(matches!(
            error,
            PostgresError::RuntimeIncompatible {
                kind: VersionedKind::EventPayload,
                version: 2
            }
        ));
    }

    #[test]
    fn malformed_supported_version_maps_to_durable_state_corrupt() {
        let error = decode_event("message_appended", EVENT_VERSION_V1, json!({}), None)
            .expect_err("missing variant field is corrupt");
        assert!(matches!(error, PostgresError::DurableStateCorrupt { .. }));
        assert!(
            std::error::Error::source(&error).is_some(),
            "decode failures keep the source chain"
        );
    }

    #[test]
    fn unknown_event_type_at_known_version_maps_to_durable_state_corrupt() {
        let error = decode_event("turn_rewound", EVENT_VERSION_V1, json!({}), None)
            .expect_err("unknown type is corrupt");
        assert!(matches!(error, PostgresError::DurableStateCorrupt { .. }));
    }

    #[test]
    fn specialized_approval_decodes_reject_unsupported_versions() {
        let requested_payload = json!({
            "approval_id": ApprovalId::new(),
            "hook_invocation_id": HookInvocationId::new(),
            "call_id": "call-1",
            "tool_name": "echo",
            "arguments": {},
            "tool_kind": "read",
            "danger_level": "low",
        });
        let error = RequestedApprovalPayload::decode(2, requested_payload.clone())
            .expect_err("newer request version is incompatible");
        assert!(matches!(
            error,
            PostgresError::RuntimeIncompatible {
                kind: VersionedKind::EventPayload,
                version: 2
            }
        ));

        let resolved_payload = json!({
            "approval_id": ApprovalId::new(),
            "decision": "approve",
        });
        let error = ResolvedApprovalPayload::decode(2, resolved_payload.clone())
            .expect_err("newer resolution version is incompatible");
        assert!(matches!(
            error,
            PostgresError::RuntimeIncompatible {
                kind: VersionedKind::EventPayload,
                version: 2
            }
        ));

        // A malformed payload of a supported version stays corrupt.
        let error = RequestedApprovalPayload::decode(EVENT_VERSION_V1, json!({}))
            .expect_err("malformed v1 request is corrupt");
        assert!(matches!(error, PostgresError::DurableStateCorrupt { .. }));
        let error = ResolvedApprovalPayload::decode(EVENT_VERSION_V1, json!({}))
            .expect_err("malformed v1 resolution is corrupt");
        assert!(matches!(error, PostgresError::DurableStateCorrupt { .. }));

        // The supported shapes still decode.
        assert!(
            RequestedApprovalPayload::decode(EVENT_VERSION_V1, requested_payload).is_ok(),
            "v1 request decodes"
        );
        assert!(
            ResolvedApprovalPayload::decode(EVENT_VERSION_V1, resolved_payload).is_ok(),
            "v1 resolution decodes"
        );
    }

    #[test]
    fn event_version_guard_accepts_only_v1() {
        assert!(ensure_supported_event_version(EVENT_VERSION_V1).is_ok());
        for version in [0, 2, 7] {
            let error = ensure_supported_event_version(version)
                .expect_err("unsupported versions fail closed");
            assert!(matches!(
                error,
                PostgresError::RuntimeIncompatible {
                    kind: VersionedKind::EventPayload,
                    ..
                }
            ));
        }
    }

    #[test]
    fn runtime_snapshot_strict_decode_distinguishes_incompatible_and_corrupt() {
        let snapshot = TurnRuntimeSnapshot::new(
            AgentVersionId::new(),
            ModelConfig::new(
                ModelId::new("openai", "test-model").expect("model id is valid"),
                Map::new(),
            ),
            "a".repeat(64).parse().expect("fingerprint is valid"),
            SkillSetVersionId::new(),
            ExtensionSetVersionId::new(),
            vec![HookHandlerVersionId::new()],
        );
        let encoded = encode_runtime_snapshot(&snapshot).expect("snapshot encodes");
        let decoded =
            decode_runtime_snapshot(RUNTIME_SNAPSHOT_VERSION_V1, encoded).expect("decodes");
        assert_eq!(decoded, snapshot);

        let incompatible = decode_runtime_snapshot(2, json!({})).expect_err("v2 is unsupported");
        assert!(matches!(
            incompatible,
            PostgresError::RuntimeIncompatible {
                kind: VersionedKind::RuntimeSnapshot,
                version: 2
            }
        ));

        let corrupt = decode_runtime_snapshot(RUNTIME_SNAPSHOT_VERSION_V1, json!({ "model": 1 }))
            .expect_err("malformed v1 is corrupt");
        assert!(matches!(corrupt, PostgresError::DurableStateCorrupt { .. }));
    }

    fn error_kind(error: &PostgresError) -> &'static str {
        match error {
            PostgresError::DurableStateCorrupt { .. } => "corrupt",
            _ => "other",
        }
    }
}

#[cfg(test)]
mod violation_mapping_tests {
    use std::borrow::Cow;

    use super::is_unique_violation_on;

    #[derive(Debug)]
    struct MockDatabaseError {
        code: &'static str,
        constraint: Option<&'static str>,
    }

    impl std::fmt::Display for MockDatabaseError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "mock database error {}", self.code)
        }
    }

    impl std::error::Error for MockDatabaseError {}

    impl sqlx::error::DatabaseError for MockDatabaseError {
        fn message(&self) -> &str {
            "mock database error"
        }

        fn code(&self) -> Option<Cow<'_, str>> {
            Some(Cow::Borrowed(self.code))
        }

        fn constraint(&self) -> Option<&str> {
            self.constraint
        }

        fn as_error(&self) -> &(dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn as_error_mut(&mut self) -> &mut (dyn std::error::Error + Send + Sync + 'static) {
            self
        }

        fn into_error(self: Box<Self>) -> Box<dyn std::error::Error + Send + Sync + 'static> {
            self
        }

        fn kind(&self) -> sqlx::error::ErrorKind {
            sqlx::error::ErrorKind::Other
        }
    }

    #[test]
    fn unique_violation_matches_only_the_named_constraint() {
        let error = sqlx::Error::Database(Box::new(MockDatabaseError {
            code: "23505",
            constraint: Some("agent_state_running_session_unique"),
        }));
        assert!(is_unique_violation_on(
            &error,
            "agent_state_running_session_unique"
        ));
        assert!(!is_unique_violation_on(
            &error,
            "agents_idempotency_key_key"
        ));
    }

    #[test]
    fn other_sqlstates_never_match_unique_constraints() {
        let error = sqlx::Error::Database(Box::new(MockDatabaseError {
            code: "40001",
            constraint: Some("agent_state_running_session_unique"),
        }));
        assert!(!is_unique_violation_on(
            &error,
            "agent_state_running_session_unique"
        ));

        let non_database = sqlx::Error::RowNotFound;
        assert!(!is_unique_violation_on(
            &non_database,
            "agent_state_running_session_unique"
        ));
    }
}
