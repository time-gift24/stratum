//! Durable control-plane storage for the single-host recurring scheduler.
//!
//! Schedule definitions and occurrence indexes live beside the execution
//! ledger because Postgres is the process's only durable truth. They are not
//! projections, leases, or distributed claims: this version assumes exactly
//! one scheduler process for a database.

use std::str::FromStr;

use chrono::{DateTime, Utc};
use sqlx::{PgPool, Row};
use stratum_core::{AgentId, AgentName, AgentRuntimeId, ScheduleId, SessionId, TurnId};
use uuid::Uuid;

use crate::{PostgresBackend, PostgresError};

/// Durable definition of one recurring AgentRuntime launch.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScheduleDefinition {
    /// Stable schedule identity.
    pub schedule_id: ScheduleId,
    /// Agent definition resolved afresh for each occurrence.
    pub agent_name: AgentName,
    /// Canonical cron expression evaluated in the host's local timezone.
    pub cron_expression: String,
    /// Definition creation time.
    pub created_at: DateTime<Utc>,
}

/// New schedule command after API validation.
#[derive(Debug, Clone)]
pub struct CreateSchedule {
    /// Server-generated schedule identity.
    pub schedule_id: ScheduleId,
    /// Validated Agent definition name.
    pub agent_name: AgentName,
    /// Canonical cron expression.
    pub cron_expression: String,
}

/// Schedule occurrence lifecycle owned by the scheduler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ScheduleRunStatus {
    /// The occurrence identity is durable but runtime admission is unfinished.
    Starting,
    /// Runtime creation and the first user message were durably accepted.
    Accepted,
    /// Runtime creation or first-message admission failed.
    Failed,
}

impl ScheduleRunStatus {
    #[must_use]
    const fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Accepted => "accepted",
            Self::Failed => "failed",
        }
    }
}

impl FromStr for ScheduleRunStatus {
    type Err = PostgresError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "starting" => Ok(Self::Starting),
            "accepted" => Ok(Self::Accepted),
            "failed" => Ok(Self::Failed),
            _ => Err(PostgresError::ScheduleStateCorrupt {
                context: "schedule_runs.status outside the closed set",
                source: None,
            }),
        }
    }
}

/// Durable occurrence index linking a schedule to one conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ScheduleRun {
    /// Owning schedule.
    pub schedule_id: ScheduleId,
    /// Preallocated Session identity for the occurrence.
    pub session_id: SessionId,
    /// Key used for crash-safe AgentRuntime creation reconciliation.
    pub idempotency_key: Uuid,
    /// Created runtime, once runtime creation succeeded.
    pub agent_runtime_id: Option<AgentRuntimeId>,
    /// Immutable Agent definition pinned by the created runtime.
    pub agent_id: Option<AgentId>,
    /// First Turn, once its user message was accepted.
    pub turn_id: Option<TurnId>,
    /// Scheduler-owned occurrence state.
    pub status: ScheduleRunStatus,
    /// Time at which this occurrence began.
    pub triggered_at: DateTime<Utc>,
    /// Last scheduler state update.
    pub updated_at: DateTime<Utc>,
    /// Current execution status joined from `agent_states`, when a runtime exists.
    pub runtime_status: Option<crate::AgentStatus>,
}

/// Command that durably starts one schedule occurrence.
#[derive(Debug, Clone)]
pub struct BeginScheduleRun {
    /// Owning schedule.
    pub schedule_id: ScheduleId,
    /// Preallocated Session identity.
    pub session_id: SessionId,
    /// Runtime-create idempotency key.
    pub idempotency_key: Uuid,
    /// Occurrence time observed by the scheduler.
    pub triggered_at: DateTime<Utc>,
}

/// Terminal occurrence update.
#[derive(Debug, Clone)]
pub struct FinishScheduleRun {
    /// Owning schedule.
    pub schedule_id: ScheduleId,
    /// Preallocated occurrence Session.
    pub session_id: SessionId,
    /// Terminal scheduler status.
    pub status: ScheduleRunStatus,
    /// Created runtime, when creation succeeded.
    pub agent_runtime_id: Option<AgentRuntimeId>,
    /// Runtime's immutable Agent definition.
    pub agent_id: Option<AgentId>,
    /// Accepted first Turn; required only for `Accepted`.
    pub turn_id: Option<TurnId>,
}

/// Offset/limit query shared by schedule and occurrence pages.
#[derive(Debug, Clone, Copy)]
pub struct SchedulePageQuery {
    /// Zero-based row offset.
    pub offset: u64,
    /// Bounded page size.
    pub limit: u32,
}

/// One page of schedule definitions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct SchedulePage {
    /// Newest-first definitions.
    pub items: Vec<ScheduleDefinition>,
    /// Total definitions at query time.
    pub total: u64,
}

/// Query for occurrence history of one schedule.
#[derive(Debug, Clone, Copy)]
pub struct ScheduleRunsQuery {
    /// Owning schedule.
    pub schedule_id: ScheduleId,
    /// Zero-based row offset.
    pub offset: u64,
    /// Bounded page size.
    pub limit: u32,
}

/// One page of schedule occurrence indexes.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ScheduleRunsPage {
    /// Newest-first occurrence indexes.
    pub items: Vec<ScheduleRun>,
    /// Total occurrences for the schedule at query time.
    pub total: u64,
}

impl PostgresBackend {
    /// Creates one schedule definition.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::StoreUnavailable`] when Postgres rejects the write.
    #[tracing::instrument(name = "schedule_store.create", skip(self, command), fields(schedule_id = %command.schedule_id))]
    pub async fn create_schedule(
        &self,
        command: CreateSchedule,
    ) -> Result<ScheduleDefinition, PostgresError> {
        create_schedule(&self.pool, command).await
    }

    /// Reads one schedule definition.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::ScheduleNotFound`] when it does not exist,
    /// [`PostgresError::ScheduleStateCorrupt`] for malformed persisted state,
    /// or [`PostgresError::StoreUnavailable`] on storage failure.
    #[tracing::instrument(name = "schedule_store.read", skip(self), fields(schedule_id = %schedule_id))]
    pub async fn read_schedule(
        &self,
        schedule_id: ScheduleId,
    ) -> Result<ScheduleDefinition, PostgresError> {
        read_schedule(&self.pool, schedule_id).await
    }

    /// Reads one newest-first schedule page.
    ///
    /// # Errors
    ///
    /// Returns a typed corruption or storage error when persisted rows cannot
    /// be read safely.
    #[tracing::instrument(name = "schedule_store.list", skip(self))]
    pub async fn read_schedules(
        &self,
        query: SchedulePageQuery,
    ) -> Result<SchedulePage, PostgresError> {
        read_schedules(&self.pool, query).await
    }

    /// Reads every schedule definition from one statement snapshot for the scheduler loop.
    ///
    /// # Errors
    ///
    /// Returns a typed corruption or storage error when persisted rows cannot
    /// be read safely.
    #[tracing::instrument(name = "schedule_store.read_scheduler_definitions", skip(self))]
    pub async fn read_scheduler_definitions(
        &self,
    ) -> Result<Vec<ScheduleDefinition>, PostgresError> {
        read_scheduler_definitions(&self.pool).await
    }

    /// Commits a `starting` occurrence before runtime creation begins.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::ScheduleNotFound`] when the schedule vanished
    /// or a typed storage error when the occurrence cannot be committed.
    #[tracing::instrument(
        name = "schedule_store.begin_run",
        skip(self, command),
        fields(schedule_id = %command.schedule_id, session_id = %command.session_id)
    )]
    pub async fn begin_schedule_run(&self, command: BeginScheduleRun) -> Result<(), PostgresError> {
        begin_schedule_run(&self.pool, command).await
    }

    /// Advances a `starting` occurrence exactly once to a terminal scheduler status.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::InvalidScheduleRunTransition`] for an invalid
    /// shape or repeated transition, or a typed storage error.
    #[tracing::instrument(
        name = "schedule_store.finish_run",
        skip(self, command),
        fields(schedule_id = %command.schedule_id, session_id = %command.session_id)
    )]
    pub async fn finish_schedule_run(
        &self,
        command: FinishScheduleRun,
    ) -> Result<(), PostgresError> {
        finish_schedule_run(&self.pool, command).await
    }

    /// Reads one newest-first occurrence page for a schedule.
    ///
    /// # Errors
    ///
    /// Returns [`PostgresError::ScheduleNotFound`] when the schedule does not
    /// exist, or a typed corruption/storage error.
    #[tracing::instrument(name = "schedule_store.list_runs", skip(self), fields(schedule_id = %query.schedule_id))]
    pub async fn read_schedule_runs(
        &self,
        query: ScheduleRunsQuery,
    ) -> Result<ScheduleRunsPage, PostgresError> {
        read_schedule_runs(&self.pool, query).await
    }

    /// Reads a bounded oldest-first batch of unfinished occurrences for startup reconciliation.
    ///
    /// # Errors
    ///
    /// Returns a typed corruption or storage error.
    #[tracing::instrument(name = "schedule_store.list_starting_runs", skip(self))]
    pub async fn read_starting_schedule_runs(
        &self,
        limit: u32,
    ) -> Result<Vec<ScheduleRun>, PostgresError> {
        read_starting_schedule_runs(&self.pool, limit).await
    }
}

async fn create_schedule(
    pool: &PgPool,
    command: CreateSchedule,
) -> Result<ScheduleDefinition, PostgresError> {
    let row = sqlx::query(
        "INSERT INTO schedules (id, agent_name, cron_expression) \
         VALUES ($1, $2, $3) \
         RETURNING id, agent_name, cron_expression, created_at",
    )
    .bind(command.schedule_id.as_uuid())
    .bind(command.agent_name.as_str())
    .bind(command.cron_expression)
    .fetch_one(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    decode_schedule(&row)
}

async fn read_schedule(
    pool: &PgPool,
    schedule_id: ScheduleId,
) -> Result<ScheduleDefinition, PostgresError> {
    let row = sqlx::query(
        "SELECT id, agent_name, cron_expression, created_at FROM schedules WHERE id = $1",
    )
    .bind(schedule_id.as_uuid())
    .fetch_optional(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?
    .ok_or(PostgresError::ScheduleNotFound { schedule_id })?;
    decode_schedule(&row)
}

async fn read_schedules(
    pool: &PgPool,
    query: SchedulePageQuery,
) -> Result<SchedulePage, PostgresError> {
    let offset = page_offset(query.offset)?;
    let limit = i64::from(query.limit);
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM schedules")
        .fetch_one(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let rows = sqlx::query(
        "SELECT id, agent_name, cron_expression, created_at \
         FROM schedules ORDER BY created_at DESC, id DESC OFFSET $1 LIMIT $2",
    )
    .bind(offset)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    let items = rows.iter().map(decode_schedule).collect::<Result<_, _>>()?;
    Ok(SchedulePage {
        items,
        total: count_to_u64(total)?,
    })
}

async fn read_scheduler_definitions(
    pool: &PgPool,
) -> Result<Vec<ScheduleDefinition>, PostgresError> {
    let rows = sqlx::query(
        "SELECT id, agent_name, cron_expression, created_at \
         FROM schedules ORDER BY created_at DESC, id DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    rows.iter().map(decode_schedule).collect()
}

async fn begin_schedule_run(pool: &PgPool, command: BeginScheduleRun) -> Result<(), PostgresError> {
    let result = sqlx::query(
        "INSERT INTO schedule_runs \
             (schedule_id, session_id, idempotency_key, status, triggered_at) \
         VALUES ($1, $2, $3, 'starting', $4)",
    )
    .bind(command.schedule_id.as_uuid())
    .bind(command.session_id.as_uuid())
    .bind(command.idempotency_key)
    .bind(command.triggered_at)
    .execute(pool)
    .await;
    match result {
        Ok(_) => Ok(()),
        Err(source) if foreign_key_violation(&source) => Err(PostgresError::ScheduleNotFound {
            schedule_id: command.schedule_id,
        }),
        Err(source) => Err(PostgresError::StoreUnavailable(source)),
    }
}

async fn finish_schedule_run(
    pool: &PgPool,
    command: FinishScheduleRun,
) -> Result<(), PostgresError> {
    validate_finish_shape(&command)?;
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    validate_finish_identity(&mut tx, &command).await?;
    let result = sqlx::query(
        "UPDATE schedule_runs \
         SET status = $3, agent_runtime_id = $4, agent_id = $5, turn_id = $6, updated_at = now() \
         WHERE schedule_id = $1 AND session_id = $2 AND status = 'starting'",
    )
    .bind(command.schedule_id.as_uuid())
    .bind(command.session_id.as_uuid())
    .bind(command.status.as_str())
    .bind(command.agent_runtime_id.map(|id| id.as_uuid()))
    .bind(command.agent_id.map(|id| id.as_uuid()))
    .bind(command.turn_id.map(|id| id.as_uuid()))
    .execute(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    if result.rows_affected() == 1 {
        tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
        Ok(())
    } else {
        Err(PostgresError::InvalidScheduleRunTransition)
    }
}

async fn read_schedule_runs(
    pool: &PgPool,
    query: ScheduleRunsQuery,
) -> Result<ScheduleRunsPage, PostgresError> {
    let offset = page_offset(query.offset)?;
    let limit = i64::from(query.limit);
    let mut tx = pool
        .begin()
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM schedules WHERE id = $1)")
        .bind(query.schedule_id.as_uuid())
        .fetch_one(&mut *tx)
        .await
        .map_err(PostgresError::StoreUnavailable)?;
    if !exists {
        return Err(PostgresError::ScheduleNotFound {
            schedule_id: query.schedule_id,
        });
    }
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM schedule_runs WHERE schedule_id = $1")
            .bind(query.schedule_id.as_uuid())
            .fetch_one(&mut *tx)
            .await
            .map_err(PostgresError::StoreUnavailable)?;
    let rows = sqlx::query(
        "SELECT r.schedule_id, r.session_id, r.idempotency_key, r.agent_runtime_id, \
                r.agent_id, r.turn_id, r.status, r.triggered_at, r.updated_at, \
                s.status AS runtime_status \
         FROM schedule_runs r \
         LEFT JOIN agent_states s ON s.id = r.agent_runtime_id \
         WHERE r.schedule_id = $1 \
         ORDER BY r.triggered_at DESC, r.session_id DESC OFFSET $2 LIMIT $3",
    )
    .bind(query.schedule_id.as_uuid())
    .bind(offset)
    .bind(limit)
    .fetch_all(&mut *tx)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    tx.commit().await.map_err(PostgresError::StoreUnavailable)?;
    let items = rows.iter().map(decode_run).collect::<Result<_, _>>()?;
    Ok(ScheduleRunsPage {
        items,
        total: count_to_u64(total)?,
    })
}

async fn read_starting_schedule_runs(
    pool: &PgPool,
    limit: u32,
) -> Result<Vec<ScheduleRun>, PostgresError> {
    let rows = sqlx::query(
        "SELECT r.schedule_id, r.session_id, r.idempotency_key, r.agent_runtime_id, \
                r.agent_id, r.turn_id, r.status, r.triggered_at, r.updated_at, \
                s.status AS runtime_status \
         FROM schedule_runs r \
         LEFT JOIN agent_states s ON s.id = r.agent_runtime_id \
         WHERE r.status = 'starting' \
         ORDER BY r.triggered_at, r.schedule_id, r.session_id LIMIT $1",
    )
    .bind(i64::from(limit))
    .fetch_all(pool)
    .await
    .map_err(PostgresError::StoreUnavailable)?;
    rows.iter().map(decode_run).collect()
}

fn decode_schedule(row: &sqlx::postgres::PgRow) -> Result<ScheduleDefinition, PostgresError> {
    let agent_name: String = row
        .try_get("agent_name")
        .map_err(PostgresError::StoreUnavailable)?;
    let agent_name = agent_name
        .parse()
        .map_err(|source| PostgresError::ScheduleStateCorrupt {
            context: "schedules.agent_name is invalid",
            source: Some(source),
        })?;
    Ok(ScheduleDefinition {
        schedule_id: ScheduleId::from(
            row.try_get::<Uuid, _>("id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        agent_name,
        cron_expression: row
            .try_get("cron_expression")
            .map_err(PostgresError::StoreUnavailable)?,
        created_at: row
            .try_get("created_at")
            .map_err(PostgresError::StoreUnavailable)?,
    })
}

fn decode_run(row: &sqlx::postgres::PgRow) -> Result<ScheduleRun, PostgresError> {
    let status: String = row
        .try_get("status")
        .map_err(PostgresError::StoreUnavailable)?;
    let runtime_status = row
        .try_get::<Option<String>, _>("runtime_status")
        .map_err(PostgresError::StoreUnavailable)?
        .map(|value| value.parse())
        .transpose()?;
    let run = ScheduleRun {
        schedule_id: ScheduleId::from(
            row.try_get::<Uuid, _>("schedule_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        session_id: SessionId::from(
            row.try_get::<Uuid, _>("session_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        idempotency_key: row
            .try_get("idempotency_key")
            .map_err(PostgresError::StoreUnavailable)?,
        agent_runtime_id: optional_id(
            row.try_get("agent_runtime_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        agent_id: optional_id(
            row.try_get("agent_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        turn_id: optional_id(
            row.try_get("turn_id")
                .map_err(PostgresError::StoreUnavailable)?,
        ),
        status: status.parse()?,
        triggered_at: row
            .try_get("triggered_at")
            .map_err(PostgresError::StoreUnavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(PostgresError::StoreUnavailable)?,
        runtime_status,
    };
    validate_persisted_run(&run)?;
    Ok(run)
}

fn validate_finish_shape(command: &FinishScheduleRun) -> Result<(), PostgresError> {
    let valid = match command.status {
        ScheduleRunStatus::Starting => false,
        ScheduleRunStatus::Accepted => {
            command.agent_runtime_id.is_some()
                && command.agent_id.is_some()
                && command.turn_id.is_some()
        }
        ScheduleRunStatus::Failed => {
            command.turn_id.is_none()
                && command.agent_runtime_id.is_some() == command.agent_id.is_some()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(PostgresError::InvalidScheduleRunTransition)
    }
}

async fn validate_finish_identity(
    tx: &mut sqlx::PgConnection,
    command: &FinishScheduleRun,
) -> Result<(), PostgresError> {
    let Some(agent_runtime_id) = command.agent_runtime_id else {
        return Ok(());
    };
    let row =
        sqlx::query("SELECT agent_id, session_id, current_turn_id FROM agent_states WHERE id = $1")
            .bind(agent_runtime_id.as_uuid())
            .fetch_optional(&mut *tx)
            .await
            .map_err(PostgresError::StoreUnavailable)?
            .ok_or(PostgresError::InvalidScheduleRunTransition)?;
    let persisted_agent_id: Uuid = row
        .try_get("agent_id")
        .map_err(PostgresError::StoreUnavailable)?;
    if command.agent_id.map(AgentId::as_uuid) != Some(persisted_agent_id) {
        return Err(PostgresError::InvalidScheduleRunTransition);
    }
    if command.status == ScheduleRunStatus::Accepted {
        let session_id: Option<Uuid> = row
            .try_get("session_id")
            .map_err(PostgresError::StoreUnavailable)?;
        let current_turn_id: Option<Uuid> = row
            .try_get("current_turn_id")
            .map_err(PostgresError::StoreUnavailable)?;
        let Some(turn_id) = command.turn_id else {
            return Err(PostgresError::InvalidScheduleRunTransition);
        };
        if session_id != Some(command.session_id.as_uuid())
            || current_turn_id != Some(turn_id.as_uuid())
            || !crate::queries::turn_has_user_message(tx, agent_runtime_id, turn_id).await?
        {
            return Err(PostgresError::InvalidScheduleRunTransition);
        }
    }
    Ok(())
}

fn validate_persisted_run(run: &ScheduleRun) -> Result<(), PostgresError> {
    let valid = match run.status {
        ScheduleRunStatus::Starting => {
            run.agent_runtime_id.is_none() && run.agent_id.is_none() && run.turn_id.is_none()
        }
        ScheduleRunStatus::Accepted => {
            run.agent_runtime_id.is_some() && run.agent_id.is_some() && run.turn_id.is_some()
        }
        ScheduleRunStatus::Failed => {
            run.turn_id.is_none() && run.agent_runtime_id.is_some() == run.agent_id.is_some()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(PostgresError::ScheduleStateCorrupt {
            context: "schedule run lifecycle shape is invalid",
            source: None,
        })
    }
}

fn page_offset(offset: u64) -> Result<i64, PostgresError> {
    i64::try_from(offset)
        .map_err(|_| PostgresError::InvalidCommand("schedule offset exceeds bigint range"))
}

fn count_to_u64(total: i64) -> Result<u64, PostgresError> {
    u64::try_from(total).map_err(|_| PostgresError::ScheduleStateCorrupt {
        context: "schedule count is negative",
        source: None,
    })
}

fn optional_id<T: From<Uuid>>(value: Option<Uuid>) -> Option<T> {
    value.map(T::from)
}

fn foreign_key_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(sqlx::error::DatabaseError::code)
        .is_some_and(|code| code == "23503")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finish_shape_requires_terminal_identity_contract() {
        let schedule_id = ScheduleId::new();
        let session_id = SessionId::new();
        let accepted = FinishScheduleRun {
            schedule_id,
            session_id,
            status: ScheduleRunStatus::Accepted,
            agent_runtime_id: Some(AgentRuntimeId::new()),
            agent_id: Some(AgentId::new()),
            turn_id: Some(TurnId::new()),
        };
        assert!(validate_finish_shape(&accepted).is_ok());

        let mut invalid = accepted;
        invalid.turn_id = None;
        assert!(matches!(
            validate_finish_shape(&invalid),
            Err(PostgresError::InvalidScheduleRunTransition)
        ));
    }

    #[test]
    fn persisted_status_fails_closed_on_unknown_values() {
        assert!(matches!(
            "future".parse::<ScheduleRunStatus>(),
            Err(PostgresError::ScheduleStateCorrupt { .. })
        ));
    }
}
