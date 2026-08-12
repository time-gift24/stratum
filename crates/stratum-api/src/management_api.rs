//! Loopback-only HTTP boundary for Studio management resources.

use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        rejection::{JsonRejection, PathRejection, QueryRejection},
    },
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{ETAG, IF_MATCH, LOCATION},
    },
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use stratum_config::{AgentDefinitionConfig, AgentName, ConfigError, ProviderKind};
use utoipa::IntoParams;

use crate::{
    HostError, HostState,
    api::ErrorResponse,
    management_dto::{
        AgentDefinitionView, AgentDefinitionsPage, CreateAgentDefinitionRequest,
        CreateModelRequest, CreateProviderRequest, FieldViolation, ModelView, ModelsPage,
        ProviderKindDto, ProviderTestView, ProviderView, ProvidersPage,
        UpdateAgentDefinitionRequest, UpdateProviderRequest,
    },
};

const DEFAULT_PAGE: usize = 1;
const DEFAULT_PER_PAGE: usize = 20;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct AgentDefinitionListParams {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_per_page")]
    per_page: usize,
    #[serde(default = "default_agent_sort")]
    sort: String,
    #[serde(default)]
    search: Option<String>,
}

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct PageParams {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_per_page")]
    per_page: usize,
}

/// Returns every Studio management route with its host-state type still open.
pub(crate) fn routes() -> Router<Arc<HostState>> {
    Router::new()
        .route(
            "/v1/agent-definitions",
            get(list_agent_definitions).post(create_agent_definition),
        )
        .route(
            "/v1/agent-definitions/{agent_name}",
            get(get_agent_definition)
                .put(update_agent_definition)
                .delete(delete_agent_definition),
        )
        .route("/v1/providers", get(list_providers).post(create_provider))
        .route(
            "/v1/providers/{provider}",
            get(get_provider)
                .put(update_provider)
                .delete(delete_provider),
        )
        .route("/v1/providers/{provider}/test", post(test_provider))
        .route(
            "/v1/providers/{provider}/models",
            get(list_provider_models).post(create_provider_model),
        )
        .route(
            "/v1/providers/{provider}/models/{model_name}",
            get(get_provider_model).delete(delete_provider_model),
        )
}

/// Lists one page of persisted Agent definitions.
#[utoipa::path(
    get,
    path = "/v1/agent-definitions",
    params(AgentDefinitionListParams),
    responses(
        (status = 200, description = "one page of Agent definitions", body = AgentDefinitionsPage),
        (status = 400, description = "pagination, sorting, or search query is invalid", body = ErrorResponse),
        (status = 500, description = "definitions could not be decoded", body = ErrorResponse),
        (status = 503, description = "definition storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn list_agent_definitions(
    State(state): State<Arc<HostState>>,
    query: Result<Query<AgentDefinitionListParams>, QueryRejection>,
) -> Result<Json<AgentDefinitionsPage>, HostError> {
    let Query(query) = query.map_err(|_| HostError::InvalidRequest)?;
    let descending = match query.sort.as_str() {
        "updated_at" => false,
        "-updated_at" => true,
        _ => return Err(HostError::InvalidRequest),
    };
    let page = state
        .list_agent_definitions(
            query.page,
            query.per_page,
            query.search.as_deref(),
            descending,
        )
        .await?;
    Ok(Json(page))
}

/// Creates one Agent definition.
#[utoipa::path(
    post,
    path = "/v1/agent-definitions",
    request_body = CreateAgentDefinitionRequest,
    responses(
        (status = 201, description = "Agent definition created", body = AgentDefinitionView,
            headers(
                ("Location" = String, description = "canonical URI of the created definition"),
                ("ETag" = String, description = "strong canonical representation revision")
            )),
        (status = 400, description = "body or Agent name is invalid", body = ErrorResponse),
        (status = 409, description = "an Agent definition with this name already exists", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "definition fields or references are invalid", body = ErrorResponse),
        (status = 500, description = "definition could not be persisted", body = ErrorResponse),
        (status = 503, description = "definition storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn create_agent_definition(
    State(state): State<Arc<HostState>>,
    request: Result<Json<CreateAgentDefinitionRequest>, JsonRejection>,
) -> Result<Response, HostError> {
    let request = json_request(request)?;
    let agent_name = parse_agent_name(&request.agent_name)?;
    let definition = agent_definition(
        request.model,
        request.model_parameters,
        request.tools,
        request.prompt,
    )?;
    let (view, etag) = state
        .create_agent_definition(agent_name, definition)
        .await?;
    let location = format!(
        "/v1/agent-definitions/{}",
        encode_path_segment(&view.agent_name)
    );
    representation_response(StatusCode::CREATED, view, &etag, Some(&location))
}

/// Returns one Agent definition and its current strong revision.
#[utoipa::path(
    get,
    path = "/v1/agent-definitions/{agent_name}",
    params(("agent_name" = String, Path, description = "validated Agent definition name")),
    responses(
        (status = 200, description = "canonical Agent definition", body = AgentDefinitionView,
            headers(("ETag" = String, description = "strong canonical representation revision"))),
        (status = 400, description = "Agent name is invalid", body = ErrorResponse),
        (status = 404, description = "Agent definition was not found", body = ErrorResponse),
        (status = 500, description = "definition could not be decoded", body = ErrorResponse),
        (status = 503, description = "definition storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_agent_definition(
    State(state): State<Arc<HostState>>,
    path: Result<Path<String>, PathRejection>,
) -> Result<Response, HostError> {
    let Path(agent_name) = path.map_err(|_| HostError::InvalidRequest)?;
    let agent_name = parse_agent_name(&agent_name)?;
    let (view, etag) = state.read_agent_definition(agent_name).await?;
    representation_response(StatusCode::OK, view, &etag, None)
}

/// Completely replaces one Agent definition when `If-Match` is current.
#[utoipa::path(
    put,
    path = "/v1/agent-definitions/{agent_name}",
    params(
        ("agent_name" = String, Path, description = "validated Agent definition name"),
        ("If-Match" = String, Header, description = "strong revision returned by the latest GET")
    ),
    request_body = UpdateAgentDefinitionRequest,
    responses(
        (status = 200, description = "Agent definition replaced", body = AgentDefinitionView,
            headers(("ETag" = String, description = "new strong representation revision"))),
        (status = 400, description = "path, header, or body is invalid", body = ErrorResponse),
        (status = 404, description = "Agent definition was not found", body = ErrorResponse),
        (status = 412, description = "If-Match revision is stale", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "definition fields or references are invalid", body = ErrorResponse),
        (status = 500, description = "definition could not be persisted", body = ErrorResponse),
        (status = 503, description = "definition storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn update_agent_definition(
    State(state): State<Arc<HostState>>,
    path: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
    request: Result<Json<UpdateAgentDefinitionRequest>, JsonRejection>,
) -> Result<Response, HostError> {
    let Path(agent_name) = path.map_err(|_| HostError::InvalidRequest)?;
    let agent_name = parse_agent_name(&agent_name)?;
    let request = json_request(request)?;
    let definition = agent_definition(
        request.model,
        request.model_parameters,
        request.tools,
        request.prompt,
    )?;
    let (view, etag) = state
        .update_agent_definition(agent_name, definition, if_match(&headers)?)
        .await?;
    representation_response(StatusCode::OK, view, &etag, None)
}

/// Deletes only an Agent definition when `If-Match` is current.
#[utoipa::path(
    delete,
    path = "/v1/agent-definitions/{agent_name}",
    params(
        ("agent_name" = String, Path, description = "validated Agent definition name"),
        ("If-Match" = String, Header, description = "strong revision returned by the latest GET")
    ),
    responses(
        (status = 204, description = "Agent definition deleted"),
        (status = 400, description = "path or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Agent definition was not found", body = ErrorResponse),
        (status = 412, description = "If-Match revision is stale", body = ErrorResponse),
        (status = 500, description = "definition could not be deleted", body = ErrorResponse),
        (status = 503, description = "definition storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn delete_agent_definition(
    State(state): State<Arc<HostState>>,
    path: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
) -> Result<StatusCode, HostError> {
    let Path(agent_name) = path.map_err(|_| HostError::InvalidRequest)?;
    let agent_name = parse_agent_name(&agent_name)?;
    state
        .delete_agent_definition(agent_name, if_match(&headers)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists configured Providers without exposing credentials.
#[utoipa::path(
    get,
    path = "/v1/providers",
    params(PageParams),
    responses(
        (status = 200, description = "one page of configured Providers", body = ProvidersPage),
        (status = 400, description = "pagination query is invalid", body = ErrorResponse),
    )
)]
pub(crate) async fn list_providers(
    State(state): State<Arc<HostState>>,
    query: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<ProvidersPage>, HostError> {
    let Query(query) = query.map_err(|_| HostError::InvalidRequest)?;
    Ok(Json(
        state.list_providers(query.page, query.per_page).await?,
    ))
}

/// Creates one supported Provider and accepts its credential only as write-only input.
#[utoipa::path(
    post,
    path = "/v1/providers",
    request_body = CreateProviderRequest,
    responses(
        (status = 201, description = "Provider created", body = ProviderView,
            headers(
                ("Location" = String, description = "canonical URI of the created Provider"),
                ("ETag" = String, description = "strong canonical representation revision")
            )),
        (status = 400, description = "body, Provider kind, or credential is invalid", body = ErrorResponse),
        (status = 409, description = "Provider already exists", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "Provider candidate is invalid", body = ErrorResponse),
        (status = 500, description = "Provider could not be persisted", body = ErrorResponse),
        (status = 503, description = "catalog storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn create_provider(
    State(state): State<Arc<HostState>>,
    request: Result<Json<CreateProviderRequest>, JsonRejection>,
) -> Result<Response, HostError> {
    let request = json_request(request)?;
    let kind = ProviderKind::from(request.provider);
    let (view, etag) = state.create_provider(kind, request.api_key).await?;
    representation_response(
        StatusCode::CREATED,
        view,
        &etag,
        Some(&format!("/v1/providers/{}", kind.as_str())),
    )
}

/// Returns one sanitized Provider and its current strong revision.
#[utoipa::path(
    get,
    path = "/v1/providers/{provider}",
    params(("provider" = ProviderKindDto, Path, description = "supported Provider kind")),
    responses(
        (status = 200, description = "sanitized Provider projection", body = ProviderView,
            headers(("ETag" = String, description = "strong canonical representation revision"))),
        (status = 400, description = "Provider kind is invalid", body = ErrorResponse),
        (status = 404, description = "Provider was not found", body = ErrorResponse),
    )
)]
pub(crate) async fn get_provider(
    State(state): State<Arc<HostState>>,
    path: Result<Path<ProviderKindDto>, PathRejection>,
) -> Result<Response, HostError> {
    let Path(provider) = path.map_err(|_| HostError::InvalidRequest)?;
    let (view, etag) = state.read_provider(provider.into()).await?;
    representation_response(StatusCode::OK, view, &etag, None)
}

/// Replaces a Provider credential when one is supplied and `If-Match` is current.
#[utoipa::path(
    put,
    path = "/v1/providers/{provider}",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("If-Match" = String, Header, description = "strong revision returned by the latest GET")
    ),
    request_body = UpdateProviderRequest,
    responses(
        (status = 200, description = "Provider updated", body = ProviderView,
            headers(("ETag" = String, description = "new strong representation revision"))),
        (status = 400, description = "path, header, or credential is invalid", body = ErrorResponse),
        (status = 404, description = "Provider was not found", body = ErrorResponse),
        (status = 412, description = "If-Match revision is stale", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "Provider candidate is invalid", body = ErrorResponse),
        (status = 500, description = "Provider could not be persisted", body = ErrorResponse),
        (status = 503, description = "catalog storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn update_provider(
    State(state): State<Arc<HostState>>,
    path: Result<Path<ProviderKindDto>, PathRejection>,
    headers: HeaderMap,
    request: Result<Json<UpdateProviderRequest>, JsonRejection>,
) -> Result<Response, HostError> {
    let Path(provider) = path.map_err(|_| HostError::InvalidRequest)?;
    let request = json_request(request)?;
    let (view, etag) = state
        .update_provider(provider.into(), request.api_key, if_match(&headers)?)
        .await?;
    representation_response(StatusCode::OK, view, &etag, None)
}

/// Deletes an unreferenced Provider when `If-Match` is current.
#[utoipa::path(
    delete,
    path = "/v1/providers/{provider}",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("If-Match" = String, Header, description = "strong revision returned by the latest GET")
    ),
    responses(
        (status = 204, description = "Provider and its unreferenced Models deleted"),
        (status = 400, description = "path or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Provider was not found", body = ErrorResponse),
        (status = 409, description = "Provider is referenced by the default Model or Agent definitions", body = ErrorResponse),
        (status = 412, description = "If-Match revision is stale", body = ErrorResponse),
        (status = 500, description = "Provider could not be deleted", body = ErrorResponse),
        (status = 503, description = "catalog storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn delete_provider(
    State(state): State<Arc<HostState>>,
    path: Result<Path<ProviderKindDto>, PathRejection>,
    headers: HeaderMap,
) -> Result<StatusCode, HostError> {
    let Path(provider) = path.map_err(|_| HostError::InvalidRequest)?;
    state
        .delete_provider(provider.into(), if_match(&headers)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Runs one transient, sanitized Provider connection test.
#[utoipa::path(
    post,
    path = "/v1/providers/{provider}/test",
    params(("provider" = ProviderKindDto, Path, description = "supported Provider kind")),
    responses(
        (status = 200, description = "transient Provider test succeeded", body = ProviderTestView),
        (status = 400, description = "Provider kind is invalid", body = ErrorResponse),
        (status = 404, description = "Provider was not found", body = ErrorResponse),
        (status = 502, description = "Provider rejected or failed the sanitized probe", body = ErrorResponse),
    )
)]
pub(crate) async fn test_provider(
    State(state): State<Arc<HostState>>,
    path: Result<Path<ProviderKindDto>, PathRejection>,
) -> Result<Json<ProviderTestView>, HostError> {
    let Path(provider) = path.map_err(|_| HostError::InvalidRequest)?;
    Ok(Json(state.test_provider(provider.into()).await?))
}

/// Lists one page of Models configured beneath a Provider.
#[utoipa::path(
    get,
    path = "/v1/providers/{provider}/models",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        PageParams
    ),
    responses(
        (status = 200, description = "one page of Provider Models", body = ModelsPage),
        (status = 400, description = "Provider kind or pagination query is invalid", body = ErrorResponse),
        (status = 404, description = "Provider was not found", body = ErrorResponse),
        (status = 500, description = "Model schema could not be projected", body = ErrorResponse),
    )
)]
pub(crate) async fn list_provider_models(
    State(state): State<Arc<HostState>>,
    path: Result<Path<ProviderKindDto>, PathRejection>,
    query: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<ModelsPage>, HostError> {
    let Path(provider) = path.map_err(|_| HostError::InvalidRequest)?;
    let Query(query) = query.map_err(|_| HostError::InvalidRequest)?;
    Ok(Json(
        state
            .list_provider_models(provider.into(), query.page, query.per_page)
            .await?,
    ))
}

/// Creates one Provider-local Model.
#[utoipa::path(
    post,
    path = "/v1/providers/{provider}/models",
    params(("provider" = ProviderKindDto, Path, description = "supported Provider kind")),
    request_body = CreateModelRequest,
    responses(
        (status = 201, description = "Provider Model created", body = ModelView,
            headers(
                ("Location" = String, description = "canonical URI of the created Model"),
                ("ETag" = String, description = "strong canonical representation revision")
            )),
        (status = 400, description = "path, body, or Model name is invalid", body = ErrorResponse),
        (status = 404, description = "Provider was not found", body = ErrorResponse),
        (status = 409, description = "Model already exists", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "Model is unsupported by the Provider adapter", body = ErrorResponse),
        (status = 500, description = "Model could not be persisted", body = ErrorResponse),
        (status = 503, description = "catalog storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn create_provider_model(
    State(state): State<Arc<HostState>>,
    path: Result<Path<ProviderKindDto>, PathRejection>,
    request: Result<Json<CreateModelRequest>, JsonRejection>,
) -> Result<Response, HostError> {
    let Path(provider) = path.map_err(|_| HostError::InvalidRequest)?;
    let request = json_request(request)?;
    let kind = ProviderKind::from(provider);
    let (view, etag) = state.create_provider_model(kind, request.name).await?;
    let location = format!(
        "/v1/providers/{}/models/{}",
        kind.as_str(),
        encode_path_segment(&view.name)
    );
    representation_response(StatusCode::CREATED, view, &etag, Some(&location))
}

/// Returns one Provider Model and its current strong revision.
#[utoipa::path(
    get,
    path = "/v1/providers/{provider}/models/{model_name}",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("model_name" = String, Path, description = "Provider-local Model name")
    ),
    responses(
        (status = 200, description = "canonical Provider Model", body = ModelView,
            headers(("ETag" = String, description = "strong canonical representation revision"))),
        (status = 400, description = "Provider kind or Model name is invalid", body = ErrorResponse),
        (status = 404, description = "Provider or Model was not found", body = ErrorResponse),
        (status = 500, description = "Model schema could not be projected", body = ErrorResponse),
    )
)]
pub(crate) async fn get_provider_model(
    State(state): State<Arc<HostState>>,
    path: Result<Path<(ProviderKindDto, String)>, PathRejection>,
) -> Result<Response, HostError> {
    let Path((provider, model_name)) = path.map_err(|_| HostError::InvalidRequest)?;
    let (view, etag) = state
        .read_provider_model(provider.into(), &model_name)
        .await?;
    representation_response(StatusCode::OK, view, &etag, None)
}

/// Deletes an unreferenced Provider Model when `If-Match` is current.
#[utoipa::path(
    delete,
    path = "/v1/providers/{provider}/models/{model_name}",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("model_name" = String, Path, description = "Provider-local Model name"),
        ("If-Match" = String, Header, description = "strong revision returned by the latest GET")
    ),
    responses(
        (status = 204, description = "Provider Model deleted"),
        (status = 400, description = "path or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Provider or Model was not found", body = ErrorResponse),
        (status = 409, description = "Model is referenced by the default or an Agent definition", body = ErrorResponse),
        (status = 412, description = "If-Match revision is stale", body = ErrorResponse),
        (status = 500, description = "Model could not be deleted", body = ErrorResponse),
        (status = 503, description = "catalog storage is unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn delete_provider_model(
    State(state): State<Arc<HostState>>,
    path: Result<Path<(ProviderKindDto, String)>, PathRejection>,
    headers: HeaderMap,
) -> Result<StatusCode, HostError> {
    let Path((provider, model_name)) = path.map_err(|_| HostError::InvalidRequest)?;
    state
        .delete_provider_model(provider.into(), &model_name, if_match(&headers)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn json_request<T>(request: Result<Json<T>, JsonRejection>) -> Result<T, HostError> {
    request.map(|Json(value)| value).map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            HostError::MessageTooLarge
        } else {
            HostError::InvalidRequest
        }
    })
}

fn agent_definition(
    model: stratum_core::ModelId,
    model_parameters: serde_json::Map<String, serde_json::Value>,
    tools: Vec<stratum_core::ToolName>,
    prompt: String,
) -> Result<AgentDefinitionConfig, HostError> {
    AgentDefinitionConfig::new(Some(model), model_parameters, tools, prompt).map_err(|error| {
        match error {
            ConfigError::EmptyPrompt => {
                field_error("prompt", "required", "prompt must not be blank")
            }
            ConfigError::DuplicateTool { .. } => {
                field_error("tools", "duplicate", "tool names must be unique")
            }
            other => other.into(),
        }
    })
}

fn field_error(field: &'static str, code: &'static str, message: &'static str) -> HostError {
    HostError::ManagementValidation {
        violations: vec![FieldViolation {
            field,
            code,
            message,
        }],
    }
}

fn parse_agent_name(value: &str) -> Result<AgentName, HostError> {
    value
        .parse()
        .map_err(|_| HostError::ManagementRequestValidation {
            violations: vec![FieldViolation {
                field: "agent_name",
                code: "invalid",
                message: "agent name must match the documented ASCII pattern",
            }],
        })
}

fn if_match(headers: &HeaderMap) -> Result<&str, HostError> {
    headers
        .get(IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or(HostError::InvalidRequest)
}

fn representation_response<T: serde::Serialize>(
    status: StatusCode,
    body: T,
    etag: &str,
    location: Option<&str>,
) -> Result<Response, HostError> {
    let etag = HeaderValue::from_str(etag).map_err(|_| HostError::InvalidRequest)?;
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(ETAG, etag);
    if let Some(location) = location {
        let location = HeaderValue::from_str(location).map_err(|_| HostError::InvalidRequest)?;
        response.headers_mut().insert(LOCATION, location);
    }
    Ok(response)
}

const fn default_page() -> usize {
    DEFAULT_PAGE
}

const fn default_per_page() -> usize {
    DEFAULT_PER_PAGE
}

fn default_agent_sort() -> String {
    "-updated_at".to_owned()
}

fn encode_path_segment(value: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push('%');
            encoded.push(char::from(HEX[usize::from(byte >> 4)]));
            encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segment_encoding_preserves_unreserved_and_escapes_separators() {
        assert_eq!(encode_path_segment("gpt-4.1_mini~x"), "gpt-4.1_mini~x");
        assert_eq!(encode_path_segment("org/model:beta"), "org%2Fmodel%3Abeta");
    }

    #[test]
    fn missing_if_match_is_rejected() {
        assert!(matches!(
            if_match(&HeaderMap::new()),
            Err(HostError::InvalidRequest)
        ));
    }
}
