//! Agent catalog, creation, cold view, and history handlers.

use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, extract::rejection::JsonRejection};
use stratum_config::AgentName;
use stratum_core::{AgentId, ModelConfig};
use stratum_llm::LlmError;
use stratum_postgres::{
    AgentView, CreateAgent, CreateAgentOutcome, HISTORY_DEFAULT_LIMIT, HISTORY_MAX_LIMIT,
    HistoryQuery, encode_event_seq, parse_event_seq,
};
use tracing::{Span, field};
use utoipa::IntoParams;

use super::{json_request, parse_agent_id};
use crate::dto::{
    AgentStatusDto, AgentTemplateDto, AgentTemplatesResponse, AgentViewResponse,
    CreateAgentRequest, CreateAgentResponse, HistoryItemDto, HistoryResponse, ModelsResponse,
    PendingApprovalDto,
};
use crate::error::{ApiError, ErrorKind, ErrorResponse};
use crate::frames::history_item_event;
use crate::state::AppState;
use crate::turn::{ResolvedDefinitionV1, build_tool_registry};

/// Creates an immutable Agent, idempotently keyed by the client.
#[utoipa::path(
    post,
    path = "/v1/agents",
    request_body = CreateAgentRequest,
    params(("Idempotency-Key" = String, Header, description = "client-generated UUID idempotency key")),
    responses(
        (status = 201, description = "agent created (or identically replayed)", body = CreateAgentResponse,
            headers(("Location" = String, description = "canonical URI of the created agent"))),
        (status = 400, description = "missing/invalid idempotency key or request body", body = ErrorResponse),
        (status = 404, description = "agent template not found", body = ErrorResponse),
        (status = 409, description = "idempotency key is bound to a different create request", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "template or model validation failed", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt or an internal error occurred", body = ErrorResponse),
        (status = 503, description = "store unavailable or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn create_agent(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    request: Result<Json<CreateAgentRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let idempotency_key = parse_idempotency_key(&headers)?;
    let body = json_request(request)?;
    let agent_name: AgentName = body
        .agent_name
        .parse()
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    Span::current().record("agent_name", agent_name.as_str());

    // Key-first: an idempotent replay answers from the stored record alone,
    // without reading the template catalog.
    if let Some(existing) = state
        .pg()
        .find_agent_by_idempotency_key(idempotency_key)
        .await
        .map_err(ApiError::from_postgres)?
    {
        if existing.source_template_name == agent_name.as_str()
            && existing.creation_model_override == body.model_config
        {
            let view = state
                .pg()
                .read_agent_view(existing.agent_id)
                .await
                .map_err(ApiError::from_postgres)?;
            return Ok(created_response(&view));
        }
        return Err(ApiError::new(ErrorKind::IdempotencyKeyConflict));
    }

    // Miss: hot-read and validate the current template, then preflight the
    // model and tools before any durable mutation.
    let definition = state.templates().resolve(&agent_name).await?;
    let effective_model = match &body.model_config {
        Some(overridden) => {
            validate_model_override(&state, overridden)?;
            overridden.clone()
        }
        None => state
            .providers()
            .default_model_config(&definition.model)
            .map_err(|_| ApiError::new(ErrorKind::ModelNotConfigured))?,
    };
    build_tool_registry(&definition.tools)
        .map_err(|_| ApiError::new(ErrorKind::InvalidAgentTemplate))?;

    let resolved = ResolvedDefinitionV1 {
        agent_name: agent_name.as_str().to_owned(),
        model: effective_model.clone(),
        tools: definition.tools.clone(),
        prompt: definition.prompt.clone(),
    };
    let resolved_definition = serde_json::to_value(&resolved)
        .map_err(|source| ApiError::with_source(ErrorKind::Internal, source))?;

    let outcome = state
        .pg()
        .create_agent(CreateAgent {
            agent_id: AgentId::new(),
            agent_version_id: stratum_core::AgentVersionId::new(),
            idempotency_key,
            source_template_name: agent_name.as_str().to_owned(),
            creation_model_override: body.model_config,
            resolved_definition,
            default_model_config: effective_model,
        })
        .await
        .map_err(ApiError::from_postgres)?;
    let agent_id = match outcome {
        CreateAgentOutcome::Created { agent_id } | CreateAgentOutcome::Replay { agent_id } => {
            agent_id
        }
        _ => return Err(ApiError::new(ErrorKind::Internal)),
    };
    let view = state
        .pg()
        .read_agent_view(agent_id)
        .await
        .map_err(ApiError::from_postgres)?;
    Ok(created_response(&view))
}

/// 201 + Location + the stored representation, identical on replay.
fn created_response(view: &AgentView) -> Response {
    let body = CreateAgentResponse {
        agent_id: view.agent_id,
        agent_name: view.source_template_name.clone(),
        model_config: view.default_model_config.clone(),
        created_at: view.created_at,
    };
    (
        StatusCode::CREATED,
        [("Location", format!("/v1/agents/{}", view.agent_id))],
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
    let templates: Vec<AgentTemplateDto> = state.templates().list(state.providers()).await?;
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

/// Cold Agent view at a fixed Postgres barrier.
#[utoipa::path(
    get,
    path = "/v1/agents/{agent_id}",
    params(("agent_id" = String, Path, description = "agent identity")),
    responses(
        (status = 200, description = "agent view at the current barrier", body = AgentViewResponse),
        (status = 400, description = "malformed agent identity", body = ErrorResponse),
        (status = 404, description = "agent not found", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_agent(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentViewResponse>, ApiError> {
    let agent_id = parse_agent_id(&agent_id)?;
    Span::current().record("agent_id", field::display(agent_id));
    let view = state
        .pg()
        .read_agent_view(agent_id)
        .await
        .map_err(ApiError::from_postgres)?;
    Ok(Json(agent_view_response(&state, &view)?))
}

/// Maps the store view to the API DTO, adding the process-local advisory.
pub(crate) fn agent_view_response(
    state: &AppState,
    view: &AgentView,
) -> Result<AgentViewResponse, ApiError> {
    let resume_required = match (view.status, view.current_turn_id) {
        (stratum_postgres::AgentStatus::Running, Some(turn_id)) => state
            .registry()
            .claim_state(view.agent_id, turn_id)
            .is_none(),
        _ => false,
    };
    Ok(AgentViewResponse {
        agent_id: view.agent_id,
        agent_name: view.source_template_name.clone(),
        status: map_status(view.status)?,
        model_config: view.default_model_config.clone(),
        session_id: view.session_id,
        current_turn_id: view.current_turn_id,
        snapshot_event_seq: encode_event_seq(view.snapshot_event_seq),
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
fn map_status(status: stratum_postgres::AgentStatus) -> Result<AgentStatusDto, ApiError> {
    match status {
        stratum_postgres::AgentStatus::Idle => Ok(AgentStatusDto::Idle),
        stratum_postgres::AgentStatus::Running => Ok(AgentStatusDto::Running),
        stratum_postgres::AgentStatus::Finished => Ok(AgentStatusDto::Finished),
        stratum_postgres::AgentStatus::Failed => Ok(AgentStatusDto::Failed),
        stratum_postgres::AgentStatus::Cancelled => Ok(AgentStatusDto::Cancelled),
        _ => Err(ApiError::new(ErrorKind::Internal)),
    }
}

/// History query parameters; all sequences are decimal strings.
#[derive(Debug, serde::Deserialize, IntoParams)]
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
    path = "/v1/agents/{agent_id}/history",
    params(
        ("agent_id" = String, Path, description = "agent identity"),
        HistoryParams,
    ),
    responses(
        (status = 200, description = "ascending history page", body = HistoryResponse),
        (status = 400, description = "invalid history query or agent identity", body = ErrorResponse),
        (status = 404, description = "agent not found", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_history(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
    Query(params): Query<HistoryParams>,
) -> Result<Json<HistoryResponse>, ApiError> {
    let agent_id = parse_agent_id(&agent_id)?;
    Span::current().record("agent_id", field::display(agent_id));

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

    let agent_state = state
        .pg()
        .read_agent_state(agent_id)
        .await
        .map_err(ApiError::from_postgres)?;
    if through > agent_state.last_event_seq {
        return Err(invalid());
    }

    let page = state
        .pg()
        .read_history_page(HistoryQuery {
            agent_id,
            through_event_seq: through,
            before_event_seq: before,
            limit,
        })
        .await
        .map_err(ApiError::from_postgres)?;

    let mut items = Vec::with_capacity(page.items.len());
    for item in &page.items {
        let event = history_item_event(item)
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
