//! HTTP surface: router, OpenAPI document, health endpoints, and shared
//! request helpers.

mod agents;
mod events;
mod turns;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{MatchedPath, Request, State};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use stratum_core::AgentId;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{Span, field, info_span};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::dto::{
    AgentTemplatesResponse, AgentViewResponse, CreateAgentRequest, CreateAgentResponse,
    HistoryResponse, LivenessResponse, ModelsResponse, ReadinessResponse, TurnAccepted,
};
use crate::error::{ApiError, ErrorKind, ErrorResponse};
use crate::state::AppState;

/// Hard limit for every JSON request body (64 KiB).
const JSON_BODY_LIMIT: usize = 64 * 1024;

/// OpenAPI document covering every endpoint of the API host.
#[derive(OpenApi)]
#[openapi(
    info(title = "Stratum API", description = "Stratum agent runtime HTTP API"),
    paths(
        agents::create_agent,
        agents::list_agent_templates,
        agents::list_models,
        agents::get_agent,
        agents::get_history,
        turns::post_message,
        turns::post_resume,
        turns::post_cancel,
        turns::post_approval,
        events::get_events,
        liveness,
        readiness,
    ),
    components(schemas(
        CreateAgentRequest,
        CreateAgentResponse,
        AgentTemplatesResponse,
        crate::dto::AgentTemplateDto,
        ModelsResponse,
        stratum_llm::ModelDescriptor,
        AgentViewResponse,
        crate::dto::AgentStatusDto,
        crate::dto::PendingApprovalDto,
        HistoryResponse,
        crate::dto::HistoryItemDto,
        crate::dto::MessageRequest,
        crate::dto::ResumeRequest,
        crate::dto::CancelRequest,
        crate::dto::ApprovalResolveRequest,
        TurnAccepted,
        crate::frames::AgentStreamFrameV1,
        crate::frames::ControlEventV1,
        crate::frames::StreamResetReason,
        crate::frames::AgentProductEventV1,
        crate::frames::LlmTelemetryEventV1,
        LivenessResponse,
        ReadinessResponse,
        ErrorResponse,
        crate::error::ErrorBody,
        stratum_core::ModelConfig,
        stratum_core::TokenUsage,
        stratum_core::ChatMessage,
        stratum_core::ChatContent,
        stratum_core::ChatRole,
        stratum_core::ToolCall,
        stratum_core::ToolKind,
        stratum_core::DangerLevel,
        stratum_core::ApprovalDecision,
    ))
)]
struct ApiDoc;

/// Builds the HTTP API router for one assembled state.
pub fn router(state: Arc<AppState>) -> Router {
    let origins = state
        .allowed_origins()
        .iter()
        .map(|origin| {
            // Invariant (programmer error): `stratum-config` validation
            // rejects any origin that is not a valid header value, so parsing
            // here can never fail.
            http::HeaderValue::from_str(origin)
                .expect("allowed origins are validated during config parsing")
        })
        .collect::<Vec<_>>();
    let router = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .route("/v1/agents", post(agents::create_agent))
        .route("/v1/agent-templates", get(agents::list_agent_templates))
        .route("/v1/models", get(agents::list_models))
        .route("/v1/agents/{agent_id}", get(agents::get_agent))
        .route("/v1/agents/{agent_id}/history", get(agents::get_history))
        .route("/v1/agents/{agent_id}/messages", post(turns::post_message))
        .route("/v1/agents/{agent_id}/resume", post(turns::post_resume))
        .route("/v1/agents/{agent_id}/cancel", post(turns::post_cancel))
        .route(
            "/v1/agents/{agent_id}/approvals/{approval_id}",
            post(turns::post_approval),
        )
        .route("/v1/agents/{agent_id}/events", get(events::get_events))
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness))
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(JSON_BODY_LIMIT))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &Request| {
                    let route = request
                        .extensions()
                        .get::<MatchedPath>()
                        .map_or("unmatched", MatchedPath::as_str);
                    info_span!(
                        "http.request",
                        route,
                        method = %request.method(),
                        agent_id = field::Empty,
                        session_id = field::Empty,
                        turn_id = field::Empty,
                        status = field::Empty,
                        latency = field::Empty,
                    )
                })
                .on_response(|response: &Response, latency: Duration, span: &Span| {
                    span.record("status", response.status().as_u16());
                    span.record("latency", field::debug(latency));
                }),
        );
    if origins.is_empty() {
        router
    } else {
        router.layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_methods([http::Method::GET, http::Method::POST])
                // Browser clients send `Idempotency-Key` on create and
                // `Last-Event-ID` on SSE reconnect; both must survive the
                // preflight allowlist.
                .allow_headers([
                    http::header::CONTENT_TYPE,
                    http::HeaderName::from_static("last-event-id"),
                    http::HeaderName::from_static("idempotency-key"),
                ]),
        )
    }
}

/// Parses a JSON body, mapping size and shape failures to the stable codes.
pub(crate) fn json_request<T>(request: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    request.map(|Json(value)| value).map_err(|rejection| {
        if rejection.status() == http::StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::new(ErrorKind::RequestTooLarge)
        } else {
            ApiError::new(ErrorKind::InvalidRequest)
        }
    })
}

/// Parses the agent path identity.
pub(crate) fn parse_agent_id(raw: &str) -> Result<AgentId, ApiError> {
    let uuid = uuid::Uuid::parse_str(raw).map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    Ok(AgentId::from(uuid))
}

/// Liveness: the process answers.
#[utoipa::path(
    get,
    path = "/health/live",
    responses(
        (status = 200, description = "the process is alive", body = LivenessResponse),
    )
)]
async fn liveness() -> Json<LivenessResponse> {
    Json(LivenessResponse { status: "ok" })
}

/// Readiness: Postgres is the core dependency; NATS only degrades realtime.
#[utoipa::path(
    get,
    path = "/health/ready",
    responses(
        (status = 200, description = "postgres serves; realtime capability is reported", body = ReadinessResponse),
        (status = 503, description = "postgres is unavailable", body = ReadinessResponse),
    )
)]
async fn readiness(State(state): State<Arc<AppState>>) -> Response {
    let realtime = if state.tail().is_some() {
        "ok"
    } else {
        "degraded"
    };
    match state.pg().ping().await {
        Ok(()) => (
            http::StatusCode::OK,
            Json(ReadinessResponse {
                status: "ok",
                realtime,
            }),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "readiness probe failed: postgres unavailable");
            (
                http::StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadinessResponse {
                    status: "unavailable",
                    realtime,
                }),
            )
                .into_response()
        }
    }
}
