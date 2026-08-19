//! Concrete Postgres execution storage: the only durable truth for Agent
//! execution.
//!
//! Four tables carry every execution fact: `agents` (immutable identity and
//! resolved definition), `agent_states` (thin current/recent-Turn state and
//! the AgentRuntime-wide high-water), `durable_events` (the append-only runtime
//! ledger), and `transcript_compactions` (durable compaction companions).
//! Every view — AgentRuntime view, product history, pending approvals, latest usage,
//! resume slices — is derived from the ledger at a fixed barrier; there are
//! no projections, no outbox, and no rebuild metadata.
//!
//! All durable writers share one transaction template: the exact
//! `agent_states` row is locked `FOR UPDATE` (it is both the `event_seq`
//! allocator and the serialization point for concurrent writers of one
//! runtime), exact runtime/Session/Turn/status expectations are validated, the
//! versioned row is inserted, only the state side effect owned by that event
//! is applied, and the high-water advances in the same commit. A
//! [`CommitReceipt`] is returned only after commit.
//!
//! The crate is concrete: no traits, one implementation, and only the
//! assembly layer (`stratum-api`) calls it.

pub mod error;
pub mod scheduler;
pub mod types;

mod codec;
mod commands;
mod queries;

use sqlx::PgPool;
use stratum_core::{AgentId, AgentRuntimeId, HookInvocationId, SessionId, TurnId};
use uuid::Uuid;

pub use error::{PostgresError, VersionedKind};
pub use scheduler::{
    BeginScheduleRun, CreateSchedule, FinishScheduleRun, ScheduleDefinition, SchedulePage,
    SchedulePageQuery, ScheduleRun, ScheduleRunStatus, ScheduleRunsPage, ScheduleRunsQuery,
};
pub use types::{
    AgentRuntimeCreated, AgentRuntimeStateView, AgentRuntimeView, AgentStatus, AppendEvent,
    ApprovalFacts, ApprovalLookup, ApprovalResolution, BeginTurn, CommitReceipt, CompactionInput,
    CreateAgentRuntime, CreateAgentRuntimeOutcome, CreateKeyLookup, DurableEventRow, EVENT_SEQ_MAX,
    HISTORY_DEFAULT_LIMIT, HISTORY_MAX_LIMIT, HISTORY_SOFT_PAGE_BUDGET_BYTES, HistoryItem,
    HistoryPage, HistoryQuery, HookInvocationLookup, LoopStartedRecord, PendingApproval,
    ResolveApproval, ResolveApprovalOutcome, ResolvedDefinitionV1, ResumeSliceQuery,
    TranscriptCompaction, encode_event_seq, parse_event_seq,
};

/// Shared Postgres execution-storage handle: one connection pool over the
/// migrated baseline schema.
#[derive(Clone)]
pub struct PostgresBackend {
    pool: PgPool,
}

impl PostgresBackend {
    /// Connects to Postgres and applies the schema baseline.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::Connect`] when the database is unreachable and
    /// [`PostgresError::Migrate`] when the baseline cannot be applied.
    pub async fn connect(database_url: &str) -> Result<Self, PostgresError> {
        let pool = PgPool::connect(database_url)
            .await
            .map_err(PostgresError::Connect)?;
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(PostgresError::Migrate)?;
        Ok(Self { pool })
    }

    /// Liveness probe for the core readiness dependency: a trivial round-trip
    /// against the migrated schema.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::StoreUnavailable`] when the database cannot
    /// answer.
    pub async fn ping(&self) -> Result<(), PostgresError> {
        sqlx::query("SELECT 1")
            .execute(&self.pool)
            .await
            .map_err(PostgresError::StoreUnavailable)?;
        Ok(())
    }

    /// Reads the immutable create result behind one idempotency key, before
    /// any template access.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::DurableStateCorrupt`] when the stored tag
    /// violates its boundary and [`PostgresError::StoreUnavailable`] on
    /// storage failure.
    pub async fn find_agent_runtime_by_idempotency_key(
        &self,
        idempotency_key: Uuid,
    ) -> Result<Option<CreateKeyLookup>, PostgresError> {
        queries::find_agent_runtime_by_idempotency_key(&self.pool, idempotency_key).await
    }

    /// Finds the one open journaled hook invocation at an exact address (the
    /// kernel journals at most one open invocation per address).
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::RuntimeIncompatible`] when the matched pending
    /// row declares an unsupported event version,
    /// [`PostgresError::DurableStateCorrupt`] when a persisted
    /// invocation identity is malformed and [`PostgresError::StoreUnavailable`]
    /// on storage failure.
    pub async fn read_open_hook_invocation(
        &self,
        lookup: HookInvocationLookup,
    ) -> Result<Option<HookInvocationId>, PostgresError> {
        queries::read_open_hook_invocation(&self.pool, lookup).await
    }

    /// Creates one AgentRuntime and materializes or reuses its immutable Agent
    /// definition row, or replays the original key-owned runtime.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::AgentVersionConflict`] when an exact
    /// name/version tag already identifies a different definition and
    /// [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn create_agent_runtime(
        &self,
        command: CreateAgentRuntime,
    ) -> Result<CreateAgentRuntimeOutcome, PostgresError> {
        commands::create_agent_runtime(&self.pool, command).await
    }

    /// Admits a new Turn: CAS on the expected current Turn, Session binding,
    /// `LoopStarted` with the v1 runtime snapshot, and the running transition
    /// in one transaction.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::AgentRuntimeNotFound`],
    /// [`PostgresError::AgentRuntimeBusy`],
    /// [`PostgresError::StaleTurn`], [`PostgresError::SessionMismatch`], or
    /// [`PostgresError::SessionBusy`] on expectation failures and
    /// [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn begin_turn(&self, command: BeginTurn) -> Result<CommitReceipt, PostgresError> {
        commands::begin_turn(&self.pool, command).await
    }

    /// Revalidates an exact running Session/Turn under the AgentRuntime state
    /// row lock immediately before a prepared resume task is installed.
    ///
    /// This operation does not mutate durable state or append an event.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::AgentRuntimeNotFound`],
    /// [`PostgresError::SessionMismatch`], [`PostgresError::StaleTurn`], or
    /// [`PostgresError::TurnNotRunning`] when the prepared resume is stale,
    /// and [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn revalidate_resume(
        &self,
        agent_runtime_id: AgentRuntimeId,
        agent_id: AgentId,
        session_id: SessionId,
        turn_id: TurnId,
    ) -> Result<(), PostgresError> {
        commands::revalidate_resume(&self.pool, agent_runtime_id, agent_id, session_id, turn_id)
            .await
    }

    /// Appends one durable event through the centralized transaction shared
    /// by every writer, applying only the side effect the event owns.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::AgentRuntimeNotFound`],
    /// [`PostgresError::SessionMismatch`], [`PostgresError::StaleTurn`],
    /// [`PostgresError::TurnNotRunning`],
    /// [`PostgresError::ApprovalAlreadyRequested`],
    /// [`PostgresError::ApprovalIdConflict`],
    /// [`PostgresError::InvalidCompactionPointer`], or
    /// [`PostgresError::InvalidCommand`] on validation failure and
    /// [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn append_event(&self, command: AppendEvent) -> Result<CommitReceipt, PostgresError> {
        commands::append_event(&self.pool, command).await
    }

    /// Resolves one durable approval request, linearized with every other
    /// writer of the AgentRuntime.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::AgentRuntimeNotFound`],
    /// [`PostgresError::ApprovalNotFound`], [`PostgresError::StaleTurn`],
    /// [`PostgresError::ApprovalInvalidated`],
    /// [`PostgresError::ApprovalAlreadyResolved`], or
    /// [`PostgresError::RuntimeIncompatible`] when a matched approval row
    /// declares an unsupported event version,
    /// [`PostgresError::DurableStateCorrupt`] when such a row fails strict v1
    /// decode, and [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn resolve_approval(
        &self,
        command: ResolveApproval,
    ) -> Result<ResolveApprovalOutcome, PostgresError> {
        commands::resolve_approval(&self.pool, command).await
    }

    /// Reads the thin durable AgentRuntime state.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::AgentRuntimeNotFound`] when the runtime does not exist,
    /// [`PostgresError::DurableStateCorrupt`] when persisted shapes fail v1
    /// decode, and [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn read_agent_runtime_state(
        &self,
        agent_runtime_id: AgentRuntimeId,
    ) -> Result<AgentRuntimeStateView, PostgresError> {
        queries::read_agent_runtime_state(&self.pool, agent_runtime_id).await
    }

    /// Reads the AgentRuntime view in one MVCC snapshot: identities, thin state,
    /// barrier (`snapshot_event_seq` equals `last_event_seq`), derived pending
    /// approvals and latest usage.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::AgentRuntimeNotFound`] when the runtime does not exist,
    /// [`PostgresError::RuntimeIncompatible`] when a derived approval row
    /// declares an unsupported event version,
    /// [`PostgresError::DurableStateCorrupt`] when persisted shapes fail v1
    /// decode, and [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn read_agent_runtime_view(
        &self,
        agent_runtime_id: AgentRuntimeId,
    ) -> Result<AgentRuntimeView, PostgresError> {
        queries::read_agent_runtime_view(&self.pool, agent_runtime_id).await
    }

    /// Checks whether one exact Turn contains a strictly decoded user message.
    ///
    /// This narrow reconciliation query is used by the single-host scheduler
    /// to distinguish a durably accepted conversation from a crash after
    /// `LoopStarted` but before the first message commit.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::DurableStateCorrupt`] when a selected message
    /// row is malformed and [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn turn_has_user_message(
        &self,
        agent_runtime_id: AgentRuntimeId,
        turn_id: TurnId,
    ) -> Result<bool, PostgresError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(PostgresError::StoreUnavailable)?;
        let found = queries::turn_has_user_message(&mut tx, agent_runtime_id, turn_id).await?;
        tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
        Ok(found)
    }

    /// Reads one ascending product-history page from the filtered durable
    /// ledger.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::DurableStateCorrupt`] when a visible row fails
    /// strict v1 decode and [`PostgresError::StoreUnavailable`] on storage
    /// failure.
    pub async fn read_history_page(
        &self,
        query: HistoryQuery,
    ) -> Result<HistoryPage, PostgresError> {
        queries::read_history_page(&self.pool, query).await
    }

    /// Reads the `LoopStarted` row and runtime snapshot of one exact Turn.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::TurnNotFound`] when the Turn has no
    /// `LoopStarted`, [`PostgresError::RuntimeIncompatible`] for an
    /// unsupported snapshot version, [`PostgresError::DurableStateCorrupt`]
    /// for a malformed v1 snapshot, and [`PostgresError::StoreUnavailable`] on
    /// storage failure.
    pub async fn read_loop_started(
        &self,
        agent_runtime_id: AgentRuntimeId,
        turn_id: TurnId,
    ) -> Result<LoopStartedRecord, PostgresError> {
        queries::read_loop_started(&self.pool, agent_runtime_id, turn_id).await
    }

    /// Reads and verifies the complete gapless `(base, through]` current-Turn
    /// slice for resume.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::DurableStateCorrupt`] when the slice has
    /// missing rows, foreign identity, or fails strict decode, and
    /// [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn read_resume_slice(
        &self,
        query: ResumeSliceQuery,
    ) -> Result<Vec<DurableEventRow>, PostgresError> {
        queries::read_resume_slice(&self.pool, query).await
    }

    /// Reads decoded durable rows in `(from_event_seq, to_event_seq]` for one
    /// AgentRuntime, in order (dispatcher scans and full-replay recovery).
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::DurableStateCorrupt`] when a row fails strict
    /// decode and [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn read_events_range(
        &self,
        agent_runtime_id: AgentRuntimeId,
        from_event_seq: u64,
        to_event_seq: u64,
    ) -> Result<Vec<DurableEventRow>, PostgresError> {
        queries::read_events_range(&self.pool, agent_runtime_id, from_event_seq, to_event_seq).await
    }

    /// Reads the latest valid compaction companion at or below
    /// `base_event_seq`.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::DurableStateCorrupt`] when the companion or
    /// its summary is malformed or disagrees with its discriminator, and
    /// [`PostgresError::StoreUnavailable`] on storage failure.
    pub async fn read_latest_companion(
        &self,
        agent_runtime_id: AgentRuntimeId,
        base_event_seq: u64,
    ) -> Result<Option<TranscriptCompaction>, PostgresError> {
        queries::read_latest_companion(&self.pool, agent_runtime_id, base_event_seq).await
    }

    /// Reads ledger facts about one approval request for the decide Handler.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::RuntimeIncompatible`] when a matched row
    /// declares an unsupported event version,
    /// [`PostgresError::DurableStateCorrupt`] when a payload fails
    /// strict v1 decode and [`PostgresError::StoreUnavailable`] on storage
    /// failure.
    pub async fn read_approval(
        &self,
        agent_runtime_id: AgentRuntimeId,
        turn_id: TurnId,
        lookup: ApprovalLookup,
    ) -> Result<Option<ApprovalFacts>, PostgresError> {
        queries::read_approval(&self.pool, agent_runtime_id, turn_id, lookup).await
    }
}
