//! HTTP surface: router, OpenAPI document, health endpoints, and shared
//! request helpers.

mod agents;
mod events;
mod ontology;
mod turns;

use std::sync::Arc;
use std::time::Duration;

use axum::extract::rejection::JsonRejection;
use axum::extract::{MatchedPath, Request, State};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use stratum_core::AgentRuntimeId;
use stratum_infra::NatsAgentRuntimeTail;
use tokio_util::sync::CancellationToken;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{Span, field, info_span};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::dto::{
    AgentRuntimeCreated, AgentRuntimeView, AgentTemplatesResponse, CreateAgentRuntimeRequest,
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
        agents::create_agent_runtime,
        agents::list_agent_templates,
        agents::list_models,
        agents::get_agent_runtime,
        agents::get_history,
        turns::post_message,
        turns::post_resume,
        turns::post_cancel,
        turns::post_approval,
        events::get_events,
        ontology::list_ontologies,
        ontology::create_ontology,
        ontology::get_ontology,
        ontology::replace_ontology,
        ontology::delete_ontology,
        ontology::get_neighborhood,
        liveness,
        readiness,
    ),
    components(schemas(
        CreateAgentRuntimeRequest,
        AgentRuntimeCreated,
        AgentTemplatesResponse,
        crate::dto::AgentTemplateDto,
        ModelsResponse,
        stratum_llm::ModelDescriptor,
        AgentRuntimeView,
        crate::dto::AgentRuntimeStatusDto,
        crate::dto::PendingApprovalDto,
        HistoryResponse,
        crate::dto::HistoryItemDto,
        crate::dto::MessageRequest,
        crate::dto::ResumeRequest,
        crate::dto::CancelRequest,
        crate::dto::ApprovalResolveRequest,
        TurnAccepted,
        crate::frames::AgentRuntimeStreamFrameV1,
        crate::frames::ControlEventV1,
        crate::frames::StreamResetReason,
        crate::frames::AgentRuntimeProductEventV1,
        crate::frames::LlmTelemetryEventV1,
        LivenessResponse,
        ReadinessResponse,
        ErrorResponse,
        crate::error::ErrorBody,
        crate::error::OntologyValidationErrorResponse,
        crate::error::OntologyViolationCode,
        crate::error::ViolationResponse,
        ontology::OntologyIdDto,
        ontology::ObjectTypeIdDto,
        ontology::PropertyIdDto,
        ontology::LinkTypeIdDto,
        ontology::ValueTypeDto,
        ontology::CardinalityDto,
        ontology::CreateOntologyRequest,
        ontology::OntologyDto,
        ontology::ObjectTypeDto,
        ontology::PropertyDto,
        ontology::LinkTypeDto,
        ontology::CanvasDto,
        ontology::CanvasPositionDto,
        ontology::OntologySummaryDto,
        ontology::PaginationDto,
        ontology::OntologyListResponse,
        ontology::NeighborhoodDto,
        ontology::ListSortDto,
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
    let management_enabled = state.management_enabled();
    let openapi = openapi(management_enabled);
    let shutdown = state.shutdown_token();
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
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", openapi))
        .route("/v1/agent-runtimes", post(agents::create_agent_runtime))
        .route("/v1/agent-templates", get(agents::list_agent_templates))
        .route("/v1/models", get(agents::list_models))
        .route(
            "/v1/agent-runtimes/{agent_runtime_id}",
            get(agents::get_agent_runtime),
        )
        .route(
            "/v1/agent-runtimes/{agent_runtime_id}/history",
            get(agents::get_history),
        )
        .route(
            "/v1/agent-runtimes/{agent_runtime_id}/messages",
            post(turns::post_message),
        )
        .route(
            "/v1/agent-runtimes/{agent_runtime_id}/resume",
            post(turns::post_resume),
        )
        .route(
            "/v1/agent-runtimes/{agent_runtime_id}/cancel",
            post(turns::post_cancel),
        )
        .route(
            "/v1/agent-runtimes/{agent_runtime_id}/approvals/{approval_id}",
            post(turns::post_approval),
        )
        .route(
            "/v1/agent-runtimes/{agent_runtime_id}/events",
            get(events::get_events),
        )
        .merge(ontology::routes())
        .route("/health/live", get(liveness))
        .route("/health/ready", get(readiness));
    let router = if management_enabled {
        router.merge(crate::studio_management::routes())
    } else {
        router
    };
    let router = router
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(JSON_BODY_LIMIT))
        .layer(axum::middleware::from_fn_with_state(
            shutdown,
            reject_during_shutdown,
        ))
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
                        agent_runtime_id = field::Empty,
                        agent_id = field::Empty,
                        session_id = field::Empty,
                        turn_id = field::Empty,
                        ontology_id = field::Empty,
                        object_type_id = field::Empty,
                        ontology_depth = field::Empty,
                        operation = field::Empty,
                        outcome = field::Empty,
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
                .allow_methods([
                    http::Method::GET,
                    http::Method::POST,
                    http::Method::PUT,
                    http::Method::DELETE,
                ])
                // Browser clients send `Idempotency-Key` on create and
                // `Last-Event-ID` on SSE reconnect; both must survive the
                // preflight allowlist.
                .allow_headers([
                    http::header::CONTENT_TYPE,
                    http::HeaderName::from_static("last-event-id"),
                    http::HeaderName::from_static("idempotency-key"),
                    http::header::IF_MATCH,
                ])
                .expose_headers([http::header::ETAG, http::header::LOCATION]),
        )
    }
}

fn openapi(studio_enabled: bool) -> utoipa::openapi::OpenApi {
    let mut document = ApiDoc::openapi();
    if studio_enabled {
        document.merge(crate::studio_management::openapi());
    }
    document
}

/// Cancels an in-flight handler future when process shutdown begins. Axum's
/// graceful server can then finish its connection tasks instead of waiting on
/// an unbounded Postgres, NATS, or provider operation. This is a hosting
/// concern only: it never signals a Turn token or writes a business terminal
/// event.
async fn reject_during_shutdown(
    State(shutdown): State<CancellationToken>,
    request: Request,
    next: Next,
) -> Response {
    tokio::select! {
        biased;
        () = shutdown.cancelled() => {
            ApiError::new(ErrorKind::ServiceShuttingDown).into_response()
        }
        response = next.run(request) => response,
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

/// Parses the AgentRuntime path identity.
pub(crate) fn parse_agent_runtime_id(raw: &str) -> Result<AgentRuntimeId, ApiError> {
    let uuid = uuid::Uuid::parse_str(raw).map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    Ok(AgentRuntimeId::from(uuid))
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

/// Readiness: execution, Ontology, and Studio Postgres must answer; NATS only
/// degrades realtime.
#[utoipa::path(
    get,
    path = "/health/ready",
    responses(
        (status = 200, description = "execution, Ontology, and Studio Postgres serve; realtime capability is reported", body = ReadinessResponse),
        (status = 503, description = "a required Postgres dependency is unavailable", body = ReadinessResponse),
    )
)]
async fn readiness(State(state): State<Arc<AppState>>) -> Response {
    let realtime = if state.tail().is_some_and(NatsAgentRuntimeTail::is_available) {
        "ok"
    } else {
        "degraded"
    };
    let probes = tokio::time::timeout(state.readiness_timeout(), async {
        tokio::join!(
            state.pg().ping(),
            state.ontology().is_ready(),
            state.studio().ping()
        )
    })
    .await;
    match probes {
        Ok((Ok(()), true, Ok(()))) => (
            http::StatusCode::OK,
            Json(ReadinessResponse {
                status: "ok",
                realtime,
            }),
        )
            .into_response(),
        Ok((execution, ontology_ready, studio)) => {
            tracing::error!(
                execution_ready = execution.is_ok(),
                ontology_ready,
                studio_ready = studio.is_ok(),
                "readiness probe failed"
            );
            (
                http::StatusCode::SERVICE_UNAVAILABLE,
                Json(ReadinessResponse {
                    status: "unavailable",
                    realtime,
                }),
            )
                .into_response()
        }
        Err(_) => {
            tracing::error!("readiness probe timed out");
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::future::pending;
    use std::sync::Arc;

    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::routing::get;
    use serde_json::json;
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;
    use tower::ServiceExt;

    use super::{openapi, reject_during_shutdown};

    #[test]
    fn openapi_documents_base_endpoints_and_decimal_sequences() {
        let document = serde_json::to_value(openapi(false)).expect("OpenAPI serializes");
        let actual = document["paths"]
            .as_object()
            .expect("paths are an object")
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let expected = [
            "/health/live",
            "/health/ready",
            "/v1/agent-runtimes",
            "/v1/agent-runtimes/{agent_runtime_id}",
            "/v1/agent-runtimes/{agent_runtime_id}/approvals/{approval_id}",
            "/v1/agent-runtimes/{agent_runtime_id}/cancel",
            "/v1/agent-runtimes/{agent_runtime_id}/events",
            "/v1/agent-runtimes/{agent_runtime_id}/history",
            "/v1/agent-runtimes/{agent_runtime_id}/messages",
            "/v1/agent-runtimes/{agent_runtime_id}/resume",
            "/v1/agent-templates",
            "/v1/models",
            "/v1/ontologies",
            "/v1/ontologies/{ontology_id}",
            "/v1/ontologies/{ontology_id}/object-types/{object_type_id}/neighborhood",
        ]
        .into_iter()
        .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected);

        let schemas = document["components"]["schemas"]
            .as_object()
            .expect("schemas are an object");
        assert!(schemas.contains_key("AgentRuntimeCreated"));
        assert!(schemas.contains_key("AgentRuntimeView"));
        assert!(schemas.contains_key("AgentRuntimeStreamFrameV1"));

        let frame = &schemas["AgentRuntimeStreamFrameV1"];
        for field in ["event_seq", "durable_before_event_seq", "telemetry_seq"] {
            let mut matches = Vec::new();
            collect_property_schemas(frame, field, &mut matches);
            assert_eq!(matches.len(), 1, "one frame variant owns {field}");
            assert_eq!(matches[0]["type"], "string", "{field} is a string");
        }
    }

    #[test]
    fn openapi_documents_studio_endpoints_only_when_studio_is_enabled() {
        let disabled = serde_json::to_value(openapi(false)).expect("OpenAPI serializes");
        let disabled_paths = disabled["paths"].as_object().expect("paths are an object");
        assert!(!disabled_paths.contains_key("/v1/agent-definitions"));
        assert!(!disabled_paths.contains_key("/v1/providers"));
        assert!(!disabled_paths.contains_key("/v1/providers/{provider}/test"));
        assert!(!disabled_paths.contains_key("/v1/providers/{provider}/models/{model_name}/test"));
        assert!(!disabled_paths.contains_key("/v1/tools"));

        let enabled = serde_json::to_value(openapi(true)).expect("OpenAPI serializes");
        let enabled_paths = enabled["paths"].as_object().expect("paths are an object");
        let expected_operations = [
            ("/v1/agent-definitions", ["get", "post"].as_slice()),
            (
                "/v1/agent-definitions/{agent_name}",
                ["get", "put", "delete"].as_slice(),
            ),
            ("/v1/providers", ["get", "post"].as_slice()),
            (
                "/v1/providers/{provider}",
                ["get", "put", "delete"].as_slice(),
            ),
            (
                "/v1/providers/{provider}/models",
                ["get", "post"].as_slice(),
            ),
            (
                "/v1/providers/{provider}/models/{model_name}",
                ["get", "delete"].as_slice(),
            ),
            (
                "/v1/providers/{provider}/models/{model_name}/test",
                ["post"].as_slice(),
            ),
            ("/v1/providers/{provider}/test", ["post"].as_slice()),
            ("/v1/tools", ["get"].as_slice()),
        ];

        for (path, methods) in expected_operations {
            let item = enabled_paths
                .get(path)
                .and_then(serde_json::Value::as_object)
                .expect("Studio path is documented");
            for method in methods {
                let responses = item
                    .get(*method)
                    .and_then(|operation| operation.get("responses"))
                    .and_then(serde_json::Value::as_object)
                    .expect("Studio operation documents responses");
                assert!(
                    responses
                        .values()
                        .all(|response| response["description"].is_string())
                );
            }
        }

        let response_codes = |path: &str, method: &str| {
            enabled_paths[path][method]["responses"]
                .as_object()
                .expect("responses are documented")
        };
        assert!(
            !response_codes("/v1/agent-definitions/{agent_name}", "delete").contains_key("409")
        );
        assert!(
            response_codes("/v1/providers/{provider}/models/{model_name}", "delete")
                .contains_key("409")
        );
        assert!(response_codes("/v1/providers/{provider}/test", "post").contains_key("500"));
        assert!(response_codes("/v1/providers/{provider}", "put").contains_key("422"));
        assert!(response_codes("/v1/providers/{provider}/models", "post").contains_key("404"));
        assert!(
            response_codes("/v1/providers/{provider}/models/{model_name}", "delete")
                .contains_key("422")
        );

        let schemas = enabled["components"]["schemas"]
            .as_object()
            .expect("schemas are an object");
        for schema in [
            "AgentDefinitionView",
            "CreateAgentDefinitionRequest",
            "ProviderView",
            "CreateProviderRequest",
            "ModelView",
            "CreateModelRequest",
        ] {
            assert!(
                schemas.contains_key(schema),
                "Studio schema {schema} is documented"
            );
        }

        let error_body = &schemas["ErrorBody"];
        let error_properties = error_body["properties"]
            .as_object()
            .expect("ErrorBody properties are documented");
        assert!(error_properties.contains_key("violations"));
        assert!(error_properties.contains_key("blockers"));
    }

    fn collect_property_schemas<'a>(
        value: &'a serde_json::Value,
        property: &str,
        found: &mut Vec<&'a serde_json::Value>,
    ) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(schema) = object
                    .get("properties")
                    .and_then(serde_json::Value::as_object)
                    .and_then(|properties| properties.get(property))
                {
                    found.push(schema);
                }
                for child in object.values() {
                    collect_property_schemas(child, property, found);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    collect_property_schemas(child, property, found);
                }
            }
            _ => {}
        }
    }

    #[tokio::test]
    async fn shutdown_drops_a_hanging_handler_and_returns_the_stable_envelope() {
        let shutdown = CancellationToken::new();
        let entered = Arc::new(Notify::new());
        let handler_entered = Arc::clone(&entered);
        let app = Router::new()
            .route(
                "/",
                get(move || {
                    let entered = Arc::clone(&handler_entered);
                    async move {
                        entered.notify_one();
                        pending::<&'static str>().await
                    }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                shutdown.clone(),
                reject_during_shutdown,
            ));
        let response_task = tokio::spawn(
            app.oneshot(
                http::Request::builder()
                    .uri("/")
                    .body(Body::empty())
                    .expect("test request is valid"),
            ),
        );
        entered.notified().await;

        shutdown.cancel();

        let response = response_task
            .await
            .expect("request task joins")
            .expect("router is infallible");
        assert_eq!(response.status(), http::StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(response.into_body(), 1024)
            .await
            .expect("response body is readable");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).expect("body is json"),
            json!({
                "error": {
                "code": "service_shutting_down",
                    "message": "the service is shutting down"
                }
            })
        );
    }
}
