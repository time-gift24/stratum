//! Error types for agent store persistence.

use stratum_core::{AgentId, AgentLocation, ChatRole, SessionId, TurnId};
use thiserror::Error;

use crate::AgentStatus;

/// Error returned by agent store operations.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum StoreError {
    /// A store backend operation failed.
    #[error("store backend operation failed")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync + 'static>),
    /// The agent state file is missing.
    #[error("agent store is missing")]
    AgentMissing,
    /// The persisted state schema version is not supported.
    #[error("unsupported agent state version: {version}")]
    UnsupportedStateVersion {
        /// Unsupported schema version.
        version: u32,
    },
    /// A current state lacks the required model configuration.
    #[error("current agent state is missing model configuration")]
    MissingModelConfig,
    /// An iteration can only complete while the agent is running.
    #[error("agent is not running: {actual:?}")]
    AgentNotRunning {
        /// Current persisted agent status.
        actual: AgentStatus,
    },
    /// A new running transition would replace another active session operation.
    #[error("persisted active session operation conflict")]
    RunningSessionConflict {
        /// Session currently persisted as running.
        current: Option<SessionId>,
        /// Session that attempted to become running.
        attempted: Option<SessionId>,
    },
    /// The requested iteration differs from the durable frontier.
    #[error("iteration mismatch: expected {expected}, actual {actual}")]
    IterationMismatch {
        /// Current durable iteration frontier.
        expected: u64,
        /// Requested iteration.
        actual: u64,
    },
    /// The next iteration cannot be represented.
    #[error("iteration overflow")]
    IterationOverflow,
    /// The next message sequence cannot be represented.
    #[error("message sequence overflow")]
    SequenceOverflow,
    /// A message role cannot be committed to store history.
    #[error("invalid store message role: {role:?}")]
    InvalidMessageRole {
        /// Rejected message role.
        role: ChatRole,
    },
    /// The requested history page size is outside the supported range.
    #[error("history limit must be between 1 and {maximum}: {actual}")]
    InvalidHistoryLimit {
        /// Requested page size.
        actual: usize,
        /// Maximum supported page size.
        maximum: usize,
    },
    /// The exclusive history front exceeds its inclusive barrier.
    #[error("history front {after_seq} exceeds barrier {through_seq}")]
    InvalidHistoryRange {
        /// Exclusive lower sequence bound.
        after_seq: u64,
        /// Inclusive upper sequence bound.
        through_seq: u64,
    },
    /// The requested history barrier exceeds the committed sequence.
    #[error("history barrier {through_seq} exceeds committed sequence {last_seq}")]
    HistoryBarrierBeyondLast {
        /// Requested inclusive upper sequence bound.
        through_seq: u64,
        /// Last committed message sequence.
        last_seq: u64,
    },
    /// Persisted state or an event belongs to a different agent.
    #[error("store agent mismatch: expected {expected}, actual {actual}")]
    AgentMismatch {
        /// Agent identity required by the store.
        expected: AgentId,
        /// Agent identity found in persisted data.
        actual: AgentId,
    },
    /// A persisted event belongs to a different session.
    #[error("store session mismatch: expected {expected}, actual {actual}")]
    SessionMismatch {
        /// Session identity required by the store.
        expected: SessionId,
        /// Session identity found in persisted data.
        actual: SessionId,
    },
    /// A persisted event belongs to a different turn.
    #[error("store turn mismatch: expected {expected}, actual {actual}")]
    TurnMismatch {
        /// Turn identity required by the store.
        expected: TurnId,
        /// Turn identity found in persisted data.
        actual: TurnId,
    },
    /// A persisted event has a different Agent execution location.
    #[error("store agent location mismatch")]
    LocationMismatch {
        /// Location required by the active turn.
        expected: AgentLocation,
        /// Location found in persisted data.
        actual: AgentLocation,
    },
    /// The active turn is missing its pinned runtime snapshot.
    #[error("active turn is missing runtime snapshot")]
    MissingRuntimeSnapshot,
    /// A pinned runtime component differs from the available runtime.
    #[error("pinned runtime component mismatch: {component}")]
    RuntimeSnapshotMismatch {
        /// Component that failed closed.
        component: &'static str,
    },
    /// A message path sequence differs from its event sequence.
    #[error("message sequence mismatch: path {path_seq}, event {event_seq}")]
    MessageSequenceMismatch {
        /// Sequence encoded in the file path.
        path_seq: u64,
        /// Sequence encoded in the event.
        event_seq: u64,
    },
    /// A message within the committed range is absent.
    #[error("committed message is missing: {seq}")]
    MissingCommittedMessage {
        /// Missing committed sequence.
        seq: u64,
    },
    /// A store message file contains another event type.
    #[error("store file does not contain an agent message")]
    UnexpectedMessageEvent,
    /// A store message filename does not encode a valid sequence.
    #[error("invalid message filename: {file_name}")]
    InvalidMessageFilename {
        /// Invalid filename.
        file_name: String,
    },
    /// A message exists beyond the allowed committed frontier.
    #[error("message {seq} exists beyond allowed frontier {frontier}")]
    MessageBeyondFrontier {
        /// Unexpected message sequence.
        seq: u64,
        /// Maximum allowed message sequence.
        frontier: u64,
    },
    /// The store backend does not provide compare-and-swap.
    #[error("store backend does not support compare-and-swap")]
    CasUnsupported,
    /// The complete store compare-and-swap update timed out.
    #[error("store compare-and-swap timed out")]
    CasTimeout,
    /// Every permitted store compare-and-swap write conflicted.
    #[error("store compare-and-swap retries exhausted")]
    CasRetriesExhausted,
    /// Persisted agent state is malformed JSON.
    #[error("invalid agent state json")]
    DecodeState(#[source] serde_json::Error),
    /// A persisted message envelope is malformed JSON.
    #[error("invalid message envelope json")]
    DecodeMessage(#[source] serde_json::Error),
    /// Store state or a message could not be encoded as JSON.
    #[error("failed to encode store json")]
    Encode(#[source] serde_json::Error),
}

impl StoreError {
    /// Wraps a store backend failure.
    pub fn backend(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Backend(Box::new(source))
    }
}
