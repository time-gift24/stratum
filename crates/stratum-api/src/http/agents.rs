//! Agent catalog, creation, cold view, and history handlers.

use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, extract::rejection::JsonRejection};
use stratum_config::AgentName;
use stratum_core::ModelConfig;
use stratum_llm::LlmError;
use stratum_postgres::{
    AgentRuntimeView as StoredAgentRuntimeView, CreateAgentRuntime, HISTORY_DEFAULT_LIMIT,
    HISTORY_MAX_LIMIT, HistoryQuery, ResolvedDefinitionV1, encode_event_seq, parse_event_seq,
};
use tracing::{Span, field};
use utoipa::IntoParams;

use super::{json_request, parse_agent_runtime_id};
use crate::dto::{
    AgentRuntimeCreated, AgentRuntimeStatusDto, AgentRuntimeView, AgentTemplateDto,
    AgentTemplatesResponse, CreateAgentRuntimeRequest, HistoryItemDto, HistoryResponse,
    ModelsResponse, PendingApprovalDto,
};
use crate::error::{ApiError, ErrorKind, ErrorResponse};
use crate::frames::history_item_event;
use crate::state::AppState;
use crate::turn::build_tool_registry;

/// Creates an AgentRuntime, idempotently keyed by the client.
#[utoipa::path(
    post,
    path = "/v1/agent-runtimes",
    request_body = CreateAgentRuntimeRequest,
    params(("Idempotency-Key" = String, Header, description = "client-generated UUID idempotency key")),
    responses(
        (status = 201, description = "agent runtime created (or key-only replayed)", body = AgentRuntimeCreated,
            headers(("Location" = String, description = "canonical URI of the created agent runtime"))),
        (status = 400, description = "missing/invalid idempotency key or request body", body = ErrorResponse),
        (status = 404, description = "agent template not found", body = ErrorResponse),
        (status = 409, description = "the exact template name/version tag conflicts with another immutable definition", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "template or model validation failed", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt or an internal error occurred", body = ErrorResponse),
        (status = 503, description = "store unavailable or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn create_agent_runtime(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    request: Result<Json<CreateAgentRuntimeRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let idempotency_key = parse_idempotency_key(&headers)?;
    let body = json_request(request)?;
    // Key-first: an idempotent replay answers from the stored record alone,
    // without reading the template catalog.
    if let Some(existing) = state
        .pg()
        .find_agent_runtime_by_idempotency_key(idempotency_key)
        .await
        .map_err(ApiError::from_postgres)?
    {
        return Ok(created_response(
            existing.agent_runtime_id,
            existing.agent_id,
            existing.agent_name,
            existing.agent_version,
            existing.created_at,
        ));
    }

    // Miss: hot-read and validate the current template, then preflight the
    // model and tools before any durable mutation.
    let agent_name: AgentName = body
        .agent_name
        .parse()
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    Span::current().record("agent_name", agent_name.as_str());
    let definition = state.resolve_agent_definition(&agent_name).await?;
    let template_model = definition.model.clone();
    let effective_model = match &body.model_config {
        Some(overridden) => {
            validate_model_override(&state, overridden)?;
            overridden.clone()
        }
        None => template_model.clone(),
    };
    build_tool_registry(&definition.tools)
        .map_err(|_| ApiError::new(ErrorKind::InvalidAgentTemplate))?;

    let resolved = ResolvedDefinitionV1 {
        model: template_model,
        tools: definition.tools.clone(),
        prompt: definition.prompt.clone(),
    };

    let outcome = state
        .pg()
        .create_agent_runtime(CreateAgentRuntime {
            idempotency_key,
            name: agent_name.as_str().to_owned(),
            version: definition.agent_version,
            resolved_definition: resolved,
            model_config: effective_model,
        })
        .await
        .map_err(ApiError::from_postgres)?;
    let runtime = outcome.runtime();
    Ok(created_response(
        runtime.agent_runtime_id,
        runtime.agent_id,
        runtime.agent_name.clone(),
        runtime.agent_version.clone(),
        runtime.created_at,
    ))
}

/// 201 + Location + the stored representation, identical on replay.
fn created_response(
    agent_runtime_id: stratum_core::AgentRuntimeId,
    agent_id: stratum_core::AgentId,
    agent_name: String,
    agent_version: stratum_core::AgentVersionTag,
    created_at: chrono::DateTime<chrono::Utc>,
) -> Response {
    let body = AgentRuntimeCreated {
        agent_runtime_id,
        agent_id,
        agent_name,
        agent_version,
        created_at,
    };
    (
        StatusCode::CREATED,
        [("Location", format!("/v1/agent-runtimes/{agent_runtime_id}"))],
        Json(body),
    )
        .into_response()
}

/// Validates a full-replacement model override before any durable mutation.
pub(crate) fn validate_model_override(
    state: &AppState,
    model_config: &ModelConfig,
) -> Result<(), ApiError> {
    state
        .providers()
        .configure(model_config)
        .map(|_| ())
        .map_err(|error| match error {
            LlmError::ProviderNotFound { .. } => ApiError::new(ErrorKind::ModelNotConfigured),
            LlmError::InvalidModelParameters { .. } => {
                ApiError::new(ErrorKind::InvalidModelParameters)
            }
            other => ApiError::with_source(ErrorKind::Internal, other),
        })
}

/// Parses the required `Idempotency-Key` header.
fn parse_idempotency_key(headers: &HeaderMap) -> Result<uuid::Uuid, ApiError> {
    let value = headers
        .get("Idempotency-Key")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::new(ErrorKind::InvalidRequest))?;
    uuid::Uuid::parse_str(value).map_err(|_| ApiError::new(ErrorKind::InvalidRequest))
}

/// Lists the current valid template catalog (all-or-nothing).
#[utoipa::path(
    get,
    path = "/v1/agent-templates",
    responses(
        (status = 200, description = "current template catalog", body = AgentTemplatesResponse),
        (status = 422, description = "at least one template is unreadable or invalid", body = ErrorResponse),
    )
)]
pub(crate) async fn list_agent_templates(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AgentTemplatesResponse>, ApiError> {
    let templates: Vec<AgentTemplateDto> = state.list_agent_templates().await?;
    Ok(Json(AgentTemplatesResponse { templates }))
}

/// Lists the configured model catalog with parameter schemas.
#[utoipa::path(
    get,
    path = "/v1/models",
    responses(
        (status = 200, description = "configured model catalog", body = ModelsResponse),
    )
)]
pub(crate) async fn list_models(State(state): State<Arc<AppState>>) -> Json<ModelsResponse> {
    Json(ModelsResponse {
        models: state.providers().models(),
    })
}

/// Cold AgentRuntime view at a fixed Postgres barrier.
#[utoipa::path(
    get,
    path = "/v1/agent-runtimes/{agent_runtime_id}",
    params(("agent_runtime_id" = String, Path, description = "agent runtime identity")),
    responses(
        (status = 200, description = "agent runtime view at the current barrier", body = AgentRuntimeView),
        (status = 400, description = "malformed agent runtime identity", body = ErrorResponse),
        (status = 404, description = "agent runtime not found", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_agent_runtime(
    State(state): State<Arc<AppState>>,
    Path(agent_runtime_id): Path<String>,
) -> Result<Json<AgentRuntimeView>, ApiError> {
    let agent_runtime_id = parse_agent_runtime_id(&agent_runtime_id)?;
    Span::current().record("agent_runtime_id", field::display(agent_runtime_id));
    let view = state
        .pg()
        .read_agent_runtime_view(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    Ok(Json(agent_view_response(&state, &view)?))
}

/// Maps the store view to the API DTO, adding the process-local advisory.
pub(crate) fn agent_view_response(
    state: &AppState,
    view: &StoredAgentRuntimeView,
) -> Result<AgentRuntimeView, ApiError> {
    let status = map_status(view.status)?;
    let resume_required = match (status, view.current_turn_id) {
        (AgentRuntimeStatusDto::Running, Some(turn_id)) => state
            .registry()
            .claim_state(view.agent_runtime_id, turn_id)
            .is_none(),
        (AgentRuntimeStatusDto::Running, None) => {
            return Err(ApiError::new(ErrorKind::DurableStateCorrupt));
        }
        (
            AgentRuntimeStatusDto::Idle
            | AgentRuntimeStatusDto::Finished
            | AgentRuntimeStatusDto::Failed
            | AgentRuntimeStatusDto::Cancelled,
            _,
        ) => false,
    };
    Ok(AgentRuntimeView {
        agent_runtime_id: view.agent_runtime_id,
        agent_id: view.agent_id,
        agent_name: view.agent_name.clone(),
        agent_version: view.agent_version.clone(),
        status,
        model_config: view.model_config.clone(),
        session_id: view.session_id,
        current_turn_id: view.current_turn_id,
        snapshot_event_seq: encode_event_seq(view.snapshot_event_seq),
        telemetry_floor_event_seq: encode_event_seq(view.telemetry_floor_event_seq),
        pending_approvals: view
            .pending_approvals
            .iter()
            .map(|approval| PendingApprovalDto {
                requested_event_seq: encode_event_seq(approval.requested_event_seq),
                approval_id: approval.approval_id,
                call_id: approval.call_id.clone(),
                tool_name: approval.tool_name.clone(),
                arguments: approval.arguments.clone(),
                tool_kind: approval.tool_kind,
                danger_level: approval.danger_level,
            })
            .collect(),
        latest_usage: view.latest_usage,
        resume_required,
    })
}

/// The store already fails closed on unknown status text; a variant outside
/// the closed set here is an internal invariant violation.
fn map_status(status: stratum_postgres::AgentStatus) -> Result<AgentRuntimeStatusDto, ApiError> {
    match status {
        stratum_postgres::AgentStatus::Idle => Ok(AgentRuntimeStatusDto::Idle),
        stratum_postgres::AgentStatus::Running => Ok(AgentRuntimeStatusDto::Running),
        stratum_postgres::AgentStatus::Finished => Ok(AgentRuntimeStatusDto::Finished),
        stratum_postgres::AgentStatus::Failed => Ok(AgentRuntimeStatusDto::Failed),
        stratum_postgres::AgentStatus::Cancelled => Ok(AgentRuntimeStatusDto::Cancelled),
        _ => Err(ApiError::new(ErrorKind::Internal)),
    }
}

/// History query parameters; all sequences are decimal strings.
#[derive(Debug, serde::Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct HistoryParams {
    /// Inclusive barrier of the fixed window (required).
    through_event_seq: Option<String>,
    /// Exclusive upper cursor from a previous page.
    before_event_seq: Option<String>,
    /// Page size (default 50, maximum 256).
    limit: Option<u32>,
}

/// Reads one ascending product-history page from the durable ledger.
#[utoipa::path(
    get,
    path = "/v1/agent-runtimes/{agent_runtime_id}/history",
    params(
        ("agent_runtime_id" = String, Path, description = "agent runtime identity"),
        HistoryParams,
    ),
    responses(
        (status = 200, description = "ascending history page", body = HistoryResponse),
        (status = 400, description = "invalid history query or runtime identity", body = ErrorResponse),
        (status = 404, description = "agent runtime not found", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(agent_runtime_id): Path<String>,
    params: Result<Query<HistoryParams>, QueryRejection>,
) -> Result<Json<HistoryResponse>, ApiError> {
    let params = history_query(params)?;
    let agent_runtime_id = parse_agent_runtime_id(&agent_runtime_id)?;
    Span::current().record("agent_runtime_id", field::display(agent_runtime_id));

    let invalid = || ApiError::new(ErrorKind::InvalidHistoryQuery);
    let through = params
        .through_event_seq
        .as_deref()
        .and_then(parse_event_seq)
        .ok_or_else(invalid)?;
    let before = params
        .before_event_seq
        .as_deref()
        .map(|value| parse_event_seq(value).ok_or_else(invalid))
        .transpose()?;
    let limit = match params.limit {
        None => HISTORY_DEFAULT_LIMIT,
        Some(limit) if (1..=HISTORY_MAX_LIMIT).contains(&limit) => limit,
        Some(_) => return Err(invalid()),
    };
    if before.is_some_and(|before| before > through) {
        return Err(invalid());
    }

    let runtime_state = state
        .pg()
        .read_agent_runtime_state(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    if through > runtime_state.last_event_seq {
        return Err(invalid());
    }

    let page = state
        .pg()
        .read_history_page(HistoryQuery {
            agent_runtime_id,
            through_event_seq: through,
            before_event_seq: before,
            limit,
        })
        .await
        .map_err(ApiError::from_postgres)?;

    let mut items = Vec::with_capacity(page.items.len());
    for item in &page.items {
        let event = history_item_event(item)
            .map_err(|source| ApiError::with_source(ErrorKind::DurableStateCorrupt, source))?
            .ok_or_else(|| ApiError::new(ErrorKind::DurableStateCorrupt))?;
        items.push(HistoryItemDto {
            event_seq: encode_event_seq(item.event_seq),
            event_version: item.event_version,
            session_id: item.session_id,
            turn_id: item.turn_id,
            created_at: item.created_at,
            event,
        });
    }

    Ok(Json(HistoryResponse {
        items,
        through_event_seq: encode_event_seq(through),
        next_before_event_seq: page.next_before_event_seq.map(encode_event_seq),
        has_more: page.has_more,
    }))
}

fn history_query(
    params: Result<Query<HistoryParams>, QueryRejection>,
) -> Result<HistoryParams, ApiError> {
    params
        .map(|Query(params)| params)
        .map_err(|_| ApiError::new(ErrorKind::InvalidHistoryQuery))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_query_rejects_unknown_fields_with_the_stable_kind() {
        let uri: http::Uri = "/v1/agent-runtimes/id/history?through_event_seq=4&replay=all"
            .parse()
            .expect("test URI parses");

        let error = history_query(Query::<HistoryParams>::try_from_uri(&uri))
            .expect_err("unknown query is rejected");

        assert_eq!(error.kind(), ErrorKind::InvalidHistoryQuery);
    }
}
