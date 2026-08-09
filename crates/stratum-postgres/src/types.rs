//! Narrow domain types for the Postgres execution-storage API.
//!
//! Agent-wide event sequences are `u64` inside this crate and `bigint` in the
//! schema; at the API boundary they are encoded as decimal strings so
//! JavaScript number precision never changes identity ([`encode_event_seq`],
//! [`parse_event_seq`]).

use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde_json::Value;
use stratum_core::{
    AgentId, AgentVersionId, ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel,
    DurableAgentEvent, HookInvocationId, HookPoint, ModelConfig, SessionId, TokenUsage, ToolKind,
    ToolName, TurnId, TurnRuntimeSnapshot,
};
use uuid::Uuid;

use crate::error::PostgresError;

/// Largest event sequence representable in the `bigint` column.
pub const EVENT_SEQ_MAX: u64 = i64::MAX.unsigned_abs();

/// Converts a non-negative domain integer into the schema's `bigint`
/// representation without truncation.
pub(crate) fn u64_to_bigint(
    value: u64,
    overflow_context: &'static str,
) -> Result<i64, PostgresError> {
    i64::try_from(value).map_err(|_| PostgresError::InvalidCommand(overflow_context))
}

/// Default history page size.
pub const HISTORY_DEFAULT_LIMIT: u32 = 50;
/// Largest history page size accepted by the store.
pub const HISTORY_MAX_LIMIT: u32 = 256;
/// Soft history page budget in bytes (1 MiB); the first oversized item of a
/// page is still returned whole.
pub const HISTORY_SOFT_PAGE_BUDGET_BYTES: usize = 1024 * 1024;

/// Encodes an agent-wide event sequence as a decimal string for API frames.
#[must_use]
pub fn encode_event_seq(event_seq: u64) -> String {
    event_seq.to_string()
}

/// Parses a decimal-string event sequence from an API frame.
///
/// Returns `None` for empty, signed, non-decimal, or overflowing input; the
/// caller maps `None` to its own invalid-cursor error.
#[must_use]
pub fn parse_event_seq(value: &str) -> Option<u64> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    value.parse().ok()
}

/// Durable status of an Agent's current or most recent Turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AgentStatus {
    /// Created but never started a Turn.
    Idle,
    /// Current Turn is durably running (hosted or unhosted).
    Running,
    /// Most recent Turn finished successfully.
    Finished,
    /// Most recent Turn failed.
    Failed,
    /// Most recent Turn was cancelled.
    Cancelled,
}

impl AgentStatus {
    /// Stable database text representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::Finished => "finished",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether this status describes a terminal recent Turn.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Finished | Self::Failed | Self::Cancelled)
    }
}

impl std::fmt::Display for AgentStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AgentStatus {
    type Err = PostgresError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "idle" => Ok(Self::Idle),
            "running" => Ok(Self::Running),
            "finished" => Ok(Self::Finished),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(PostgresError::corrupt_invariant(
                "agent_state.status outside the closed set",
            )),
        }
    }
}

/// Stored create request behind one idempotency key.
///
/// Read before any template access: an idempotent replay must answer from
/// this record alone, without re-reading the template catalog.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CreateKeyLookup {
    /// Existing Agent bound to the key.
    pub agent_id: AgentId,
    /// Source template name recorded at creation.
    pub source_template_name: String,
    /// Client creation-time model override recorded at creation.
    pub creation_model_override: Option<ModelConfig>,
}

/// Exact hook invocation address used to find the one open journaled
/// invocation (`HookInvocationPending` without a matching Completed/Failed).
///
/// The kernel journals at most one open invocation per address, so the lookup
/// identifies it uniquely.
#[derive(Debug, Clone)]
pub struct HookInvocationLookup {
    /// Owning Agent.
    pub agent_id: AgentId,
    /// Exact current Turn.
    pub turn_id: TurnId,
    /// Decision point being invoked.
    pub point: HookPoint,
    /// Zero-based model iteration the invocation belongs to.
    pub iteration: u64,
    /// Tool call identity for tool hooks; `None` for context hooks.
    pub call_id: Option<CallId>,
}

/// Create command: one immutable Agent plus its idle state.
///
/// The caller has already resolved and validated the template, model and tool
/// preflight; the store persists exactly what it is given.
#[derive(Debug, Clone)]
pub struct CreateAgent {
    /// Server-generated Agent identity.
    pub agent_id: AgentId,
    /// Immutable one-to-one Agent version identity.
    pub agent_version_id: AgentVersionId,
    /// Permanent client idempotency key.
    pub idempotency_key: Uuid,
    /// Source template name used for request equivalence.
    pub source_template_name: String,
    /// Client creation-time model override, used for request equivalence.
    pub creation_model_override: Option<ModelConfig>,
    /// Immutable resolved definition snapshot (definition schema v1).
    pub resolved_definition: Value,
    /// Mutable default model for the next Turn, initialized from the
    /// creation-time effective model.
    pub default_model_config: ModelConfig,
}

/// Outcome of an idempotent create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CreateAgentOutcome {
    /// A new Agent and idle state were committed.
    Created {
        /// New Agent identity.
        agent_id: AgentId,
    },
    /// The idempotency key replayed an equivalent earlier request.
    Replay {
        /// Original Agent identity.
        agent_id: AgentId,
    },
}

/// Admission command: atomically install a new current Turn.
#[derive(Debug, Clone)]
pub struct BeginTurn {
    /// Agent being admitted.
    pub agent_id: AgentId,
    /// CAS expectation: `None` for the first Turn, otherwise the exact recent
    /// Turn the caller observed.
    pub expected_current_turn_id: Option<TurnId>,
    /// Server-generated identity of the new Turn.
    pub turn_id: TurnId,
    /// Session to bind on the first Turn, or the already-bound Session.
    pub session_id: SessionId,
    /// Exact runtime identity pinned to the Turn's `LoopStarted` row.
    pub snapshot: TurnRuntimeSnapshot,
}

/// Centralized append command used by every durable writer.
#[derive(Debug, Clone)]
pub struct AppendEvent {
    /// Owning Agent.
    pub agent_id: AgentId,
    /// Exact bound Session expectation.
    pub session_id: SessionId,
    /// Exact current Turn expectation.
    pub turn_id: TurnId,
    /// Typed event to append; must not be `LoopStarted` (use
    /// [`crate::PostgresBackend::begin_turn`]).
    pub event: DurableAgentEvent,
    /// Exact hook invocation identity; required for `ToolApprovalRequested`
    /// and forbidden otherwise.
    pub approval_hook_invocation_id: Option<HookInvocationId>,
    /// Full replacement for the mutable default model; only meaningful on the
    /// Turn's first user `MessageAppended` and applied only when the value
    /// differs. Forbidden on other events.
    pub default_model_update: Option<ModelConfig>,
    /// Companion facts; required for `TranscriptCompacted` and forbidden
    /// otherwise.
    pub compaction: Option<CompactionInput>,
}

/// Companion facts committed atomically with a `TranscriptCompacted` row.
#[derive(Debug, Clone)]
pub struct CompactionInput {
    /// Iteration whose prepare boundary executed the compaction.
    pub compacted_iteration: u64,
    /// Exclusive end index of the replaced committed-context prefix.
    pub upto: u64,
    /// Agent-wide sequence of the first retained `MessageAppended`; must
    /// address a real earlier message of the same Agent.
    pub retained_from_event_seq: u64,
    /// Kernel-owned summary marker replacing the compacted prefix.
    pub summary: ChatMessage,
}

/// Resolve command for one durable approval request.
#[derive(Debug, Clone, Copy)]
pub struct ResolveApproval {
    /// Owning Agent (path identity).
    pub agent_id: AgentId,
    /// Approval request being decided.
    pub approval_id: ApprovalId,
    /// Exact Turn the caller believes owns the approval.
    pub turn_id: TurnId,
    /// User decision.
    pub decision: ApprovalDecision,
}

/// Outcome of a resolve command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ResolveApprovalOutcome {
    /// The first decision was committed.
    Resolved {
        /// Commit receipt of the `ToolApprovalResolved` row.
        receipt: CommitReceipt,
    },
    /// An identical earlier decision already exists; nothing was appended.
    AlreadyResolvedSame,
}

/// Commit receipt returned after a durable transaction commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CommitReceipt {
    /// Agent-wide sequence assigned to the committed row.
    pub event_seq: u64,
}

/// Thin durable Agent state, as persisted in `agent_state`.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentStateView {
    /// Durable status.
    pub status: AgentStatus,
    /// Bound Session (`None` only while idle).
    pub session_id: Option<SessionId>,
    /// Current or most recent Turn.
    pub current_turn_id: Option<TurnId>,
    /// Mutable default model for the next Turn.
    pub default_model_config: ModelConfig,
    /// Agent-wide high-water sequence.
    pub last_event_seq: u64,
}

/// Cold read of one Agent: `agents` + `agent_state` plus ledger-derived
/// facts, all captured in one MVCC snapshot.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct AgentView {
    /// Agent identity.
    pub agent_id: AgentId,
    /// Immutable Agent version identity.
    pub agent_version_id: AgentVersionId,
    /// Source template name recorded at creation.
    pub source_template_name: String,
    /// Client creation-time model override.
    pub creation_model_override: Option<ModelConfig>,
    /// Definition schema version of `resolved_definition`.
    pub definition_schema_version: i32,
    /// Immutable resolved definition snapshot.
    pub resolved_definition: Value,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Durable status.
    pub status: AgentStatus,
    /// Bound Session (`None` only while idle).
    pub session_id: Option<SessionId>,
    /// Current or most recent Turn.
    pub current_turn_id: Option<TurnId>,
    /// Mutable default model for the next Turn.
    pub default_model_config: ModelConfig,
    /// Snapshot barrier; always equals `agent_state.last_event_seq` in the
    /// same MVCC snapshot.
    pub snapshot_event_seq: u64,
    /// Latest assistant `MessageAppended` sequence at the snapshot barrier,
    /// or zero when no assistant message exists. This is the durable floor
    /// used to reject an older volatile telemetry tail after cold recovery.
    pub telemetry_floor_event_seq: u64,
    /// Undecided approvals of the current Turn within the barrier, ordered by
    /// requested sequence; empty when the Turn is terminal.
    pub pending_approvals: Vec<PendingApproval>,
    /// Usage of the most recent usage-carrying durable event of the current
    /// Turn within the barrier.
    pub latest_usage: Option<TokenUsage>,
}

/// One undecided approval derived from the durable ledger.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PendingApproval {
    /// Sequence of the `ToolApprovalRequested` row.
    pub requested_event_seq: u64,
    /// Approval identity.
    pub approval_id: ApprovalId,
    /// Exact hook invocation awaiting the decision.
    pub hook_invocation_id: HookInvocationId,
    /// Tool call identity.
    pub call_id: CallId,
    /// Provider-visible tool name.
    pub tool_name: ToolName,
    /// Final durable-safe arguments.
    pub arguments: Value,
    /// Whether the tool observes or mutates state.
    pub tool_kind: ToolKind,
    /// Declared danger of the tool.
    pub danger_level: DangerLevel,
}

/// History page query over product-visible durable rows.
#[derive(Debug, Clone, Copy)]
pub struct HistoryQuery {
    /// Owning Agent.
    pub agent_id: AgentId,
    /// Inclusive barrier; rows above it are never returned.
    pub through_event_seq: u64,
    /// Exclusive upper cursor from a previous page.
    pub before_event_seq: Option<u64>,
    /// Page size; clamped into `1..=HISTORY_MAX_LIMIT`.
    pub limit: u32,
}

/// One product-visible history item.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HistoryItem {
    /// Agent-wide sequence of the durable row.
    pub event_seq: u64,
    /// Durable payload version of the row.
    pub event_version: i32,
    /// Session owning the row.
    pub session_id: SessionId,
    /// Turn owning the row.
    pub turn_id: TurnId,
    /// Materialized typed event (`TranscriptCompacted` is joined with its
    /// companion).
    pub event: DurableAgentEvent,
    /// Commit timestamp.
    pub created_at: DateTime<Utc>,
}

/// One ascending history page.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct HistoryPage {
    /// Items in ascending event order.
    pub items: Vec<HistoryItem>,
    /// Exclusive cursor for the next (older) page: the smallest returned
    /// sequence.
    pub next_before_event_seq: Option<u64>,
    /// Whether older product-visible rows exist beyond this page.
    pub has_more: bool,
}

/// The single `LoopStarted` row of one Turn, with its runtime snapshot.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoopStartedRecord {
    /// Sequence of the `LoopStarted` row; the Turn's historical base is
    /// always `event_seq - 1`.
    pub event_seq: u64,
    /// Session bound to the Turn.
    pub session_id: SessionId,
    /// Turn identity.
    pub turn_id: TurnId,
    /// Declared runtime snapshot version.
    pub snapshot_version: i32,
    /// Decoded v1 runtime snapshot.
    pub snapshot: TurnRuntimeSnapshot,
    /// Commit timestamp.
    pub created_at: DateTime<Utc>,
}

/// One decoded durable ledger row.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DurableEventRow {
    /// Agent-wide sequence.
    pub event_seq: u64,
    /// Durable payload version of the row.
    pub event_version: i32,
    /// Session owning the row.
    pub session_id: SessionId,
    /// Turn owning the row.
    pub turn_id: TurnId,
    /// Materialized typed event (`TranscriptCompacted` is joined with its
    /// companion).
    pub event: DurableAgentEvent,
    /// Commit timestamp.
    pub created_at: DateTime<Utc>,
}

/// Exact-Turn resume slice request.
#[derive(Debug, Clone, Copy)]
pub struct ResumeSliceQuery {
    /// Owning Agent.
    pub agent_id: AgentId,
    /// Bound Session expectation.
    pub session_id: SessionId,
    /// Exact current Turn.
    pub turn_id: TurnId,
    /// Historical base: `LoopStarted.event_seq - 1`.
    pub base_event_seq: u64,
    /// Fixed barrier captured as `agent_state.last_event_seq`.
    pub through_event_seq: u64,
}

/// Durable companion of one `TranscriptCompacted` discriminator.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct TranscriptCompaction {
    /// Owning Agent.
    pub agent_id: AgentId,
    /// Sequence shared with the discriminator row.
    pub event_seq: u64,
    /// Turn that committed the compaction.
    pub turn_id: TurnId,
    /// Iteration whose prepare boundary executed the compaction.
    pub compacted_iteration: u64,
    /// Exclusive end index of the replaced committed-context prefix.
    pub upto: u64,
    /// Agent-wide sequence of the first retained `MessageAppended`.
    pub retained_from_event_seq: u64,
    /// Kernel-owned summary marker.
    pub summary: ChatMessage,
    /// Commit timestamp.
    pub created_at: DateTime<Utc>,
}

/// Approval lookup key for the decide-hook Handler.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub enum ApprovalLookup {
    /// Look up by approval identity.
    ByApprovalId(ApprovalId),
    /// Look up by exact hook invocation identity.
    ByHookInvocationId(HookInvocationId),
}

/// Ledger facts about one approval request.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ApprovalFacts {
    /// Sequence of the `ToolApprovalRequested` row.
    pub requested_event_seq: u64,
    /// Approval identity.
    pub approval_id: ApprovalId,
    /// Exact hook invocation awaiting the decision.
    pub hook_invocation_id: HookInvocationId,
    /// Tool call identity.
    pub call_id: CallId,
    /// Provider-visible tool name.
    pub tool_name: ToolName,
    /// Final durable-safe arguments.
    pub arguments: Value,
    /// Whether the tool observes or mutates state.
    pub tool_kind: ToolKind,
    /// Declared danger of the tool.
    pub danger_level: DangerLevel,
    /// Committed decision, when the request is already resolved.
    pub resolution: Option<ApprovalResolution>,
}

/// A committed approval decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ApprovalResolution {
    /// Sequence of the `ToolApprovalResolved` row.
    pub resolved_event_seq: u64,
    /// Committed decision.
    pub decision: ApprovalDecision,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_status_round_trips_its_database_text() {
        let statuses = [
            AgentStatus::Idle,
            AgentStatus::Running,
            AgentStatus::Finished,
            AgentStatus::Failed,
            AgentStatus::Cancelled,
        ];
        for status in statuses {
            assert_eq!(
                status
                    .as_str()
                    .parse::<AgentStatus>()
                    .expect("status text parses"),
                status
            );
        }
        assert!(AgentStatus::Finished.is_terminal());
        assert!(!AgentStatus::Running.is_terminal());
    }

    #[test]
    fn unknown_status_text_maps_to_durable_state_corrupt() {
        let error = "archived"
            .parse::<AgentStatus>()
            .expect_err("unknown status is corrupt");
        assert!(matches!(error, PostgresError::DurableStateCorrupt { .. }));
    }

    #[test]
    fn event_seq_decimal_encoding_round_trips_at_the_bigint_boundary() {
        assert_eq!(encode_event_seq(EVENT_SEQ_MAX), "9223372036854775807");
        assert_eq!(parse_event_seq("9223372036854775807"), Some(EVENT_SEQ_MAX));
        assert_eq!(parse_event_seq("0"), Some(0));
    }

    #[test]
    fn bigint_conversion_rejects_values_above_the_schema_boundary() {
        assert_eq!(
            u64_to_bigint(EVENT_SEQ_MAX, "overflow").expect("boundary fits"),
            i64::MAX
        );
        let overflow = EVENT_SEQ_MAX
            .checked_add(1)
            .expect("u64 has room above bigint max");
        assert!(matches!(
            u64_to_bigint(overflow, "overflow"),
            Err(PostgresError::InvalidCommand("overflow"))
        ));
    }

    #[test]
    fn event_seq_parser_rejects_non_decimal_and_overflowing_input() {
        assert_eq!(parse_event_seq(""), None);
        assert_eq!(parse_event_seq("-1"), None);
        assert_eq!(parse_event_seq("+7"), None);
        assert_eq!(parse_event_seq("1.5"), None);
        assert_eq!(parse_event_seq("abc"), None);
        assert_eq!(parse_event_seq(" 7"), None);
        assert_eq!(parse_event_seq("18446744073709551616"), None);
    }
}
