//! Loopback-only HTTP boundary for the mutable Studio catalog.
//!
//! Routes are attached only when `[studio].management_enabled` has passed the
//! loopback and database validation in `stratum-config`. This module never
//! returns credentials: the only secret-bearing values are write-only request
//! fields forwarded straight to `stratum-studio`.

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::{Path, Query, State, rejection::JsonRejection},
    http::{
        HeaderMap, HeaderValue, StatusCode,
        header::{ETAG, IF_MATCH, LOCATION},
    },
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use stratum_core::{AgentName, ModelConfig};
use stratum_llm::LlmError;
use stratum_studio::{
    AgentDefinition, AgentDefinitionInput, ManagedModel, ProviderKind, ProviderSummary,
    ResourceVersion, StudioError, StudioStore, Versioned,
};
use utoipa::OpenApi;

use crate::{
    ApiError, AppState, ErrorKind,
    error::ErrorResponse,
    management_dto::{
        AgentDefinitionView, AgentDefinitionsPage, CreateAgentDefinitionRequest,
        CreateModelRequest, CreateProviderRequest, ModelView, ModelsPage, PaginationView,
        ProviderKindDto, ProviderView, ProvidersPage, UpdateAgentDefinitionRequest,
        UpdateProviderRequest,
    },
};

const DEFAULT_PAGE: usize = 1;
const DEFAULT_PER_PAGE: usize = 20;
const MAX_PER_PAGE: usize = 100;

/// OpenAPI document for the loopback-only Studio management surface.
#[derive(OpenApi)]
#[openapi(
    tags((
        name = "Studio",
        description = "Loopback-only management API for the mutable Studio catalog"
    )),
    paths(
        list_agent_definitions,
        create_agent_definition,
        get_agent_definition,
        update_agent_definition,
        delete_agent_definition,
        list_providers,
        create_provider,
        get_provider,
        update_provider,
        delete_provider,
        list_provider_models,
        create_provider_model,
        get_provider_model,
        delete_provider_model,
    ),
    components(schemas(
        AgentDefinitionView,
        AgentDefinitionsPage,
        CreateAgentDefinitionRequest,
        UpdateAgentDefinitionRequest,
        ProviderKindDto,
        ProviderView,
        ProvidersPage,
        CreateProviderRequest,
        UpdateProviderRequest,
        ModelView,
        ModelsPage,
        CreateModelRequest,
        PaginationView,
        ErrorResponse,
    ))
)]
struct StudioApiDoc;

/// Returns the management OpenAPI fragment when Studio is enabled.
pub(crate) fn openapi() -> utoipa::openapi::OpenApi {
    StudioApiDoc::openapi()
}

#[derive(Deserialize)]
struct PageParams {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_per_page")]
    per_page: usize,
}

#[derive(Deserialize)]
struct AgentPageParams {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_per_page")]
    per_page: usize,
    #[serde(default)]
    search: Option<String>,
}

/// Returns every Studio management route. The caller attaches this router
/// only when a Studio catalog exists in [`AppState`].
pub(crate) fn routes() -> Router<Arc<AppState>> {
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
        .route(
            "/v1/providers/{provider}/models",
            get(list_provider_models).post(create_provider_model),
        )
        .route(
            "/v1/providers/{provider}/models/{model_name}",
            get(get_provider_model).delete(delete_provider_model),
        )
}

/// Lists Studio Agent definitions with deterministic pagination.
#[utoipa::path(
    get,
    path = "/v1/agent-definitions",
    tag = "Studio",
    params(
        ("page" = Option<usize>, Query, description = "one-based page number; defaults to 1"),
        ("per_page" = Option<usize>, Query, description = "page size from 1 through 100; defaults to 20"),
        ("search" = Option<String>, Query, description = "case-insensitive Agent name substring")
    ),
    responses(
        (status = 200, description = "one Studio Agent definition page", body = AgentDefinitionsPage),
        (status = 400, description = "pagination is invalid", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn list_agent_definitions(
    State(state): State<Arc<AppState>>,
    Query(query): Query<AgentPageParams>,
) -> Result<Json<AgentDefinitionsPage>, ApiError> {
    let (page, per_page) = page(query.page, query.per_page)?;
    let search = query.search.map(|value| value.to_lowercase());
    let mut entries = studio(&state)?
        .list_agent_definitions()
        .await
        .map_err(map_studio_error)?
        .into_iter()
        .filter(|entry| {
            search.as_ref().is_none_or(|query| {
                entry
                    .value
                    .agent_name
                    .as_str()
                    .to_lowercase()
                    .contains(query)
            })
        })
        .map(|entry| agent_view(entry.value))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.updated_at));
    let response = page_response(entries, page, per_page);
    Ok(Json(AgentDefinitionsPage {
        data: response.data,
        pagination: response.pagination,
    }))
}

/// Creates a Studio Agent definition.
#[utoipa::path(
    post,
    path = "/v1/agent-definitions",
    tag = "Studio",
    request_body(
        content = CreateAgentDefinitionRequest,
        description = "complete Studio Agent definition; maximum encoded body size is 64 KiB"
    ),
    responses(
        (status = 201, description = "Studio Agent definition created", body = AgentDefinitionView,
            headers(
                ("Location" = String, description = "canonical URI of the created definition"),
                ("ETag" = String, description = "current strong Studio entity tag")
            )
        ),
        (status = 400, description = "request body is invalid", body = ErrorResponse),
        (status = 409, description = "Agent name or version conflicts with the catalog", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "Studio Agent definition is invalid", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn create_agent_definition(
    State(state): State<Arc<AppState>>,
    request: Result<Json<CreateAgentDefinitionRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let request = json(request)?;
    let input = agent_input(
        request.agent_name,
        request.agent_version,
        request.model,
        request.model_parameters,
        request.tools,
        request.prompt,
    )?;
    let created = studio(&state)?
        .create_agent_definition(input)
        .await
        .map_err(map_studio_error)?;
    let location = format!(
        "/v1/agent-definitions/{}",
        encode_path_segment(created.value.agent_name.as_str())
    );
    versioned_response(
        StatusCode::CREATED,
        agent_view(created.value),
        created.version,
        Some(&location),
    )
}

/// Reads one Studio Agent definition.
#[utoipa::path(
    get,
    path = "/v1/agent-definitions/{agent_name}",
    tag = "Studio",
    params(("agent_name" = String, Path, description = "stable Agent name")),
    responses(
        (status = 200, description = "current Studio Agent definition", body = AgentDefinitionView,
            headers(("ETag" = String, description = "current strong Studio entity tag"))
        ),
        (status = 404, description = "Studio Agent definition was not found", body = ErrorResponse),
        (status = 422, description = "Agent name is invalid", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn get_agent_definition(
    State(state): State<Arc<AppState>>,
    Path(agent_name): Path<String>,
) -> Result<Response, ApiError> {
    let agent_name = parse_agent_name(&agent_name)?;
    let definition = studio(&state)?
        .agent_definition(&agent_name)
        .await
        .map_err(map_studio_error)?;
    versioned_response(
        StatusCode::OK,
        agent_view(definition.value),
        definition.version,
        None,
    )
}

/// Replaces a Studio Agent definition when its strong ETag is current.
#[utoipa::path(
    put,
    path = "/v1/agent-definitions/{agent_name}",
    tag = "Studio",
    params(
        ("agent_name" = String, Path, description = "stable Agent name"),
        ("If-Match" = String, Header, description = "one required current strong Studio entity tag")
    ),
    request_body(
        content = UpdateAgentDefinitionRequest,
        description = "complete replacement definition; maximum encoded body size is 64 KiB"
    ),
    responses(
        (status = 200, description = "Studio Agent definition replaced", body = AgentDefinitionView,
            headers(("ETag" = String, description = "new strong Studio entity tag"))
        ),
        (status = 400, description = "request, path, or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Studio Agent definition was not found", body = ErrorResponse),
        (status = 409, description = "replacement conflicts with the catalog", body = ErrorResponse),
        (status = 412, description = "Studio entity tag is stale", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "Studio Agent definition is invalid", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn update_agent_definition(
    State(state): State<Arc<AppState>>,
    Path(agent_name): Path<String>,
    headers: HeaderMap,
    request: Result<Json<UpdateAgentDefinitionRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let request = json(request)?;
    let input = agent_input(
        agent_name,
        request.agent_version,
        request.model,
        request.model_parameters,
        request.tools,
        request.prompt,
    )?;
    let updated = studio(&state)?
        .replace_agent_definition(input, if_match(&headers)?)
        .await
        .map_err(map_studio_error)?;
    versioned_response(
        StatusCode::OK,
        agent_view(updated.value),
        updated.version,
        None,
    )
}

/// Deletes a Studio Agent definition when its strong ETag is current.
#[utoipa::path(
    delete,
    path = "/v1/agent-definitions/{agent_name}",
    tag = "Studio",
    params(
        ("agent_name" = String, Path, description = "stable Agent name"),
        ("If-Match" = String, Header, description = "one required current strong Studio entity tag")
    ),
    responses(
        (status = 204, description = "Studio Agent definition deleted", body = ()),
        (status = 400, description = "path or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Studio Agent definition was not found", body = ErrorResponse),
        (status = 409, description = "definition remains referenced by a Provider model", body = ErrorResponse),
        (status = 412, description = "Studio entity tag is stale", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn delete_agent_definition(
    State(state): State<Arc<AppState>>,
    Path(agent_name): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let agent_name = parse_agent_name(&agent_name)?;
    studio(&state)?
        .delete_agent_definition(&agent_name, if_match(&headers)?)
        .await
        .map_err(map_studio_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists configured Studio Providers with deterministic pagination.
#[utoipa::path(
    get,
    path = "/v1/providers",
    tag = "Studio",
    params(
        ("page" = Option<usize>, Query, description = "one-based page number; defaults to 1"),
        ("per_page" = Option<usize>, Query, description = "page size from 1 through 100; defaults to 20")
    ),
    responses(
        (status = 200, description = "one Studio Provider page", body = ProvidersPage),
        (status = 400, description = "pagination is invalid", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn list_providers(
    State(state): State<Arc<AppState>>,
    Query(query): Query<PageParams>,
) -> Result<Json<ProvidersPage>, ApiError> {
    let (page, per_page) = page(query.page, query.per_page)?;
    let data = studio(&state)?
        .list_providers()
        .await
        .map_err(map_studio_error)?
        .into_iter()
        .map(|entry| provider_view(entry.value))
        .collect();
    let response = page_response(data, page, per_page);
    Ok(Json(ProvidersPage {
        data: response.data,
        pagination: response.pagination,
    }))
}

/// Configures one Studio Provider credential.
#[utoipa::path(
    post,
    path = "/v1/providers",
    tag = "Studio",
    request_body(
        content = CreateProviderRequest,
        description = "Provider credential; maximum encoded body size is 64 KiB"
    ),
    responses(
        (status = 201, description = "Studio Provider created", body = ProviderView,
            headers(
                ("Location" = String, description = "canonical URI of the created Provider"),
                ("ETag" = String, description = "current strong Studio entity tag")
            )
        ),
        (status = 400, description = "request body is invalid", body = ErrorResponse),
        (status = 409, description = "Provider already exists", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "Provider credential is invalid", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn create_provider(
    State(state): State<Arc<AppState>>,
    request: Result<Json<CreateProviderRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let request = json(request)?;
    let provider = studio(&state)?
        .create_provider(request.provider.into(), request.api_key)
        .await
        .map_err(map_studio_error)?;
    state
        .refresh_studio_providers()
        .await
        .map_err(map_host_error)?;
    let location = format!("/v1/providers/{}", provider.value.kind);
    versioned_response(
        StatusCode::CREATED,
        provider_view(provider.value),
        provider.version,
        Some(&location),
    )
}

/// Reads one Studio Provider without returning its credential.
#[utoipa::path(
    get,
    path = "/v1/providers/{provider}",
    tag = "Studio",
    params(("provider" = ProviderKindDto, Path, description = "supported Provider kind")),
    responses(
        (status = 200, description = "current Studio Provider without credential material", body = ProviderView,
            headers(("ETag" = String, description = "current strong Studio entity tag"))
        ),
        (status = 400, description = "Provider path parameter is invalid", body = ErrorResponse),
        (status = 404, description = "Studio Provider was not found", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn get_provider(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<ProviderKindDto>,
) -> Result<Response, ApiError> {
    let provider = studio(&state)?
        .provider(provider.into())
        .await
        .map_err(map_studio_error)?;
    versioned_response(
        StatusCode::OK,
        provider_view(provider.value),
        provider.version,
        None,
    )
}

/// Replaces a Studio Provider credential when its strong ETag is current.
#[utoipa::path(
    put,
    path = "/v1/providers/{provider}",
    tag = "Studio",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("If-Match" = String, Header, description = "one required current strong Studio entity tag")
    ),
    request_body(
        content = UpdateProviderRequest,
        description = "credential replacement; omission preserves the current credential; maximum encoded body size is 64 KiB"
    ),
    responses(
        (status = 200, description = "Studio Provider updated", body = ProviderView,
            headers(("ETag" = String, description = "current strong Studio entity tag"))
        ),
        (status = 400, description = "request, path, or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Studio Provider was not found", body = ErrorResponse),
        (status = 412, description = "Studio entity tag is stale", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn update_provider(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<ProviderKindDto>,
    headers: HeaderMap,
    request: Result<Json<UpdateProviderRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let request = json(request)?;
    let kind: ProviderKind = provider.into();
    let expected = if_match(&headers)?;
    let updated = match request.api_key {
        Some(api_key) => studio(&state)?
            .replace_provider_credential(kind, api_key, expected)
            .await
            .map_err(map_studio_error)?,
        None => {
            let current = studio(&state)?
                .provider(kind)
                .await
                .map_err(map_studio_error)?;
            if current.version != expected {
                return Err(map_studio_error(StudioError::PreconditionFailed));
            }
            current
        }
    };
    state
        .refresh_studio_providers()
        .await
        .map_err(map_host_error)?;
    versioned_response(
        StatusCode::OK,
        provider_view(updated.value),
        updated.version,
        None,
    )
}

/// Deletes a Studio Provider when its strong ETag is current.
#[utoipa::path(
    delete,
    path = "/v1/providers/{provider}",
    tag = "Studio",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("If-Match" = String, Header, description = "one required current strong Studio entity tag")
    ),
    responses(
        (status = 204, description = "Studio Provider deleted", body = ()),
        (status = 400, description = "path or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Studio Provider was not found", body = ErrorResponse),
        (status = 409, description = "Provider remains referenced by a model", body = ErrorResponse),
        (status = 412, description = "Studio entity tag is stale", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn delete_provider(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<ProviderKindDto>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    studio(&state)?
        .delete_provider(provider.into(), if_match(&headers)?)
        .await
        .map_err(map_studio_error)?;
    state
        .refresh_studio_providers()
        .await
        .map_err(map_host_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists one Studio Provider's configured models with deterministic pagination.
#[utoipa::path(
    get,
    path = "/v1/providers/{provider}/models",
    tag = "Studio",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("page" = Option<usize>, Query, description = "one-based page number; defaults to 1"),
        ("per_page" = Option<usize>, Query, description = "page size from 1 through 100; defaults to 20")
    ),
    responses(
        (status = 200, description = "one Studio model page", body = ModelsPage),
        (status = 400, description = "path or pagination is invalid", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn list_provider_models(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<ProviderKindDto>,
    Query(query): Query<PageParams>,
) -> Result<Json<ModelsPage>, ApiError> {
    let (page, per_page) = page(query.page, query.per_page)?;
    let kind: ProviderKind = provider.into();
    let data = studio(&state)?
        .list_models()
        .await
        .map_err(map_studio_error)?
        .into_iter()
        .filter(|entry| entry.value.provider == kind)
        .map(|entry| model_view(&state, entry))
        .collect::<Result<Vec<_>, _>>()?;
    let response = page_response(data, page, per_page);
    Ok(Json(ModelsPage {
        data: response.data,
        pagination: response.pagination,
    }))
}

/// Adds a Provider-local model to the Studio catalog.
#[utoipa::path(
    post,
    path = "/v1/providers/{provider}/models",
    tag = "Studio",
    params(("provider" = ProviderKindDto, Path, description = "supported Provider kind")),
    request_body(
        content = CreateModelRequest,
        description = "Provider-local model name; maximum encoded body size is 64 KiB"
    ),
    responses(
        (status = 201, description = "Studio Provider model created", body = ModelView,
            headers(
                ("Location" = String, description = "canonical URI of the created model"),
                ("ETag" = String, description = "current strong Studio entity tag")
            )
        ),
        (status = 400, description = "request body or Provider path is invalid", body = ErrorResponse),
        (status = 409, description = "model already exists", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "model is invalid or not supported by the Provider adapter", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn create_provider_model(
    State(state): State<Arc<AppState>>,
    Path(provider): Path<ProviderKindDto>,
    request: Result<Json<CreateModelRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let request = json(request)?;
    let kind: ProviderKind = provider.into();
    state
        .validate_studio_model(kind, &request.name)
        .map_err(map_host_error)?;
    let model = studio(&state)?
        .create_model(kind, request.name)
        .await
        .map_err(map_studio_error)?;
    state
        .refresh_studio_providers()
        .await
        .map_err(map_host_error)?;
    let location = format!(
        "/v1/providers/{}/models/{}",
        model.value.provider,
        encode_path_segment(&model.value.name)
    );
    let view = model_view(&state, model.clone())?;
    versioned_response(StatusCode::CREATED, view, model.version, Some(&location))
}

/// Reads one Studio Provider model.
#[utoipa::path(
    get,
    path = "/v1/providers/{provider}/models/{model_name}",
    tag = "Studio",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("model_name" = String, Path, description = "Provider-local model name")
    ),
    responses(
        (status = 200, description = "current Studio Provider model", body = ModelView,
            headers(("ETag" = String, description = "current strong Studio entity tag"))
        ),
        (status = 400, description = "Provider path is invalid", body = ErrorResponse),
        (status = 404, description = "Studio Provider model was not found", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn get_provider_model(
    State(state): State<Arc<AppState>>,
    Path((provider, model_name)): Path<(ProviderKindDto, String)>,
) -> Result<Response, ApiError> {
    let model = studio(&state)?
        .model(provider.into(), &model_name)
        .await
        .map_err(map_studio_error)?;
    let view = model_view(&state, model.clone())?;
    versioned_response(StatusCode::OK, view, model.version, None)
}

/// Deletes one Studio Provider model when its strong ETag is current.
#[utoipa::path(
    delete,
    path = "/v1/providers/{provider}/models/{model_name}",
    tag = "Studio",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("model_name" = String, Path, description = "Provider-local model name"),
        ("If-Match" = String, Header, description = "one required current strong Studio entity tag")
    ),
    responses(
        (status = 204, description = "Studio Provider model deleted", body = ()),
        (status = 400, description = "path or If-Match header is invalid", body = ErrorResponse),
        (status = 404, description = "Studio Provider model was not found", body = ErrorResponse),
        (status = 412, description = "Studio entity tag is stale", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn delete_provider_model(
    State(state): State<Arc<AppState>>,
    Path((provider, model_name)): Path<(ProviderKindDto, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    studio(&state)?
        .delete_model(provider.into(), &model_name, if_match(&headers)?)
        .await
        .map_err(map_studio_error)?;
    state
        .refresh_studio_providers()
        .await
        .map_err(map_host_error)?;
    Ok(StatusCode::NO_CONTENT)
}

fn studio(state: &AppState) -> Result<&StudioStore, ApiError> {
    state
        .studio()
        .ok_or_else(|| ApiError::new(ErrorKind::StudioStoreUnavailable))
}

fn agent_input(
    agent_name: String,
    agent_version: stratum_core::AgentVersionTag,
    model: stratum_core::ModelId,
    model_parameters: serde_json::Map<String, serde_json::Value>,
    tools: Vec<stratum_core::ToolName>,
    prompt: String,
) -> Result<AgentDefinitionInput, ApiError> {
    Ok(AgentDefinitionInput {
        agent_name: parse_agent_name(&agent_name)?,
        agent_version,
        model: ModelConfig::new(model, model_parameters),
        tools,
        prompt,
    })
}

fn parse_agent_name(value: &str) -> Result<AgentName, ApiError> {
    value
        .parse()
        .map_err(|_| ApiError::new(ErrorKind::InvalidStudioResource))
}

fn agent_view(value: AgentDefinition) -> AgentDefinitionView {
    AgentDefinitionView {
        agent_name: value.agent_name.to_string(),
        agent_version: value.agent_version,
        model: value.model.model,
        model_parameters: value.model.parameters,
        tools: value.tools,
        prompt: value.prompt,
        updated_at: value.updated_at,
    }
}

fn provider_view(value: ProviderSummary) -> ProviderView {
    ProviderView {
        provider: value.kind.into(),
        credential_configured: value.credential_configured,
        models_count: value.models_count,
        updated_at: value.updated_at,
    }
}

fn model_view(state: &AppState, value: Versioned<ManagedModel>) -> Result<ModelView, ApiError> {
    let provider = state
        .providers()
        .get(&value.value.model)
        .map_err(|error| match error {
            LlmError::ProviderNotFound { .. } => ApiError::new(ErrorKind::RuntimeUnavailable),
            other => ApiError::with_source(ErrorKind::Internal, other),
        })?;
    Ok(ModelView {
        model_id: value.value.model,
        provider: value.value.provider.into(),
        name: value.value.name,
        parameter_schema: provider.parameter_schema(),
        updated_at: value.value.updated_at,
    })
}

fn if_match(headers: &HeaderMap) -> Result<ResourceVersion, ApiError> {
    headers
        .get(IF_MATCH)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::new(ErrorKind::StudioPreconditionRequired))?
        .parse()
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))
}

fn json<T>(request: Result<Json<T>, JsonRejection>) -> Result<T, ApiError> {
    request.map(|Json(value)| value).map_err(|rejection| {
        if rejection.status() == StatusCode::PAYLOAD_TOO_LARGE {
            ApiError::new(ErrorKind::RequestTooLarge)
        } else {
            ApiError::new(ErrorKind::InvalidRequest)
        }
    })
}

fn map_studio_error(error: StudioError) -> ApiError {
    let kind = match error {
        StudioError::NotFound => ErrorKind::AgentTemplateNotFound,
        StudioError::AlreadyExists
        | StudioError::AgentVersionUnchanged
        | StudioError::DeletionBlocked { .. } => ErrorKind::StudioConflict,
        StudioError::PreconditionFailed => ErrorKind::StudioPreconditionFailed,
        StudioError::InvalidInput { .. } | StudioError::ModelNotConfigured => {
            ErrorKind::InvalidStudioResource
        }
        StudioError::Database(_) | StudioError::Migration(_) | StudioError::NotInitialized => {
            ErrorKind::StudioStoreUnavailable
        }
        StudioError::CatalogCorrupt { .. } => ErrorKind::Internal,
        _ => ErrorKind::Internal,
    };
    ApiError::with_source(kind, error)
}

fn map_host_error(error: crate::HostError) -> ApiError {
    ApiError::with_source(ErrorKind::StudioStoreUnavailable, error)
}

fn page(page: usize, per_page: usize) -> Result<(usize, usize), ApiError> {
    if page == 0 || per_page == 0 || per_page > MAX_PER_PAGE {
        Err(ApiError::new(ErrorKind::InvalidRequest))
    } else {
        Ok((page, per_page))
    }
}

struct Page<T> {
    data: Vec<T>,
    pagination: PaginationView,
}

fn page_response<T>(mut data: Vec<T>, page: usize, per_page: usize) -> Page<T> {
    let total = data.len();
    let start = page.saturating_sub(1).saturating_mul(per_page);
    if start >= total {
        data.clear();
    } else {
        let end = start.saturating_add(per_page).min(total);
        data = data.drain(start..end).collect();
    }
    Page {
        data,
        pagination: PaginationView {
            page,
            per_page,
            total,
        },
    }
}

fn versioned_response<T: serde::Serialize>(
    status: StatusCode,
    body: T,
    version: ResourceVersion,
    location: Option<&str>,
) -> Result<Response, ApiError> {
    let etag =
        HeaderValue::from_str(&version.etag()).map_err(|_| ApiError::new(ErrorKind::Internal))?;
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(ETAG, etag);
    if let Some(location) = location {
        let location =
            HeaderValue::from_str(location).map_err(|_| ApiError::new(ErrorKind::Internal))?;
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

impl From<ProviderKindDto> for ProviderKind {
    fn from(value: ProviderKindDto) -> Self {
        match value {
            ProviderKindDto::Openai => Self::Openai,
            ProviderKindDto::Deepseek => Self::Deepseek,
        }
    }
}

impl From<ProviderKind> for ProviderKindDto {
    fn from(value: ProviderKind) -> Self {
        match value {
            ProviderKind::Openai => Self::Openai,
            ProviderKind::Deepseek => Self::Deepseek,
        }
    }
}
