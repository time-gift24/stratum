//! Single-process cron scheduling over the Postgres execution boundary.
//!
//! This module deliberately provides no lease, fencing token, or distributed
//! ownership protocol. Exactly one Stratum API process may schedule a given
//! execution database in this version.

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Local, Utc};
use cron::Schedule;
use stratum_core::{AgentName, ScheduleId, SessionId};
use stratum_postgres::{
    BeginScheduleRun, CreateSchedule, FinishScheduleRun, ScheduleDefinition, ScheduleRunStatus,
};

use crate::dto::{MessageRequest, ScheduleView};
use crate::error::{ApiError, ErrorKind};
use crate::http::agents::create_agent_runtime_from_template;
use crate::http::turns::admit_message;
use crate::state::AppState;

const SCHEDULE_TRIGGER_MESSAGE: &str = "执行计划任务";
const SCHEDULER_RETRY_DELAY: Duration = Duration::from_secs(1);
const SCHEDULER_PAGE_SIZE: u32 = 100;

/// Creates one validated recurring schedule and wakes the local scheduler.
///
/// # Errors
///
/// Returns a typed API error when the Agent definition is unavailable, the
/// cron expression is invalid, or Postgres cannot persist the definition.
pub(crate) async fn create_schedule(
    state: &AppState,
    agent_name: AgentName,
    cron_expression: String,
) -> Result<ScheduleView, ApiError> {
    let _admission = state.admission().enter()?;
    state.resolve_agent_definition(&agent_name).await?;
    let cron_expression = canonical_cron(&cron_expression)?;
    let schedule_id = ScheduleId::new();
    let definition = state
        .pg()
        .create_schedule(CreateSchedule {
            schedule_id,
            agent_name,
            cron_expression,
        })
        .await
        .map_err(ApiError::from_postgres)?;
    let view = schedule_view(definition)?;
    state.scheduler_wake().notify_one();
    Ok(view)
}

/// Reconciles occurrences interrupted between their durable `starting` row
/// and terminal scheduler update.
///
/// A runtime whose first Turn is already bound to the preallocated Session is
/// accepted; a missing or still-idle runtime is failed. This does not retry a
/// partially admitted Turn or perform missed-run catch-up.
///
/// # Errors
///
/// Returns a typed API error when persisted scheduler or runtime state cannot
/// be read or safely reconciled.
pub(crate) async fn reconcile(state: &AppState) -> Result<(), ApiError> {
    for definition in load_definitions(state).await? {
        next_run_at(&definition, Utc::now())
            .map_err(|source| ApiError::with_source(ErrorKind::DurableStateCorrupt, source))?;
    }

    loop {
        let runs = state
            .pg()
            .read_starting_schedule_runs(SCHEDULER_PAGE_SIZE)
            .await
            .map_err(ApiError::from_postgres)?;
        if runs.is_empty() {
            return Ok(());
        }
        for run in runs {
            let existing = state
                .pg()
                .find_agent_runtime_by_idempotency_key(run.idempotency_key)
                .await
                .map_err(ApiError::from_postgres)?;
            let Some(existing) = existing else {
                finish_failed(state, run.schedule_id, run.session_id, None).await?;
                continue;
            };
            let view = state
                .pg()
                .read_agent_runtime_view(existing.agent_runtime_id)
                .await
                .map_err(ApiError::from_postgres)?;
            match (view.session_id, view.current_turn_id) {
                (Some(session_id), Some(turn_id)) if session_id == run.session_id => {
                    let has_user_message = state
                        .pg()
                        .turn_has_user_message(view.agent_runtime_id, turn_id)
                        .await
                        .map_err(ApiError::from_postgres)?;
                    if has_user_message {
                        state
                            .pg()
                            .finish_schedule_run(FinishScheduleRun {
                                schedule_id: run.schedule_id,
                                session_id: run.session_id,
                                status: ScheduleRunStatus::Accepted,
                                agent_runtime_id: Some(view.agent_runtime_id),
                                agent_id: Some(view.agent_id),
                                turn_id: Some(turn_id),
                            })
                            .await
                            .map_err(ApiError::from_postgres)?;
                    } else {
                        finish_failed(
                            state,
                            run.schedule_id,
                            run.session_id,
                            Some((view.agent_runtime_id, view.agent_id)),
                        )
                        .await?;
                    }
                }
                (None, None) => {
                    finish_failed(
                        state,
                        run.schedule_id,
                        run.session_id,
                        Some((existing.agent_runtime_id, existing.agent_id)),
                    )
                    .await?;
                }
                _ => {
                    return Err(ApiError::from_postgres(
                        stratum_postgres::PostgresError::ScheduleStateCorrupt {
                            context: "starting schedule run disagrees with its runtime session",
                            source: None,
                        },
                    ));
                }
            }
        }
    }
}

/// Starts the one process-owned scheduler task.
pub(crate) fn start(state: &Arc<AppState>) {
    let state = Arc::clone(state);
    let owner = Arc::clone(&state);
    owner.spawn_runtime_task(async move {
        scheduler_loop(state).await;
    });
}

/// Maps one stored definition to its API projection and evaluates the next occurrence.
///
/// # Errors
///
/// Returns a typed corruption error when a persisted cron expression is no
/// longer valid for this binary.
pub(crate) fn schedule_view(definition: ScheduleDefinition) -> Result<ScheduleView, ApiError> {
    let next_run_at = next_run_at(&definition, Utc::now())
        .map_err(|source| ApiError::with_source(ErrorKind::DurableStateCorrupt, source))?;
    Ok(ScheduleView {
        schedule_id: definition.schedule_id,
        agent_name: definition.agent_name.into(),
        cron_expression: definition.cron_expression,
        created_at: definition.created_at,
        next_run_at,
    })
}

async fn scheduler_loop(state: Arc<AppState>) {
    loop {
        let shutdown = state.shutdown_token();
        let definitions = match load_definitions(&state).await {
            Ok(definitions) => definitions,
            Err(error) => {
                tracing::error!(
                    error.code = error.kind().code(),
                    "scheduler could not load definitions"
                );
                tokio::select! {
                    () = shutdown.cancelled() => return,
                    () = state.scheduler_wake().notified() => continue,
                    () = tokio::time::sleep(SCHEDULER_RETRY_DELAY) => continue,
                }
            }
        };
        let now = Utc::now();
        let mut upcoming = Vec::with_capacity(definitions.len());
        for definition in definitions {
            match next_run_at(&definition, now) {
                Ok(next) => upcoming.push((definition, next)),
                Err(error) => tracing::error!(
                    schedule_id = %definition.schedule_id,
                    error.code = error.kind().code(),
                    "scheduler rejected a persisted cron expression"
                ),
            }
        }
        let Some(next) = upcoming.iter().map(|(_, next)| *next).min() else {
            tokio::select! {
                () = shutdown.cancelled() => return,
                () = state.scheduler_wake().notified() => continue,
            }
        };
        let delay = next
            .signed_duration_since(Utc::now())
            .to_std()
            .unwrap_or_default();
        tokio::select! {
            () = shutdown.cancelled() => return,
            () = state.scheduler_wake().notified() => continue,
            () = tokio::time::sleep(delay) => {}
        }

        let fired_at = Utc::now();
        for (definition, occurrence) in upcoming {
            if occurrence > fired_at {
                continue;
            }
            let schedule_id = definition.schedule_id;
            if let Err(error) = trigger_schedule(&state, definition).await {
                if error.kind().status().is_server_error() {
                    tracing::error!(
                        schedule_id = %schedule_id,
                        error.code = error.kind().code(),
                        "scheduled agent turn failed"
                    );
                } else {
                    tracing::warn!(
                        schedule_id = %schedule_id,
                        error.code = error.kind().code(),
                        "scheduled agent turn was rejected"
                    );
                }
            }
        }
    }
}

#[tracing::instrument(
    name = "scheduler.trigger",
    skip(state, definition),
    fields(
        schedule_id = %definition.schedule_id,
        agent_name = %definition.agent_name,
        agent_runtime_id = tracing::field::Empty,
        agent_id = tracing::field::Empty,
        session_id = tracing::field::Empty,
        turn_id = tracing::field::Empty
    )
)]
async fn trigger_schedule(
    state: &Arc<AppState>,
    definition: ScheduleDefinition,
) -> Result<(), ApiError> {
    let _admission = state.admission().enter()?;
    let session_id = SessionId::new();
    let idempotency_key = uuid::Uuid::now_v7();
    tracing::Span::current().record("session_id", tracing::field::display(session_id));
    state
        .pg()
        .begin_schedule_run(BeginScheduleRun {
            schedule_id: definition.schedule_id,
            session_id,
            idempotency_key,
            triggered_at: Utc::now(),
        })
        .await
        .map_err(ApiError::from_postgres)?;

    let created = match create_agent_runtime_from_template(
        state,
        idempotency_key,
        definition.agent_name,
        None,
    )
    .await
    {
        Ok(created) => created,
        Err(error) => {
            if !requires_restart_reconciliation(error.kind()) {
                finish_failed(state, definition.schedule_id, session_id, None).await?;
            }
            return Err(error);
        }
    };
    tracing::Span::current().record(
        "agent_runtime_id",
        tracing::field::display(created.agent_runtime_id),
    );
    tracing::Span::current().record("agent_id", tracing::field::display(created.agent_id));

    let accepted = match admit_message(
        state,
        created.agent_runtime_id,
        MessageRequest {
            text: SCHEDULE_TRIGGER_MESSAGE.to_owned(),
            expected_current_turn_id: None,
            session_id: Some(session_id),
            model_config: None,
        },
    )
    .await
    {
        Ok(accepted) => accepted,
        Err(error) => {
            if !requires_restart_reconciliation(error.kind()) {
                finish_failed(
                    state,
                    definition.schedule_id,
                    session_id,
                    Some((created.agent_runtime_id, created.agent_id)),
                )
                .await?;
            }
            return Err(error);
        }
    };
    state
        .pg()
        .finish_schedule_run(FinishScheduleRun {
            schedule_id: definition.schedule_id,
            session_id,
            status: ScheduleRunStatus::Accepted,
            agent_runtime_id: Some(created.agent_runtime_id),
            agent_id: Some(created.agent_id),
            turn_id: Some(accepted.turn_id),
        })
        .await
        .map_err(ApiError::from_postgres)?;
    tracing::Span::current().record("turn_id", tracing::field::display(accepted.turn_id));
    tracing::info!("scheduled agent turn accepted");
    Ok(())
}

fn requires_restart_reconciliation(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::StoreUnavailable | ErrorKind::ServiceShuttingDown
    )
}

async fn finish_failed(
    state: &AppState,
    schedule_id: ScheduleId,
    session_id: SessionId,
    runtime: Option<(stratum_core::AgentRuntimeId, stratum_core::AgentId)>,
) -> Result<(), ApiError> {
    let (agent_runtime_id, agent_id) = runtime.unzip();
    state
        .pg()
        .finish_schedule_run(FinishScheduleRun {
            schedule_id,
            session_id,
            status: ScheduleRunStatus::Failed,
            agent_runtime_id,
            agent_id,
            turn_id: None,
        })
        .await
        .map_err(ApiError::from_postgres)
}

async fn load_definitions(state: &AppState) -> Result<Vec<ScheduleDefinition>, ApiError> {
    state
        .pg()
        .read_scheduler_definitions()
        .await
        .map_err(ApiError::from_postgres)
}

fn next_run_at(
    definition: &ScheduleDefinition,
    after: DateTime<Utc>,
) -> Result<DateTime<Utc>, ApiError> {
    let schedule = parse_cron(&definition.cron_expression)?;
    schedule
        .after(&after.with_timezone(&Local))
        .next()
        .map(|next| next.with_timezone(&Utc))
        .ok_or_else(|| ApiError::new(ErrorKind::CronHasNoFutureOccurrence))
}

fn canonical_cron(expression: &str) -> Result<String, ApiError> {
    let fields = expression.split_whitespace().collect::<Vec<_>>();
    if !(5..=7).contains(&fields.len()) {
        return Err(invalid_cron_field_count());
    }
    let canonical = fields.join(" ");
    parse_cron(&canonical)?;
    Ok(canonical)
}

fn parse_cron(expression: &str) -> Result<Schedule, ApiError> {
    let fields = expression.split_whitespace().collect::<Vec<_>>();
    let normalized = match fields.len() {
        5 => format!("0 {expression}"),
        6 | 7 => expression.to_owned(),
        _ => return Err(invalid_cron_field_count()),
    };
    Schedule::from_str(&normalized)
        .map_err(|source| ApiError::with_source(ErrorKind::InvalidCronExpression, source))
}

fn invalid_cron_field_count() -> ApiError {
    let source: cron::error::Error = cron::error::ErrorKind::Expression(
        "cron expression must contain five to seven fields".to_owned(),
    )
    .into();
    ApiError::with_source(ErrorKind::InvalidCronExpression, source)
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn standard_five_field_cron_is_canonical_and_schedulable() {
        let canonical = canonical_cron("  */5   * * * * ").expect("cron expression is valid");
        let schedule = parse_cron(&canonical).expect("canonical cron expression is valid");
        let after = Utc
            .with_ymd_and_hms(2026, 8, 19, 1, 1, 1)
            .single()
            .expect("timestamp is valid");

        let next = schedule
            .after(&after.with_timezone(&Local))
            .next()
            .expect("next occurrence exists")
            .with_timezone(&Utc);

        assert_eq!(canonical, "*/5 * * * *");
        assert!(next > after);
    }

    #[test]
    fn malformed_cron_preserves_the_parser_source() {
        let error = canonical_cron("not a cron").expect_err("cron expression is invalid");

        assert_eq!(error.kind(), ErrorKind::InvalidCronExpression);
        assert!(std::error::Error::source(&error).is_some());
    }
}
