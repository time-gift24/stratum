//! Loopback-only HTTP boundary for the mutable Studio catalog.
//!
//! Routes are attached only when `[studio].management_enabled` has passed the
//! loopback validation in `stratum-config`. The Studio database itself is
//! always present because it is the runtime catalog. This module never returns
//! credentials: the only secret-bearing values are write-only request fields
//! forwarded straight to `stratum-studio`.

use std::sync::Arc;

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
    routing::get,
};
use serde::{Deserialize, de::DeserializeOwned};
use stratum_core::{AgentName, DangerLevel, ModelConfig, ToolKind, ToolName};
use stratum_llm::LlmError;
use stratum_studio::{
    AgentDefinition, AgentDefinitionInput, ManagedModel, ProviderKind, ProviderSummary,
    ResourceVersion, StudioError, Versioned,
};
use utoipa::OpenApi;

use crate::{
    ApiError, AppState, ErrorKind, ModelProbeError,
    error::ErrorResponse,
    management_dto::{
        AgentDefinitionView, AgentDefinitionsPage, CreateAgentDefinitionRequest,
        CreateModelRequest, CreateProviderRequest, DangerLevelDto, ModelTestResult, ModelView,
        ModelsPage, PaginationView, ProviderKindDto, ProviderView, ProvidersPage, ToolKindDto,
        ToolView, UpdateAgentDefinitionRequest, UpdateProviderRequest,
    },
    turn::build_tool_registry,
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
        test_provider_model,
        list_tools,
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
        ModelTestResult,
        ToolView,
        ToolKindDto,
        DangerLevelDto,
        ErrorResponse,
    ))
)]
struct StudioApiDoc;

/// Returns the management OpenAPI fragment when Studio is enabled.
pub(crate) fn openapi() -> utoipa::openapi::OpenApi {
    StudioApiDoc::openapi()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageParams {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_per_page")]
    per_page: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AgentPageParams {
    #[serde(default = "default_page")]
    page: usize,
    #[serde(default = "default_per_page")]
    per_page: usize,
    #[serde(default)]
    search: Option<String>,
    #[serde(default, rename = "sort")]
    _sort: Option<AgentSort>,
}

#[derive(Deserialize)]
enum AgentSort {
    #[serde(rename = "-updated_at")]
    UpdatedAtDescending,
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
        .route(
            "/v1/providers/{provider}/models/{model_name}/test",
            axum::routing::post(test_provider_model),
        )
        .route("/v1/tools", get(list_tools))
}

/// Lists Studio Agent definitions with deterministic pagination.
#[utoipa::path(
    get,
    path = "/v1/agent-definitions",
    tag = "Studio",
    params(
        ("page" = Option<usize>, Query, description = "one-based page number; defaults to 1"),
        ("per_page" = Option<usize>, Query, description = "page size from 1 through 100; defaults to 20"),
        ("search" = Option<String>, Query, description = "case-insensitive Agent name substring"),
        ("sort" = Option<String>, Query, description = "fixed Agent ordering; only `-updated_at` is accepted")
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
    query: Result<Query<AgentPageParams>, QueryRejection>,
) -> Result<Json<AgentDefinitionsPage>, ApiError> {
    let query = parse_query(query)?;
    let (page, per_page) = page(query.page, query.per_page)?;
    let offset = page.saturating_sub(1).saturating_mul(per_page);
    let (entries, total) = state
        .studio()
        .page_agent_definitions(query.search.as_deref(), offset, per_page)
        .await
        .map_err(map_agent_error)?;
    let data = entries
        .into_iter()
        .map(|entry| agent_view(entry.value))
        .collect::<Vec<_>>();
    Ok(Json(AgentDefinitionsPage {
        data,
        pagination: PaginationView {
            page,
            per_page,
            total,
        },
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
    state.validate_studio_definition(&input).await?;
    let created = state
        .studio()
        .create_agent_definition(input)
        .await
        .map_err(map_agent_error)?;
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
        (status = 400, description = "Agent name is invalid", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn get_agent_definition(
    State(state): State<Arc<AppState>>,
    agent_name: Result<Path<String>, PathRejection>,
) -> Result<Response, ApiError> {
    let agent_name = path(agent_name)?;
    let agent_name = parse_agent_name(&agent_name)?;
    let definition = state
        .studio()
        .agent_definition(&agent_name)
        .await
        .map_err(map_agent_error)?;
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
    agent_name: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
    request: Result<Json<UpdateAgentDefinitionRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let agent_name = path(agent_name)?;
    let request = json(request)?;
    let input = agent_input(
        agent_name,
        request.agent_version,
        request.model,
        request.model_parameters,
        request.tools,
        request.prompt,
    )?;
    state.validate_studio_definition(&input).await?;
    let updated = state
        .studio()
        .replace_agent_definition(input, if_match(&headers)?)
        .await
        .map_err(map_agent_error)?;
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
        (status = 412, description = "Studio entity tag is stale", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store is unavailable", body = ErrorResponse),
    )
)]
async fn delete_agent_definition(
    State(state): State<Arc<AppState>>,
    agent_name: Result<Path<String>, PathRejection>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let agent_name = path(agent_name)?;
    let agent_name = parse_agent_name(&agent_name)?;
    state
        .studio()
        .delete_agent_definition(&agent_name, if_match(&headers)?)
        .await
        .map_err(map_agent_error)?;
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
    query: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<ProvidersPage>, ApiError> {
    let query = parse_query(query)?;
    let (page, per_page) = page(query.page, query.per_page)?;
    let data = state
        .studio()
        .list_providers()
        .await
        .map_err(map_provider_error)?
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
    let provider = state
        .studio()
        .create_provider(request.provider.into(), request.api_key)
        .await
        .map_err(map_provider_error)?;
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
    provider: Result<Path<ProviderKindDto>, PathRejection>,
) -> Result<Response, ApiError> {
    let provider = path(provider)?;
    let provider = state
        .studio()
        .provider(provider.into())
        .await
        .map_err(map_provider_error)?;
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
        (status = 422, description = "replacement credential is invalid", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn update_provider(
    State(state): State<Arc<AppState>>,
    provider: Result<Path<ProviderKindDto>, PathRejection>,
    headers: HeaderMap,
    request: Result<Json<UpdateProviderRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let provider = path(provider)?;
    let request = json(request)?;
    let kind: ProviderKind = provider.into();
    let expected = if_match(&headers)?;
    let updated = match request.api_key {
        Some(api_key) => state
            .studio()
            .replace_provider_credential(kind, api_key, expected)
            .await
            .map_err(map_provider_error)?,
        None => {
            let current = state
                .studio()
                .provider(kind)
                .await
                .map_err(map_provider_error)?;
            if current.version != expected {
                return Err(map_provider_error(StudioError::PreconditionFailed));
            }
            current
        }
    };
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
        (status = 409, description = "Provider remains referenced by an Agent definition", body = ErrorResponse),
        (status = 412, description = "Studio entity tag is stale", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn delete_provider(
    State(state): State<Arc<AppState>>,
    provider: Result<Path<ProviderKindDto>, PathRejection>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let provider = path(provider)?;
    state
        .studio()
        .delete_provider(provider.into(), if_match(&headers)?)
        .await
        .map_err(map_provider_error)?;
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
    provider: Result<Path<ProviderKindDto>, PathRejection>,
    query: Result<Query<PageParams>, QueryRejection>,
) -> Result<Json<ModelsPage>, ApiError> {
    let provider = path(provider)?;
    let query = parse_query(query)?;
    let (page, per_page) = page(query.page, query.per_page)?;
    let kind: ProviderKind = provider.into();
    let snapshot = state.studio_model_catalog().await.map_err(map_host_error)?;
    let models = snapshot
        .models
        .into_iter()
        .filter(|entry| entry.value.provider == kind)
        .collect::<Vec<_>>();
    let response = page_response(models, page, per_page);
    let model_ids = response
        .data
        .iter()
        .map(|entry| entry.value.model.clone())
        .collect::<Vec<_>>();
    let providers = state
        .providers_for_studio_models(snapshot.runtime_providers, &model_ids)
        .map_err(map_host_error)?;
    let data = response
        .data
        .into_iter()
        .map(|entry| -> Result<ModelView, ApiError> {
            let schema = model_parameter_schema(&providers, &entry.value.model)?;
            Ok(model_view(entry, schema))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Json(ModelsPage {
        data,
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
        (status = 404, description = "Studio Provider was not found", body = ErrorResponse),
        (status = 409, description = "model already exists", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "model is invalid or not supported by the Provider adapter", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn create_provider_model(
    State(state): State<Arc<AppState>>,
    provider: Result<Path<ProviderKindDto>, PathRejection>,
    request: Result<Json<CreateModelRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let provider = path(provider)?;
    let request = json(request)?;
    let kind: ProviderKind = provider.into();
    state
        .validate_studio_model(kind, &request.name)
        .map_err(map_model_validation_error)?;
    let parameter_schema = state
        .prospective_studio_model_schema(kind, &request.name)
        .await
        .map_err(map_prospective_model_error)?;
    let model = state
        .studio()
        .create_model(kind, request.name)
        .await
        .map_err(map_create_model_error)?;
    let location = format!(
        "/v1/providers/{}/models/{}",
        model.value.provider,
        encode_path_segment(&model.value.name)
    );
    let version = model.version;
    let view = model_view(model, parameter_schema);
    versioned_response(StatusCode::CREATED, view, version, Some(&location))
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
    path_value: Result<Path<(ProviderKindDto, String)>, PathRejection>,
) -> Result<Response, ApiError> {
    let (provider, model_name) = path(path_value)?;
    let provider: ProviderKind = provider.into();
    let snapshot = state.studio_model_catalog().await.map_err(map_host_error)?;
    let model = snapshot
        .models
        .into_iter()
        .find(|entry| entry.value.provider == provider && entry.value.name == model_name)
        .ok_or_else(|| map_model_error(StudioError::NotFound))?;
    let providers = state
        .providers_for_studio_models(
            snapshot.runtime_providers,
            std::slice::from_ref(&model.value.model),
        )
        .map_err(map_host_error)?;
    let schema = model_parameter_schema(&providers, &model.value.model)?;
    let version = model.version;
    let view = model_view(model, schema);
    versioned_response(StatusCode::OK, view, version, None)
}

/// Sends one real minimal message through a configured Studio Provider model.
#[utoipa::path(
    post,
    path = "/v1/providers/{provider}/models/{model_name}/test",
    tag = "Studio",
    params(
        ("provider" = ProviderKindDto, Path, description = "supported Provider kind"),
        ("model_name" = String, Path, description = "Provider-local model name")
    ),
    responses(
        (status = 200, description = "the model answered the real test message", body = ModelTestResult),
        (status = 400, description = "path is invalid", body = ErrorResponse),
        (status = 404, description = "Studio Provider or model was not found", body = ErrorResponse),
        (status = 502, description = "Provider rejected, or did not answer, the test message", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn test_provider_model(
    State(state): State<Arc<AppState>>,
    path_value: Result<Path<(ProviderKindDto, String)>, PathRejection>,
) -> Result<Json<ModelTestResult>, ApiError> {
    let (provider, model_name) = path(path_value)?;
    let kind: ProviderKind = provider.into();
    let latency_ms = state
        .test_studio_model(kind, &model_name)
        .await
        .map_err(map_model_probe_error)?;
    tracing::info!(
        provider = kind.as_str(),
        outcome = "success",
        "provider model message test completed"
    );
    Ok(Json(ModelTestResult { latency_ms }))
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
        (status = 409, description = "Agent definitions still reference this Model", body = ErrorResponse),
        (status = 412, description = "Studio entity tag is stale", body = ErrorResponse),
        (status = 422, description = "model name is invalid", body = ErrorResponse),
        (status = 428, description = "If-Match header is required", body = ErrorResponse),
        (status = 500, description = "Studio catalog is corrupt", body = ErrorResponse),
        (status = 503, description = "Studio store or runtime Provider catalog is unavailable", body = ErrorResponse),
    )
)]
async fn delete_provider_model(
    State(state): State<Arc<AppState>>,
    path_value: Result<Path<(ProviderKindDto, String)>, PathRejection>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let (provider, model_name) = path(path_value)?;
    state
        .studio()
        .delete_model(provider.into(), &model_name, if_match(&headers)?)
        .await
        .map_err(map_model_error)?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists the tools that this host can actually register for an Agent.
#[utoipa::path(
    get,
    path = "/v1/tools",
    tag = "Studio",
    responses(
        (status = 200, description = "host tool catalog", body = [ToolView]),
        (status = 500, description = "tool catalog could not be assembled", body = ErrorResponse),
    )
)]
async fn list_tools() -> Result<Json<Vec<ToolView>>, ApiError> {
    let registry = build_tool_registry(&[ToolName::from("echo")])?;
    let mut tools = Vec::new();
    for spec in registry.specs() {
        let Some((kind, danger_level)) = registry
            .authorization(&spec.name)
            .map_err(|error| ApiError::with_source(ErrorKind::Internal, error))?
        else {
            return Err(ApiError::new(ErrorKind::Internal));
        };
        tools.push(ToolView {
            name: spec.name.to_string(),
            description: spec.description,
            kind: tool_kind(kind)?,
            danger_level: danger_level_view(danger_level)?,
        });
    }
    Ok(Json(tools))
}

fn tool_kind(value: ToolKind) -> Result<ToolKindDto, ApiError> {
    match value {
        ToolKind::Read => Ok(ToolKindDto::Read),
        ToolKind::Write => Ok(ToolKindDto::Write),
        _ => Err(ApiError::new(ErrorKind::Internal)),
    }
}

fn danger_level_view(value: DangerLevel) -> Result<DangerLevelDto, ApiError> {
    match value {
        DangerLevel::Low => Ok(DangerLevelDto::Low),
        DangerLevel::Medium => Ok(DangerLevelDto::Medium),
        DangerLevel::High => Ok(DangerLevelDto::High),
        _ => Err(ApiError::new(ErrorKind::Internal)),
    }
}

fn agent_input(
    agent_name: String,
    agent_version: String,
    model: stratum_core::ModelId,
    model_parameters: serde_json::Map<String, serde_json::Value>,
    tools: Vec<stratum_core::ToolName>,
    prompt: String,
) -> Result<AgentDefinitionInput, ApiError> {
    Ok(AgentDefinitionInput {
        agent_name: parse_agent_name(&agent_name)?,
        agent_version: parse_agent_version(agent_version)?,
        model: ModelConfig::new(model, model_parameters),
        tools,
        prompt,
    })
}

fn parse_agent_name(value: &str) -> Result<AgentName, ApiError> {
    value.parse().map_err(|error| {
        ApiError::with_field_violation(ErrorKind::InvalidRequest, "agent_name", error)
    })
}

fn parse_agent_version(value: String) -> Result<stratum_core::AgentVersionTag, ApiError> {
    stratum_core::AgentVersionTag::new(value).map_err(|error| {
        ApiError::with_field_violation(ErrorKind::InvalidStudioResource, "agent_version", error)
    })
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

fn model_parameter_schema(
    providers: &stratum_llm::LlmProviderManager,
    model: &stratum_core::ModelId,
) -> Result<serde_json::Value, ApiError> {
    providers
        .get(model)
        .map_err(|error| match error {
            LlmError::ProviderNotFound { .. } => ApiError::new(ErrorKind::RuntimeUnavailable),
            other => ApiError::with_source(ErrorKind::Internal, other),
        })
        .map(|provider| provider.parameter_schema())
}

fn model_view(value: Versioned<ManagedModel>, parameter_schema: serde_json::Value) -> ModelView {
    ModelView {
        model_id: value.value.model,
        provider: value.value.provider.into(),
        name: value.value.name,
        parameter_schema,
        updated_at: value.value.updated_at,
    }
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

fn path<T: DeserializeOwned>(request: Result<Path<T>, PathRejection>) -> Result<T, ApiError> {
    request
        .map(|Path(value)| value)
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))
}

fn parse_query<T: DeserializeOwned>(
    request: Result<Query<T>, QueryRejection>,
) -> Result<T, ApiError> {
    request
        .map(|Query(value)| value)
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))
}

fn map_agent_error(error: StudioError) -> ApiError {
    map_studio_error(error, ErrorKind::AgentTemplateNotFound)
}

fn map_provider_error(error: StudioError) -> ApiError {
    map_studio_error(error, ErrorKind::ProviderNotFound)
}

fn map_model_error(error: StudioError) -> ApiError {
    map_studio_error(error, ErrorKind::ManagedModelNotFound)
}

fn map_create_model_error(error: StudioError) -> ApiError {
    match error {
        StudioError::NotFound => map_provider_error(StudioError::NotFound),
        other => map_model_error(other),
    }
}

fn map_studio_error(error: StudioError, not_found: ErrorKind) -> ApiError {
    let kind = match &error {
        StudioError::NotFound => not_found,
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
    ApiError::from_studio(kind, error)
}

fn map_model_probe_error(error: ModelProbeError) -> ApiError {
    match error {
        ModelProbeError::Studio(error) => map_provider_error(error),
        ModelProbeError::ModelNotConfigured => {
            ApiError::with_source(ErrorKind::ManagedModelNotFound, error)
        }
        ModelProbeError::Adapter(source) => map_host_error(source),
        other => ApiError::with_source(ErrorKind::ProviderTestFailed, other),
    }
}

fn map_host_error(error: crate::HostError) -> ApiError {
    match error {
        crate::HostError::Studio(error) => {
            let kind = match error {
                StudioError::Database(_)
                | StudioError::Migration(_)
                | StudioError::NotInitialized => ErrorKind::StudioStoreUnavailable,
                _ => ErrorKind::Internal,
            };
            ApiError::from_studio(kind, error)
        }
        other => ApiError::with_source(ErrorKind::RuntimeUnavailable, other),
    }
}

fn map_model_validation_error(error: crate::HostError) -> ApiError {
    ApiError::with_source(ErrorKind::InvalidStudioResource, error)
}

fn map_prospective_model_error(error: crate::HostError) -> ApiError {
    match error {
        error @ crate::HostError::Studio(StudioError::NotFound) => {
            ApiError::with_source(ErrorKind::ProviderNotFound, error)
        }
        other => map_host_error(other),
    }
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

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, StatusCode, header::CONTENT_TYPE},
        routing::get,
    };
    use serde_json::{Value, json};
    use stratum_studio::DeletionBlocker;
    use tower::ServiceExt;

    use super::*;

    async fn provider_path_boundary(
        request: Result<Path<ProviderKindDto>, PathRejection>,
    ) -> Result<StatusCode, ApiError> {
        let _provider = path(request)?;
        Ok(StatusCode::NO_CONTENT)
    }

    async fn page_query_boundary(
        request: Result<Query<PageParams>, QueryRejection>,
    ) -> Result<StatusCode, ApiError> {
        let _query = parse_query(request)?;
        Ok(StatusCode::NO_CONTENT)
    }

    async fn agent_page_query_boundary(
        request: Result<Query<AgentPageParams>, QueryRejection>,
    ) -> Result<StatusCode, ApiError> {
        let _query = parse_query(request)?;
        Ok(StatusCode::NO_CONTENT)
    }

    #[tokio::test]
    async fn malformed_studio_enum_path_returns_the_json_error_envelope() {
        let app = Router::new().route("/providers/{provider}", get(provider_path_boundary));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/providers/unsupported")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert_json_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
    }

    #[tokio::test]
    async fn malformed_studio_query_returns_the_json_error_envelope() {
        let app = Router::new().route("/agents", get(page_query_boundary));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/agents?page=not-a-number")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert_json_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
    }

    #[tokio::test]
    async fn unknown_studio_query_returns_the_json_error_envelope() {
        let app = Router::new().route("/agents", get(page_query_boundary));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/agents?unknown=value")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert_json_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
    }

    #[tokio::test]
    async fn agent_list_accepts_the_documented_updated_at_sort() {
        let app = Router::new().route("/agents", get(agent_page_query_boundary));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/agents?sort=-updated_at")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn agent_list_rejects_an_unsupported_sort() {
        let app = Router::new().route("/agents", get(agent_page_query_boundary));

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/agents?sort=agent_name")
                    .body(Body::empty())
                    .expect("request builds"),
            )
            .await
            .expect("router responds");

        assert_json_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
    }

    #[tokio::test]
    async fn invalid_agent_name_is_a_bad_request_with_a_field_violation() {
        let response = parse_agent_name("not a valid agent name")
            .expect_err("invalid name is rejected")
            .into_response();
        let body = json_error_body(response, StatusCode::BAD_REQUEST).await;

        assert_eq!(body["error"]["code"], "invalid_request");
        assert_eq!(
            body["error"]["violations"],
            json!([{
                "field": "agent_name",
                "code": "invalid_field",
                "message": "the field is invalid"
            }])
        );
    }

    #[tokio::test]
    async fn invalid_agent_version_is_unprocessable_with_a_field_violation() {
        let response = parse_agent_version(" release-2 ".to_owned())
            .expect_err("surrounding whitespace is rejected")
            .into_response();
        let body = json_error_body(response, StatusCode::UNPROCESSABLE_ENTITY).await;

        assert_eq!(body["error"]["code"], "invalid_studio_resource");
        assert_eq!(
            body["error"]["violations"],
            json!([{
                "field": "agent_version",
                "code": "invalid_field",
                "message": "the field is invalid"
            }])
        );
    }

    #[tokio::test]
    async fn studio_not_found_errors_keep_resource_specific_codes() {
        let cases = [
            (
                map_agent_error(StudioError::NotFound),
                "agent_template_not_found",
            ),
            (
                map_provider_error(StudioError::NotFound),
                "provider_not_found",
            ),
            (
                map_model_error(StudioError::NotFound),
                "managed_model_not_found",
            ),
            (
                map_create_model_error(StudioError::NotFound),
                "provider_not_found",
            ),
            (
                map_prospective_model_error(crate::HostError::Studio(StudioError::NotFound)),
                "provider_not_found",
            ),
        ];

        for (error, code) in cases {
            assert_json_error(error.into_response(), StatusCode::NOT_FOUND, code).await;
        }
    }

    #[tokio::test]
    async fn model_probe_errors_keep_stable_statuses_and_codes() {
        let not_found = [
            map_model_probe_error(ModelProbeError::Studio(StudioError::NotFound)),
            map_model_probe_error(ModelProbeError::ModelNotConfigured),
        ];
        let expected_codes = ["provider_not_found", "managed_model_not_found"];
        for (error, code) in not_found.into_iter().zip(expected_codes) {
            assert_json_error(error.into_response(), StatusCode::NOT_FOUND, code).await;
        }

        let upstream_failures = [
            ModelProbeError::Credentials,
            ModelProbeError::ModelNotAvailable,
            ModelProbeError::Failed,
        ];
        for error in upstream_failures {
            assert_json_error(
                map_model_probe_error(error).into_response(),
                StatusCode::BAD_GATEWAY,
                "provider_test_failed",
            )
            .await;
        }
    }

    #[tokio::test]
    async fn studio_invalid_input_returns_its_field_violation() {
        let response =
            map_agent_error(StudioError::InvalidInput { field: "prompt" }).into_response();
        let body = json_error_body(response, StatusCode::UNPROCESSABLE_ENTITY).await;

        assert_eq!(
            body["error"]["violations"],
            json!([{
                "field": "prompt",
                "code": "invalid_field",
                "message": "the field is invalid"
            }])
        );
    }

    #[tokio::test]
    async fn studio_deletion_blocked_returns_safe_resource_blockers() {
        let response = map_provider_error(StudioError::DeletionBlocked {
            blockers: vec![DeletionBlocker {
                resource: "agent_definition",
                name: "research-agent".to_owned(),
            }],
        })
        .into_response();
        let body = json_error_body(response, StatusCode::CONFLICT).await;

        assert_eq!(body["error"]["code"], "studio_conflict");
        assert_eq!(
            body["error"]["blockers"],
            json!([{
                "resource_type": "agent_definition",
                "name": "research-agent"
            }])
        );
    }

    #[tokio::test]
    async fn tools_endpoint_projects_the_real_echo_catalog() {
        let Json(tools) = list_tools().await.expect("builtin catalog assembles");

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "echo");
        assert_eq!(tools[0].description, "returns input arguments");
        assert_eq!(tools[0].kind, ToolKindDto::Read);
        assert_eq!(tools[0].danger_level, DangerLevelDto::Low);
    }

    async fn assert_json_error(
        response: Response,
        expected_status: StatusCode,
        expected_code: &str,
    ) {
        let body = json_error_body(response, expected_status).await;
        assert_eq!(body["error"]["code"], expected_code);
        assert!(body["error"]["message"].is_string());
    }

    async fn json_error_body(response: Response, expected_status: StatusCode) -> Value {
        assert_eq!(response.status(), expected_status);
        assert_eq!(
            response.headers().get(CONTENT_TYPE),
            Some(&HeaderValue::from_static("application/json"))
        );
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body reads");
        serde_json::from_slice(&body).expect("response is JSON")
    }
}
