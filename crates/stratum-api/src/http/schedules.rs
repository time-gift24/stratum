//! Recurring schedule definition and occurrence-history handlers.

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::{Json, Router};
use serde::Deserialize;
use stratum_core::{AgentName, ScheduleId};
use stratum_postgres::{
    AgentStatus, SchedulePageQuery, ScheduleRun, ScheduleRunStatus, ScheduleRunsQuery,
};
use tracing::{Span, field};
use utoipa::{IntoParams, ToSchema};

use super::json_request;
use crate::dto::{
    CreateScheduleRequest, Pagination, ScheduleSessionStatus, ScheduleSessionView,
    ScheduleSessionsPage, ScheduleView, SchedulesPage,
};
use crate::error::{ApiError, ErrorKind, ErrorResponse};
use crate::scheduler::{create_schedule as create_schedule_definition, schedule_view};
use crate::state::AppState;

const DEFAULT_PAGE: u32 = 1;
const DEFAULT_PER_PAGE: u32 = 20;
const MAX_PER_PAGE: u32 = 100;

/// Standard page query for schedule resources.
#[derive(Debug, Clone, Deserialize, IntoParams, ToSchema)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct PageParams {
    /// One-based page number.
    #[serde(default = "default_page")]
    page: u32,
    /// Page size, 1 through 100.
    #[serde(default = "default_per_page")]
    per_page: u32,
    /// Only the endpoint's documented descending creation/trigger order is accepted.
    #[serde(default)]
    sort: Option<String>,
}

/// Typed schedule path identity owned by the HTTP boundary.
#[derive(Debug, Clone, Copy, Deserialize, IntoParams, ToSchema)]
#[into_params(parameter_in = Path)]
pub(crate) struct SchedulePath {
    /// Recurring schedule identity.
    schedule_id: ScheduleId,
}

const fn default_page() -> u32 {
    DEFAULT_PAGE
}

const fn default_per_page() -> u32 {
    DEFAULT_PER_PAGE
}

/// Schedule routes merged into the API host.
pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/v1/schedules", axum::routing::get(list).post(create))
        .route("/v1/schedules/{schedule_id}", axum::routing::get(get))
        .route(
            "/v1/schedules/{schedule_id}/sessions",
            axum::routing::get(list_sessions),
        )
}

/// Creates one recurring Agent schedule.
#[utoipa::path(
    post,
    path = "/v1/schedules",
    request_body = CreateScheduleRequest,
    responses(
        (status = 201, description = "schedule created", body = ScheduleView),
        (status = 400, description = "request body or Agent name is invalid", body = ErrorResponse),
        (status = 404, description = "Agent template not found", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "cron expression, template, model, or tools are invalid", body = ErrorResponse),
        (status = 500, description = "catalog or persisted scheduler state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn create(
    State(state): State<Arc<AppState>>,
    request: Result<Json<CreateScheduleRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<ScheduleView>), ApiError> {
    let body = json_request(request)?;
    let agent_name: AgentName = body
        .agent_name
        .parse()
        .map_err(|source| ApiError::with_source(ErrorKind::InvalidRequest, source))?;
    Span::current().record("agent_name", agent_name.as_str());
    let created = create_schedule_definition(&state, agent_name, body.cron_expression).await?;
    Span::current().record("schedule_id", field::display(created.schedule_id));
    Ok((StatusCode::CREATED, Json(created)))
}

/// Lists schedules newest first.
#[utoipa::path(
    get,
    path = "/v1/schedules",
    params(PageParams),
    responses(
        (status = 200, description = "schedule page", body = SchedulesPage),
        (status = 400, description = "pagination or sort query is invalid", body = ErrorResponse),
        (status = 500, description = "persisted schedule state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn list(
    State(state): State<Arc<AppState>>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<SchedulesPage>, ApiError> {
    let (params, query) = page_query(params, "-created_at")?;
    let page = state
        .pg()
        .read_schedules(query)
        .await
        .map_err(ApiError::from_postgres)?;
    let data = page
        .items
        .into_iter()
        .map(schedule_view)
        .collect::<Result<_, _>>()?;
    Ok(Json(SchedulesPage {
        data,
        pagination: Pagination {
            page: params.page,
            per_page: params.per_page,
            total: page.total,
        },
    }))
}

/// Reads one schedule definition.
#[utoipa::path(
    get,
    path = "/v1/schedules/{schedule_id}",
    params(SchedulePath),
    responses(
        (status = 200, description = "schedule detail", body = ScheduleView),
        (status = 400, description = "schedule identity is malformed", body = ErrorResponse),
        (status = 404, description = "schedule not found", body = ErrorResponse),
        (status = 500, description = "persisted schedule state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get(
    State(state): State<Arc<AppState>>,
    path: Result<Path<SchedulePath>, PathRejection>,
) -> Result<Json<ScheduleView>, ApiError> {
    let Path(SchedulePath { schedule_id }) =
        path.map_err(|source| ApiError::with_source(ErrorKind::InvalidRequest, source))?;
    Span::current().record("schedule_id", field::display(schedule_id));
    let definition = state
        .pg()
        .read_schedule(schedule_id)
        .await
        .map_err(ApiError::from_postgres)?;
    Ok(Json(schedule_view(definition)?))
}

/// Lists one schedule's occurrence history newest first.
#[utoipa::path(
    get,
    path = "/v1/schedules/{schedule_id}/sessions",
    params(
        SchedulePath,
        PageParams,
    ),
    responses(
        (status = 200, description = "scheduled conversation page", body = ScheduleSessionsPage),
        (status = 400, description = "identity, pagination, or sort query is invalid", body = ErrorResponse),
        (status = 404, description = "schedule not found", body = ErrorResponse),
        (status = 500, description = "persisted scheduler/runtime state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn list_sessions(
    State(state): State<Arc<AppState>>,
    path: Result<Path<SchedulePath>, PathRejection>,
    params: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<ScheduleSessionsPage>, ApiError> {
    let Path(SchedulePath { schedule_id }) =
        path.map_err(|source| ApiError::with_source(ErrorKind::InvalidRequest, source))?;
    Span::current().record("schedule_id", field::display(schedule_id));
    let (params, page_query) = page_query(params, "-triggered_at")?;
    let page = state
        .pg()
        .read_schedule_runs(ScheduleRunsQuery {
            schedule_id,
            offset: page_query.offset,
            limit: page_query.limit,
        })
        .await
        .map_err(ApiError::from_postgres)?;
    let data = page
        .items
        .into_iter()
        .map(schedule_session_view)
        .collect::<Result<_, _>>()?;
    Ok(Json(ScheduleSessionsPage {
        data,
        pagination: Pagination {
            page: params.page,
            per_page: params.per_page,
            total: page.total,
        },
    }))
}

fn page_query(
    params: Result<Query<PageParams>, QueryRejection>,
    expected_sort: &str,
) -> Result<(PageParams, SchedulePageQuery), ApiError> {
    let Query(params) =
        params.map_err(|source| ApiError::with_source(ErrorKind::InvalidPagination, source))?;
    if params.page == 0
        || !(1..=MAX_PER_PAGE).contains(&params.per_page)
        || params
            .sort
            .as_deref()
            .is_some_and(|sort| sort != expected_sort)
    {
        return Err(ApiError::new(ErrorKind::InvalidPagination));
    }
    let offset = u64::from(params.page - 1)
        .checked_mul(u64::from(params.per_page))
        .ok_or_else(|| ApiError::new(ErrorKind::InvalidPagination))?;
    let query = SchedulePageQuery {
        offset,
        limit: params.per_page,
    };
    Ok((params, query))
}

fn schedule_session_view(run: ScheduleRun) -> Result<ScheduleSessionView, ApiError> {
    let (status, conversation_available) = match run.status {
        ScheduleRunStatus::Starting => (ScheduleSessionStatus::Starting, false),
        ScheduleRunStatus::Failed => (ScheduleSessionStatus::Failed, false),
        ScheduleRunStatus::Accepted => {
            let status = match run.runtime_status {
                Some(AgentStatus::Running) => ScheduleSessionStatus::Running,
                Some(AgentStatus::Finished) => ScheduleSessionStatus::Finished,
                Some(AgentStatus::Failed) => ScheduleSessionStatus::Failed,
                Some(AgentStatus::Cancelled) => ScheduleSessionStatus::Cancelled,
                Some(AgentStatus::Idle) | None => {
                    return Err(ApiError::new(ErrorKind::DurableStateCorrupt));
                }
                Some(_) => return Err(ApiError::new(ErrorKind::Internal)),
            };
            (status, true)
        }
        _ => return Err(ApiError::new(ErrorKind::Internal)),
    };
    if conversation_available
        && (run.agent_runtime_id.is_none() || run.agent_id.is_none() || run.turn_id.is_none())
    {
        return Err(ApiError::new(ErrorKind::DurableStateCorrupt));
    }
    Ok(ScheduleSessionView {
        schedule_id: run.schedule_id,
        session_id: run.session_id,
        agent_runtime_id: run.agent_runtime_id,
        agent_id: run.agent_id,
        status,
        conversation_available,
        triggered_at: run.triggered_at,
        updated_at: run.updated_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_query_rejects_unknown_fields_and_wrong_sort() {
        let unknown: http::Uri = "/v1/schedules?page=1&replay=all"
            .parse()
            .expect("test URI parses");
        assert!(page_query(Query::<PageParams>::try_from_uri(&unknown), "-created_at").is_err());

        let wrong_sort: http::Uri = "/v1/schedules?sort=created_at"
            .parse()
            .expect("test URI parses");
        assert!(
            page_query(
                Query::<PageParams>::try_from_uri(&wrong_sort),
                "-created_at"
            )
            .is_err()
        );
    }

    #[test]
    fn page_query_computes_bounded_offset() {
        let uri: http::Uri = "/v1/schedules?page=3&per_page=20&sort=-created_at"
            .parse()
            .expect("test URI parses");

        let (params, query) = page_query(Query::<PageParams>::try_from_uri(&uri), "-created_at")
            .expect("query is valid");

        assert_eq!(params.page, 3);
        assert_eq!(query.offset, 40);
        assert_eq!(query.limit, 20);
    }
}
