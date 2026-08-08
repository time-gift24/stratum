//! Typed errors for the Postgres execution-storage backend.
//!
//! Classification is always structural (enum variants), never string parsing.
//! Messages are lowercase without trailing periods, carry no SQL text, and
//! never include prompt, message, or tool payload contents.

use stratum_core::{AgentId, ApprovalId, HookInvocationId, SessionId, TurnId};
use thiserror::Error;
use uuid::Uuid;

use crate::types::AgentStatus;

/// Which versioned persisted shape a decode failure concerns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VersionedKind {
    /// `durable_events.event_version` / variant payload.
    EventPayload,
    /// `durable_events.runtime_snapshot_version` / runtime snapshot.
    RuntimeSnapshot,
    /// `agents.definition_schema_version` / resolved definition.
    ResolvedDefinition,
}

impl std::fmt::Display for VersionedKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::EventPayload => "event payload",
            Self::RuntimeSnapshot => "runtime snapshot",
            Self::ResolvedDefinition => "resolved definition",
        };
        f.write_str(name)
    }
}

/// Error returned by Postgres execution-storage commands and queries.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PostgresError {
    /// The Postgres connection could not be established.
    #[error("failed to connect to postgres")]
    Connect(#[source] sqlx::Error),
    /// Schema migrations could not be applied.
    #[error("failed to migrate postgres schema")]
    Migrate(#[source] sqlx::migrate::MigrateError),
    /// The addressed Agent does not exist.
    #[error("agent {agent_id} not found")]
    AgentNotFound {
        /// Missing Agent identity.
        agent_id: AgentId,
    },
    /// The addressed Turn has no durable `LoopStarted` on this Agent.
    #[error("turn {turn_id} not found for agent {agent_id}")]
    TurnNotFound {
        /// Owning Agent identity.
        agent_id: AgentId,
        /// Missing Turn identity.
        turn_id: TurnId,
    },
    /// The addressed approval request does not exist on this Agent and Turn.
    #[error("approval {approval_id} not found")]
    ApprovalNotFound {
        /// Missing approval identity.
        approval_id: ApprovalId,
    },
    /// An idempotency key was replayed with a different create request.
    #[error("idempotency key {idempotency_key} is bound to a different create request")]
    IdempotencyKeyConflict {
        /// Conflicting client idempotency key.
        idempotency_key: Uuid,
    },
    /// The caller's expected current Turn no longer matches durable state.
    #[error("stale turn expectation for agent {agent_id}")]
    StaleTurn {
        /// Agent whose current Turn moved on.
        agent_id: AgentId,
        /// Turn the caller expected (or `None` for first admission).
        expected: Option<TurnId>,
        /// Durable current Turn.
        actual: Option<TurnId>,
    },
    /// The Agent already has a running Turn and rejects admission.
    #[error("agent {agent_id} already has a running turn")]
    AgentBusy {
        /// Busy Agent identity.
        agent_id: AgentId,
    },
    /// The requested Session does not match the Agent's bound Session.
    #[error("session does not match the session bound to agent {agent_id}")]
    SessionMismatch {
        /// Agent with a different bound Session.
        agent_id: AgentId,
    },
    /// Another Agent runtime row is already running on this Session.
    #[error("session {session_id} already has a running agent")]
    SessionBusy {
        /// Busy Session identity.
        session_id: SessionId,
    },
    /// The command requires a running Turn but the Agent is not running.
    #[error("turn {turn_id} of agent {agent_id} is not running (status: {status})")]
    TurnNotRunning {
        /// Owning Agent identity.
        agent_id: AgentId,
        /// Turn that is not running.
        turn_id: TurnId,
        /// Durable Agent status.
        status: AgentStatus,
    },
    /// The approval was already resolved with the opposite decision.
    #[error("approval {approval_id} is already resolved with a different decision")]
    ApprovalAlreadyResolved {
        /// Conflicting approval identity.
        approval_id: ApprovalId,
    },
    /// The owning Turn reached a terminal event before the decision arrived.
    #[error("approval {approval_id} was invalidated by a terminal turn event")]
    ApprovalInvalidated {
        /// Invalidated approval identity.
        approval_id: ApprovalId,
    },
    /// A `ToolApprovalRequested` already exists for this hook invocation.
    #[error("hook invocation {hook_invocation_id} already has a durable approval request")]
    ApprovalAlreadyRequested {
        /// Hook invocation with an existing request.
        hook_invocation_id: HookInvocationId,
    },
    /// The compaction retained pointer does not address a real earlier
    /// `MessageAppended` of the same Agent; the append fails closed.
    #[error(
        "retained_from_event_seq {retained_from_event_seq} does not address an earlier message of agent {agent_id}"
    )]
    InvalidCompactionPointer {
        /// Owning Agent identity.
        agent_id: AgentId,
        /// Rejected retained pointer.
        retained_from_event_seq: u64,
    },
    /// The command violated a command-shape invariant (for example a
    /// compaction companion supplied for a non-compaction event).
    #[error("invalid command: {0}")]
    InvalidCommand(&'static str),
    /// A persisted shape declares a version this binary does not support.
    ///
    /// The data is structurally recognizable but newer than this binary; no
    /// upcast is attempted.
    #[error("unsupported {kind} version {version}")]
    RuntimeIncompatible {
        /// Versioned shape that could not be decoded.
        kind: VersionedKind,
        /// Declared, unsupported version.
        version: i32,
    },
    /// A persisted shape of a supported version is malformed or violates an
    /// identity/ordering invariant; durable truth is incomplete.
    #[error("durable state corrupt: {context}")]
    DurableStateCorrupt {
        /// What invariant failed; never contains payload contents.
        context: &'static str,
        /// Underlying decode failure, when one exists.
        #[source]
        source: Option<serde_json::Error>,
    },
    /// An agent-wide event sequence could not be represented as `bigint`.
    #[error("event sequence overflow for agent {agent_id}")]
    SequenceOverflow {
        /// Agent whose sequence space is exhausted.
        agent_id: AgentId,
    },
    /// A typed event could not be serialized for persistence.
    #[error("failed to serialize durable event {event_type}")]
    EventEncode {
        /// Stable type name of the event.
        event_type: &'static str,
        /// Serialization failure.
        #[source]
        source: serde_json::Error,
    },
    /// The storage backend failed or is unavailable.
    #[error("postgres execution store unavailable")]
    StoreUnavailable(#[source] sqlx::Error),
}

impl PostgresError {
    /// Builds a corruption error with an underlying decode source.
    pub(crate) fn corrupt(context: &'static str, source: serde_json::Error) -> Self {
        Self::DurableStateCorrupt {
            context,
            source: Some(source),
        }
    }

    /// Builds a corruption error without an underlying source.
    pub(crate) const fn corrupt_invariant(context: &'static str) -> Self {
        Self::DurableStateCorrupt {
            context,
            source: None,
        }
    }
}
