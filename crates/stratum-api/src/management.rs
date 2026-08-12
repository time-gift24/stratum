//! Concrete management operations over Agent templates and one managed LLM catalog.

use std::{sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use stratum_config::{
    AgentDefinitionConfig, AgentName, ManagedLlmCatalog, ManagedProviderConfig, ProviderKind,
    strong_etag,
};
use stratum_core::{ModelConfig, ModelId};
use stratum_store::StoreError;

use crate::{
    HostError, HostState,
    management_dto::{
        AgentDefinitionView, AgentDefinitionsPage, FieldViolation, ModelView, ModelsPage,
        PaginationView, ProviderKindDto, ProviderTestView, ProviderView, ProvidersPage,
        ResourceBlocker,
    },
    providers_from_catalog,
};

const MAX_PAGE_SIZE: usize = 100;
const PROVIDER_TEST_TIMEOUT: Duration = Duration::from_secs(10);

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

impl HostState {
    pub(crate) async fn list_agent_definitions(
        &self,
        page: usize,
        per_page: usize,
        search: Option<&str>,
        descending: bool,
    ) -> Result<AgentDefinitionsPage, HostError> {
        let (page, per_page) = validate_page(page, per_page)?;
        let agent_names = self.configuration_store.list_agent_definitions().await?;
        let search = search.map(str::trim).filter(|query| !query.is_empty());
        let mut data = Vec::with_capacity(agent_names.len());
        for agent_name in agent_names {
            if search.is_some_and(|query| {
                !agent_name
                    .as_str()
                    .to_lowercase()
                    .contains(&query.to_lowercase())
            }) {
                continue;
            }
            let (view, _) = self.read_agent_definition_inner(agent_name).await?;
            data.push(view);
        }
        data.sort_unstable_by(|left, right| {
            let ordering = left.updated_at.cmp(&right.updated_at);
            if descending {
                ordering.reverse()
            } else {
                ordering
            }
            .then_with(|| left.agent_name.cmp(&right.agent_name))
        });
        let total = data.len();
        let data = page_slice(data, page, per_page);
        Ok(AgentDefinitionsPage {
            data,
            pagination: PaginationView {
                page,
                per_page,
                total,
            },
        })
    }

    pub(crate) async fn read_agent_definition(
        &self,
        agent_name: AgentName,
    ) -> Result<(AgentDefinitionView, String), HostError> {
        self.read_agent_definition_inner(agent_name).await
    }

    #[tracing::instrument(skip_all, fields(agent_name = %agent_name.as_str()))]
    pub(crate) async fn create_agent_definition(
        &self,
        agent_name: AgentName,
        definition: AgentDefinitionConfig,
    ) -> Result<(AgentDefinitionView, String), HostError> {
        let _write = self.management_write.lock().await;
        self.validate_agent_definition(&agent_name, &definition)?;
        let encoded = definition.encode()?;
        match self
            .configuration_store
            .create_agent_definition(&agent_name, encoded.into_bytes())
            .await
        {
            Ok(()) => self.read_agent_definition_inner(agent_name).await,
            Err(StoreError::ConfigurationAlreadyExists) => Err(HostError::ResourceAlreadyExists),
            Err(error) => Err(error.into()),
        }
    }

    #[tracing::instrument(skip_all, fields(agent_name = %agent_name.as_str()))]
    pub(crate) async fn update_agent_definition(
        &self,
        agent_name: AgentName,
        definition: AgentDefinitionConfig,
        if_match: &str,
    ) -> Result<(AgentDefinitionView, String), HostError> {
        let _write = self.management_write.lock().await;
        self.validate_agent_definition(&agent_name, &definition)?;
        let current = self
            .configuration_store
            .read_agent_definition(&agent_name)
            .await?
            .ok_or_else(|| HostError::AgentDefinitionNotFound {
                agent_name: agent_name.clone(),
            })?;
        let current_etag = strong_etag(current.contents());
        require_matching_etag(if_match, &current_etag)?;
        let encoded = definition.encode()?;
        self.configuration_store
            .replace_agent_definition(&agent_name, encoded.into_bytes(), current.revision())
            .await
            .map_err(|error| match error {
                StoreError::ConfigurationRevisionMismatch => HostError::RevisionConflict,
                other => other.into(),
            })?;
        self.read_agent_definition_inner(agent_name).await
    }

    #[tracing::instrument(skip_all, fields(agent_name = %agent_name.as_str()))]
    pub(crate) async fn delete_agent_definition(
        &self,
        agent_name: AgentName,
        if_match: &str,
    ) -> Result<(), HostError> {
        let _write = self.management_write.lock().await;
        let current = self
            .configuration_store
            .read_agent_definition(&agent_name)
            .await?
            .ok_or_else(|| HostError::AgentDefinitionNotFound {
                agent_name: agent_name.clone(),
            })?;
        require_matching_etag(if_match, &strong_etag(current.contents()))?;
        self.configuration_store
            .delete_agent_definition(&agent_name, current.revision())
            .await
            .map_err(|error| match error {
                StoreError::ConfigurationRevisionMismatch => HostError::RevisionConflict,
                other => other.into(),
            })?;
        Ok(())
    }

    pub(crate) async fn list_providers(
        &self,
        page: usize,
        per_page: usize,
    ) -> Result<ProvidersPage, HostError> {
        let (page, per_page) = validate_page(page, per_page)?;
        let catalog = self.catalog();
        let updated_at = self.catalog_updated_at().await;
        let mut data = [ProviderKind::Openai, ProviderKind::Deepseek]
            .into_iter()
            .filter_map(|kind| {
                catalog.provider(kind).map(|provider| ProviderView {
                    provider: kind.into(),
                    credential_configured: !provider.api_key().trim().is_empty(),
                    models_count: provider.models.len(),
                    updated_at,
                })
            })
            .collect::<Vec<_>>();
        data.sort_unstable_by_key(|provider| match provider.provider {
            ProviderKindDto::Openai => 0,
            ProviderKindDto::Deepseek => 1,
        });
        let total = data.len();
        Ok(ProvidersPage {
            data: page_slice(data, page, per_page),
            pagination: PaginationView {
                page,
                per_page,
                total,
            },
        })
    }

    pub(crate) async fn read_provider(
        &self,
        kind: ProviderKind,
    ) -> Result<(ProviderView, String), HostError> {
        let catalog = self.catalog();
        let provider = catalog
            .provider(kind)
            .ok_or(HostError::ProviderNotFound { provider: kind })?;
        let view = ProviderView {
            provider: kind.into(),
            credential_configured: !provider.api_key().trim().is_empty(),
            models_count: provider.models.len(),
            updated_at: self.catalog_updated_at().await,
        };
        Ok((view, provider_etag(catalog.revision, kind, provider)))
    }

    #[tracing::instrument(skip_all, fields(provider = kind.as_str()))]
    pub(crate) async fn create_provider(
        &self,
        kind: ProviderKind,
        api_key: SecretString,
    ) -> Result<(ProviderView, String), HostError> {
        let _write = self.management_write.lock().await;
        if api_key.expose_secret().trim().is_empty() {
            return Err(field_error(
                "api_key",
                "required",
                "api key must not be blank",
            ));
        }
        let mut candidate = self.catalog();
        if candidate.provider(kind).is_some() {
            return Err(HostError::ResourceAlreadyExists);
        }
        *candidate.provider_mut(kind) = Some(ManagedProviderConfig::new(api_key, Vec::new()));
        self.commit_catalog(candidate).await?;
        self.read_provider(kind).await
    }

    #[tracing::instrument(skip_all, fields(provider = kind.as_str()))]
    pub(crate) async fn update_provider(
        &self,
        kind: ProviderKind,
        api_key: Option<SecretString>,
        if_match: &str,
    ) -> Result<(ProviderView, String), HostError> {
        let _write = self.management_write.lock().await;
        let mut candidate = self.catalog();
        let revision = candidate.revision;
        let provider = candidate
            .provider_mut(kind)
            .as_mut()
            .ok_or(HostError::ProviderNotFound { provider: kind })?;
        require_matching_etag(if_match, &provider_etag(revision, kind, provider))?;
        if let Some(api_key) = api_key {
            if api_key.expose_secret().trim().is_empty() {
                return Err(field_error(
                    "api_key",
                    "required",
                    "api key must not be blank",
                ));
            }
            provider.api_key = api_key;
        }
        self.commit_catalog(candidate).await?;
        self.read_provider(kind).await
    }

    #[tracing::instrument(skip_all, fields(provider = kind.as_str()))]
    pub(crate) async fn delete_provider(
        &self,
        kind: ProviderKind,
        if_match: &str,
    ) -> Result<(), HostError> {
        let _write = self.management_write.lock().await;
        let mut candidate = self.catalog();
        let revision = candidate.revision;
        let provider = candidate
            .provider(kind)
            .ok_or(HostError::ProviderNotFound { provider: kind })?;
        require_matching_etag(if_match, &provider_etag(revision, kind, provider))?;
        let blockers = self.provider_blockers(kind, &candidate).await?;
        if !blockers.is_empty() {
            return Err(HostError::ResourceConflict { blockers });
        }
        *candidate.provider_mut(kind) = None;
        self.commit_catalog(candidate).await
    }

    pub(crate) async fn list_provider_models(
        &self,
        kind: ProviderKind,
        page: usize,
        per_page: usize,
    ) -> Result<ModelsPage, HostError> {
        let (page, per_page) = validate_page(page, per_page)?;
        let catalog = self.catalog();
        let provider = catalog
            .provider(kind)
            .ok_or(HostError::ProviderNotFound { provider: kind })?;
        let updated_at = self.catalog_updated_at().await;
        let manager = self.provider_manager();
        let mut data = Vec::with_capacity(provider.models.len());
        for name in &provider.models {
            let model_id = ModelId::new(kind.as_str(), name)
                .map_err(|_| field_error("name", "invalid", "model name is invalid"))?;
            let schema = manager.get(&model_id)?.parameter_schema();
            data.push(ModelView {
                model_id,
                provider: kind.into(),
                name: name.clone(),
                parameter_schema: schema,
                updated_at,
            });
        }
        data.sort_unstable_by(|left, right| left.name.cmp(&right.name));
        let total = data.len();
        Ok(ModelsPage {
            data: page_slice(data, page, per_page),
            pagination: PaginationView {
                page,
                per_page,
                total,
            },
        })
    }

    pub(crate) async fn read_provider_model(
        &self,
        kind: ProviderKind,
        name: &str,
    ) -> Result<(ModelView, String), HostError> {
        let catalog = self.catalog();
        let provider = catalog
            .provider(kind)
            .ok_or(HostError::ProviderNotFound { provider: kind })?;
        if !provider.models.iter().any(|model| model == name) {
            return Err(HostError::ModelNotFound {
                model: model_id(kind, name)?,
            });
        }
        let model_id = model_id(kind, name)?;
        let parameter_schema = self.provider_manager().get(&model_id)?.parameter_schema();
        let view = ModelView {
            model_id,
            provider: kind.into(),
            name: name.to_owned(),
            parameter_schema,
            updated_at: self.catalog_updated_at().await,
        };
        Ok((view, model_etag(catalog.revision, kind, name)))
    }

    #[tracing::instrument(skip_all, fields(provider = kind.as_str(), model_name = %name))]
    pub(crate) async fn create_provider_model(
        &self,
        kind: ProviderKind,
        name: String,
    ) -> Result<(ModelView, String), HostError> {
        let _write = self.management_write.lock().await;
        let mut candidate = self.catalog();
        let provider = candidate
            .provider_mut(kind)
            .as_mut()
            .ok_or(HostError::ProviderNotFound { provider: kind })?;
        if provider.models.iter().any(|model| model == &name) {
            return Err(HostError::ResourceAlreadyExists);
        }
        validate_model_name(kind, &name)?;
        provider.models.push(name.clone());
        self.commit_catalog(candidate).await?;
        self.read_provider_model(kind, &name).await
    }

    #[tracing::instrument(skip_all, fields(provider = kind.as_str(), model_name = name))]
    pub(crate) async fn delete_provider_model(
        &self,
        kind: ProviderKind,
        name: &str,
        if_match: &str,
    ) -> Result<(), HostError> {
        let _write = self.management_write.lock().await;
        let mut candidate = self.catalog();
        let model = model_id(kind, name)?;
        let position = candidate
            .provider(kind)
            .ok_or(HostError::ProviderNotFound { provider: kind })?
            .models
            .iter()
            .position(|configured| configured == name)
            .ok_or_else(|| HostError::ModelNotFound {
                model: model.clone(),
            })?;
        require_matching_etag(if_match, &model_etag(candidate.revision, kind, name))?;
        let blockers = self.model_blockers(&model, &candidate).await?;
        if !blockers.is_empty() {
            return Err(HostError::ResourceConflict { blockers });
        }
        candidate
            .provider_mut(kind)
            .as_mut()
            .ok_or(HostError::ProviderNotFound { provider: kind })?
            .models
            .remove(position);
        self.commit_catalog(candidate).await
    }

    #[tracing::instrument(skip_all, fields(provider = kind.as_str()))]
    pub(crate) async fn test_provider(
        &self,
        kind: ProviderKind,
    ) -> Result<ProviderTestView, HostError> {
        let catalog = self.catalog();
        let provider = catalog
            .provider(kind)
            .ok_or(HostError::ProviderNotFound { provider: kind })?;
        let endpoint = match kind {
            ProviderKind::Openai => crate::OPENAI_BASE_URL,
            ProviderKind::Deepseek => crate::DEEPSEEK_BASE_URL,
        };
        let client = reqwest::Client::builder()
            .timeout(PROVIDER_TEST_TIMEOUT)
            .build()
            .map_err(|_| HostError::ProviderTestFailed)?;
        execute_provider_probe(&client, endpoint, provider.api_key()).await?;
        Ok(ProviderTestView {
            success: true,
            completed_at: Utc::now(),
        })
    }

    fn validate_agent_definition(
        &self,
        agent_name: &AgentName,
        definition: &AgentDefinitionConfig,
    ) -> Result<(), HostError> {
        let encoded = definition.encode()?;
        let (catalog, manager) = self.llm_snapshot();
        let resolved = catalog
            .resolve_definition(agent_name.clone(), &encoded)
            .map_err(|error| match error {
                stratum_config::ConfigError::ModelNotConfigured { .. } => {
                    field_error("model", "not_configured", "model is not configured")
                }
                other => other.into(),
            })?;
        let model_config = if resolved.model_parameters.is_empty() {
            manager.default_model_config(&resolved.model)?
        } else {
            ModelConfig::new(resolved.model.clone(), resolved.model_parameters.clone())
        };
        manager
            .configure(&model_config)
            .map_err(|error| match error {
                stratum_llm::LlmError::InvalidModelParameters { .. } => field_error(
                    "model_parameters",
                    "invalid",
                    "model parameters are invalid",
                ),
                other => other.into(),
            })?;
        super::host::tool_registry(&resolved).map_err(|error| match error {
            HostError::ToolNotAvailable { .. } => field_error(
                "tools",
                "not_available",
                "one or more tools are unavailable",
            ),
            other => other,
        })?;
        Ok(())
    }

    async fn read_agent_definition_inner(
        &self,
        agent_name: AgentName,
    ) -> Result<(AgentDefinitionView, String), HostError> {
        let resource = self
            .configuration_store
            .read_agent_definition(&agent_name)
            .await?
            .ok_or_else(|| HostError::AgentDefinitionNotFound {
                agent_name: agent_name.clone(),
            })?;
        let input = std::str::from_utf8(resource.contents())
            .map_err(|source| HostError::InvalidDefinitionEncoding { source })?;
        let definition = AgentDefinitionConfig::parse(input)?;
        let model = definition
            .model
            .clone()
            .unwrap_or_else(|| self.catalog().default);
        let updated_at = resource
            .modified()
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now);
        let view = AgentDefinitionView {
            agent_name: agent_name.as_str().to_owned(),
            model,
            model_parameters: definition.model_parameters,
            tools: definition.tools,
            prompt: definition.prompt,
            updated_at,
        };
        Ok((view, strong_etag(resource.contents())))
    }

    #[tracing::instrument(skip_all, fields(store.resource = "llm_catalog"))]
    async fn commit_catalog(&self, candidate: ManagedLlmCatalog) -> Result<(), HostError> {
        let mut candidate = candidate;
        candidate.revision = candidate
            .revision
            .checked_add(1)
            .ok_or(stratum_config::ConfigError::CatalogRevisionOverflow)?;
        candidate.validate()?;
        let manager = providers_from_catalog(&candidate)?;
        let encoded = candidate.encode()?.into_bytes();
        let expected = self
            .configuration_store
            .read_catalog()
            .await?
            .map(|current| current.revision());
        self.configuration_store
            .write_catalog(encoded, expected)
            .await
            .map_err(|error| match error {
                StoreError::ConfigurationAlreadyExists
                | StoreError::ConfigurationRevisionMismatch => HostError::RevisionConflict,
                other => other.into(),
            })?;
        self.replace_llm_state(super::host::RuntimeLlmState {
            catalog: candidate,
            providers: Arc::new(manager),
        });
        Ok(())
    }

    async fn model_blockers(
        &self,
        model: &ModelId,
        catalog: &ManagedLlmCatalog,
    ) -> Result<Vec<ResourceBlocker>, HostError> {
        let mut blockers = Vec::new();
        if &catalog.default == model {
            blockers.push(ResourceBlocker {
                resource_type: "default_model",
                name: model.to_string(),
            });
        }
        for definition in self.list_all_definition_models(catalog).await? {
            if &definition.1 == model {
                blockers.push(ResourceBlocker {
                    resource_type: "agent_definition",
                    name: definition.0,
                });
            }
        }
        Ok(blockers)
    }

    async fn provider_blockers(
        &self,
        kind: ProviderKind,
        catalog: &ManagedLlmCatalog,
    ) -> Result<Vec<ResourceBlocker>, HostError> {
        let mut blockers = Vec::new();
        if catalog.default.provider_name() == kind.as_str() {
            blockers.push(ResourceBlocker {
                resource_type: "default_model",
                name: catalog.default.to_string(),
            });
        }
        for (name, model) in self.list_all_definition_models(catalog).await? {
            if model.provider_name() == kind.as_str() {
                blockers.push(ResourceBlocker {
                    resource_type: "agent_definition",
                    name,
                });
            }
        }
        Ok(blockers)
    }

    async fn list_all_definition_models(
        &self,
        catalog: &ManagedLlmCatalog,
    ) -> Result<Vec<(String, ModelId)>, HostError> {
        let agent_names = self.configuration_store.list_agent_definitions().await?;
        let mut definitions = Vec::new();
        for agent_name in agent_names {
            let resource = self
                .configuration_store
                .read_agent_definition(&agent_name)
                .await?
                .ok_or_else(|| HostError::AgentDefinitionNotFound {
                    agent_name: agent_name.clone(),
                })?;
            let input = std::str::from_utf8(resource.contents())
                .map_err(|source| HostError::InvalidDefinitionEncoding { source })?;
            let resolved = catalog.resolve_definition(agent_name.clone(), input)?;
            definitions.push((agent_name.as_str().to_owned(), resolved.model));
        }
        Ok(definitions)
    }

    async fn catalog_updated_at(&self) -> DateTime<Utc> {
        self.configuration_store
            .read_catalog()
            .await
            .ok()
            .flatten()
            .and_then(|resource| resource.modified())
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(Utc::now)
    }
}

fn validate_page(page: usize, per_page: usize) -> Result<(usize, usize), HostError> {
    if page == 0 || per_page == 0 || per_page > MAX_PAGE_SIZE {
        return Err(HostError::InvalidRequest);
    }
    Ok((page, per_page))
}

fn page_slice<T>(data: Vec<T>, page: usize, per_page: usize) -> Vec<T> {
    let start = page.saturating_sub(1).saturating_mul(per_page);
    data.into_iter().skip(start).take(per_page).collect()
}

fn model_id(kind: ProviderKind, name: &str) -> Result<ModelId, HostError> {
    ModelId::new(kind.as_str(), name)
        .map_err(|_| field_error("name", "invalid", "model name is invalid"))
}

fn validate_model_name(kind: ProviderKind, name: &str) -> Result<(), HostError> {
    let _ = model_id(kind, name)?;
    if kind == ProviderKind::Deepseek && !matches!(name, "deepseek-v4-flash" | "deepseek-v4-pro") {
        return Err(field_error(
            "name",
            "unsupported",
            "model is not supported by the DeepSeek adapter",
        ));
    }
    Ok(())
}

fn require_matching_etag(provided: &str, current: &str) -> Result<(), HostError> {
    if provided == current {
        Ok(())
    } else {
        Err(HostError::RevisionConflict)
    }
}

fn provider_etag(revision: u64, kind: ProviderKind, provider: &ManagedProviderConfig) -> String {
    let mut canonical = Vec::new();
    canonical.extend_from_slice(&revision.to_be_bytes());
    canonical.extend_from_slice(kind.as_str().as_bytes());
    canonical.extend_from_slice(b"\0credential_configured=true");
    for model in &provider.models {
        canonical.push(0);
        canonical.extend_from_slice(model.as_bytes());
    }
    strong_etag(&canonical)
}

fn model_etag(revision: u64, kind: ProviderKind, name: &str) -> String {
    strong_etag(format!("{revision}:{}:{name}", kind.as_str()).as_bytes())
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

async fn execute_provider_probe(
    client: &reqwest::Client,
    endpoint: &str,
    api_key: &str,
) -> Result<(), HostError> {
    let url = format!("{}/models", endpoint.trim_end_matches('/'));
    let response = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await
        .map_err(|_| HostError::ProviderTestFailed)?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(HostError::ProviderTestFailed)
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use secrecy::SecretString;
    use stratum_config::{ManagedProviderConfig, ProviderKind};

    use super::{execute_provider_probe, provider_etag};
    use crate::HostError;

    async fn probe_server(status: u16, delay: Duration) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind probe server");
        let address = listener.local_addr().expect("probe address exists");
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept probe request");
            let mut request = vec![0_u8; 4096];
            let count = stream.read(&mut request).await.expect("read probe request");
            let request = String::from_utf8_lossy(&request[..count]);
            assert!(request.contains("GET /models HTTP/1.1"));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer test-secret")
            );
            tokio::time::sleep(delay).await;
            let response = format!(
                "HTTP/1.1 {status} probe\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            let _write_result = stream.write_all(response.as_bytes()).await;
        });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn provider_probe_accepts_success_response() {
        let endpoint = probe_server(200, Duration::ZERO).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .expect("build client");

        execute_provider_probe(&client, &endpoint, "test-secret")
            .await
            .expect("probe succeeds");
    }

    #[tokio::test]
    async fn provider_probe_sanitizes_authentication_failure() {
        let endpoint = probe_server(401, Duration::ZERO).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(1))
            .build()
            .expect("build client");

        let error = execute_provider_probe(&client, &endpoint, "test-secret")
            .await
            .expect_err("probe fails");

        assert!(matches!(error, HostError::ProviderTestFailed));
        assert!(!format!("{error:?}").contains("test-secret"));
        assert!(!error.to_string().contains("test-secret"));
    }

    #[tokio::test]
    async fn provider_probe_maps_timeout_to_sanitized_failure() {
        let endpoint = probe_server(200, Duration::from_millis(100)).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(10))
            .build()
            .expect("build client");

        let error = execute_provider_probe(&client, &endpoint, "test-secret")
            .await
            .expect_err("probe times out");

        assert!(matches!(error, HostError::ProviderTestFailed));
        assert!(!format!("{error:?}").contains("test-secret"));
    }

    #[test]
    fn provider_etag_never_hashes_or_infers_credential_content() {
        let first = ManagedProviderConfig::new(
            SecretString::new("first-secret".to_owned()),
            vec!["gpt-test".to_owned()],
        );
        let second = ManagedProviderConfig::new(
            SecretString::new("different-length-secret".to_owned()),
            vec!["gpt-test".to_owned()],
        );

        assert_eq!(
            provider_etag(7, ProviderKind::Openai, &first),
            provider_etag(7, ProviderKind::Openai, &second)
        );
        assert_ne!(
            provider_etag(7, ProviderKind::Openai, &first),
            provider_etag(8, ProviderKind::Openai, &first)
        );
    }
}
