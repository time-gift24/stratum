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

use crate::{
    ApiError, AppState, ErrorKind,
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
