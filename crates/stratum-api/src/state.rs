//! Shared application state and the shutdown admission gate.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use stratum_config::Config;
use stratum_core::{AgentName, AgentVersionTag, ModelConfig, ToolName};
use stratum_infra::NatsAgentRuntimeTail;
use stratum_llm::LlmProviderManager;
use stratum_ontology::OntologyStore;
use stratum_postgres::PostgresBackend;
use stratum_studio::{
    AgentDefinitionInput, ProviderKind, ProviderSeed, StudioCatalogSeed, StudioError, StudioStore,
};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::approval::ApprovalWaiters;
use crate::dispatcher::DispatcherHub;
use crate::error::{ApiError, ErrorKind};
use crate::host_error::HostError;
use crate::provider_catalog::ProviderCatalog;
use crate::registry::TurnRegistry;
use crate::templates::TemplateCatalog;
use crate::{ProviderFactory, providers_from_studio};

/// Process-owned background tasks (dispatchers and SSE tail pumps). Every
/// spawned task stays in this set until it is joined during normal operation
/// or shutdown; dropping the set aborts any unfinished task.
pub(crate) type RuntimeTasks = Arc<Mutex<JoinSet<()>>>;

/// Fully resolved definition used for a new AgentRuntime creation.
///
/// In Studio mode it comes from the mutable authoring catalog; otherwise it
/// is resolved from the read-only template directory. Either way, the caller
/// persists the result into the immutable execution ledger before a runtime
/// exists.
pub(crate) struct RuntimeAgentDefinition {
    pub(crate) agent_version: AgentVersionTag,
    pub(crate) model: ModelConfig,
    pub(crate) tools: Vec<ToolName>,
    pub(crate) prompt: String,
}

/// Dependencies that jointly form the hot catalog boundary.
struct CatalogDependencies {
    providers: LlmProviderManager,
    templates: TemplateCatalog,
    studio: Option<StudioStore>,
    provider_factory: Option<ProviderFactory>,
}

/// Shared state of the assembled API host.
pub struct AppState {
    pg: PostgresBackend,
    tail: Option<NatsAgentRuntimeTail>,
    registry: TurnRegistry,
    dispatchers: DispatcherHub,
    providers: ProviderCatalog,
    provider_factory: Option<ProviderFactory>,
    studio: Option<StudioStore>,
    ontology: OntologyStore,
    waiters: Arc<ApprovalWaiters>,
    templates: TemplateCatalog,
    allowed_origins: Vec<String>,
    shutdown: CancellationToken,
    admission: AdmissionGate,
    runtime_tasks: RuntimeTasks,
    sse_keep_alive: Duration,
    readiness_timeout: Duration,
}

impl AppState {
    /// Assembles the shared state from already-connected dependencies.
    ///
    /// # Errors
    ///
    /// Returns [`HostError::TemplatesRoot`] when the configured template
    /// catalog root is missing, not a directory, or unreadable.
    pub async fn new(
        pg: PostgresBackend,
        tail: Option<NatsAgentRuntimeTail>,
        providers: LlmProviderManager,
        ontology: OntologyStore,
        config: Config,
    ) -> Result<Self, HostError> {
        let templates = TemplateCatalog::new(&config.agent.templates_root, config.clone()).await?;
        Self::from_parts(
            pg,
            tail,
            ontology,
            config,
            CatalogDependencies {
                providers,
                templates,
                studio: None,
                provider_factory: None,
            },
        )
    }

    /// Assembles a state whose mutable catalog is owned by Studio.
    ///
    /// The read-only boot configuration and template directory are read once
    /// only when Studio is empty. All later runtime definition/provider reads
    /// resolve through the Studio database.
    ///
    /// # Errors
    ///
    /// Returns [`HostError`] when the catalog cannot be bootstrapped or its
    /// runtime provider registry cannot be assembled.
    pub async fn with_studio(
        pg: PostgresBackend,
        tail: Option<NatsAgentRuntimeTail>,
        ontology: OntologyStore,
        config: Config,
        studio: StudioStore,
    ) -> Result<Self, HostError> {
        let templates = TemplateCatalog::new(&config.agent.templates_root, config.clone()).await?;
        let definitions = templates
            .studio_seed_definitions()
            .await
            .map_err(HostError::StudioTemplateSeed)?;
        studio
            .seed_if_empty(studio_seed(&config, definitions))
            .await?;
        let factory = ProviderFactory::from_config(&config);
        let providers = providers_from_studio(&studio, &factory).await?;
        Self::from_parts(
            pg,
            tail,
            ontology,
            config,
            CatalogDependencies {
                providers,
                templates,
                studio: Some(studio),
                provider_factory: Some(factory),
            },
        )
    }

    fn from_parts(
        pg: PostgresBackend,
        tail: Option<NatsAgentRuntimeTail>,
        ontology: OntologyStore,
        config: Config,
        catalog: CatalogDependencies,
    ) -> Result<Self, HostError> {
        let shutdown = CancellationToken::new();
        let runtime_tasks = Arc::new(Mutex::new(JoinSet::new()));
        let api = config.api.clone().unwrap_or_default();
        let allowed_origins = config
            .api
            .as_ref()
            .map_or_else(Vec::new, |api| api.allowed_origins.clone());
        Ok(Self {
            dispatchers: DispatcherHub::new(
                pg.clone(),
                tail.clone(),
                shutdown.clone(),
                Arc::clone(&runtime_tasks),
                Duration::from_secs(api.dispatcher_idle_timeout_seconds),
            ),
            pg,
            tail,
            registry: TurnRegistry::default(),
            providers: ProviderCatalog::new(catalog.providers),
            provider_factory: catalog.provider_factory,
            studio: catalog.studio,
            ontology,
            waiters: Arc::new(ApprovalWaiters::default()),
            templates: catalog.templates,
            allowed_origins,
            shutdown,
            admission: AdmissionGate::default(),
            runtime_tasks,
            sse_keep_alive: Duration::from_secs(api.sse_keep_alive_seconds),
            readiness_timeout: Duration::from_millis(api.readiness_timeout_ms),
        })
    }

    /// Postgres execution store.
    pub(crate) fn pg(&self) -> &PostgresBackend {
        &self.pg
    }

    /// NATS tail, when realtime is connected.
    pub(crate) fn tail(&self) -> Option<&NatsAgentRuntimeTail> {
        self.tail.as_ref()
    }

    /// Process-local hosting registry.
    pub(crate) fn registry(&self) -> &TurnRegistry {
        &self.registry
    }

    /// Per-AgentRuntime realtime dispatcher hub.
    pub(crate) fn dispatchers(&self) -> &DispatcherHub {
        &self.dispatchers
    }

    /// Registered LLM providers.
    pub(crate) fn providers(&self) -> Arc<LlmProviderManager> {
        self.providers.snapshot()
    }

    /// Returns the enabled Studio catalog, if this host was configured for it.
    pub(crate) fn studio(&self) -> Option<&StudioStore> {
        self.studio.as_ref()
    }

    /// Resolves the current authoring definition for a future AgentRuntime.
    pub(crate) async fn resolve_agent_definition(
        &self,
        agent_name: &AgentName,
    ) -> Result<RuntimeAgentDefinition, ApiError> {
        if let Some(studio) = &self.studio {
            let definition = studio
                .agent_definition(agent_name)
                .await
                .map_err(map_studio_error)?
                .value;
            return Ok(RuntimeAgentDefinition {
                agent_version: definition.agent_version,
                model: definition.model,
                tools: definition.tools,
                prompt: definition.prompt,
            });
        }

        let definition = self.templates.resolve(agent_name).await?;
        let providers = self.providers();
        let model = providers
            .default_model_config(&definition.model)
            .map_err(|_| ApiError::new(ErrorKind::ModelNotConfigured))?;
        Ok(RuntimeAgentDefinition {
            agent_version: definition.agent_version,
            model,
            tools: definition.tools,
            prompt: definition.prompt,
        })
    }

    /// Lists the current definition catalog used for new AgentRuntimes.
    pub(crate) async fn list_agent_templates(
        &self,
    ) -> Result<Vec<crate::dto::AgentTemplateDto>, ApiError> {
        if let Some(studio) = &self.studio {
            return studio
                .list_agent_definitions()
                .await
                .map_err(map_studio_error)
                .map(|definitions| {
                    definitions
                        .into_iter()
                        .map(|definition| crate::dto::AgentTemplateDto {
                            agent_name: definition.value.agent_name.to_string(),
                            version: definition.value.agent_version,
                            model_config: definition.value.model,
                        })
                        .collect()
                });
        }
        let providers = self.providers();
        self.templates.list(&providers).await
    }

    /// Rebuilds the complete registry after a Studio Provider/model change.
    /// Existing Turns retain their previously-cloned provider [`Arc`].
    pub(crate) async fn refresh_studio_providers(&self) -> Result<(), HostError> {
        let studio = self.studio.as_ref().ok_or(StudioError::NotInitialized)?;
        let factory = self
            .provider_factory
            .as_ref()
            .ok_or(StudioError::NotInitialized)?;
        let providers = providers_from_studio(studio, factory).await?;
        self.providers.replace(providers);
        Ok(())
    }

    /// Checks whether a new Studio model can be assembled by this binary.
    pub(crate) fn validate_studio_model(
        &self,
        kind: ProviderKind,
        name: &str,
    ) -> Result<(), HostError> {
        self.provider_factory
            .as_ref()
            .ok_or(StudioError::NotInitialized)?
            .validate_model(kind, name)
    }

    /// Canonical Ontology metadata store.
    pub(crate) fn ontology(&self) -> &OntologyStore {
        &self.ontology
    }

    /// Process-local approval waiters.
    pub(crate) fn waiters(&self) -> &Arc<ApprovalWaiters> {
        &self.waiters
    }

    /// Browser origins allowed to call the API.
    pub(crate) fn allowed_origins(&self) -> &[String] {
        &self.allowed_origins
    }

    /// Process shutdown token.
    pub(crate) fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    /// Durable-work admission gate.
    pub(crate) fn admission(&self) -> &AdmissionGate {
        &self.admission
    }

    /// Configured SSE keep-alive interval.
    pub(crate) const fn sse_keep_alive(&self) -> Duration {
        self.sse_keep_alive
    }

    /// Maximum time for one complete readiness probe.
    pub(crate) const fn readiness_timeout(&self) -> Duration {
        self.readiness_timeout
    }

    /// Spawns one process-owned background task and opportunistically joins
    /// tasks that have already completed.
    pub(crate) fn spawn_runtime_task(&self, task: impl Future<Output = ()> + Send + 'static) {
        spawn_runtime_task(&self.runtime_tasks, task);
    }

    /// Begins shutdown: closes admission, ends SSE streams, and unblocks
    /// admission waits. Turn tokens are never signalled and no terminal event
    /// is written on behalf of the process.
    pub(crate) fn initiate_shutdown(&self) {
        self.admission.close();
        self.shutdown.cancel();
    }

    /// Boundedly joins managed Turns, dispatchers, and SSE pumps. A timeout
    /// aborts and joins the remainder; unfinished Turns stay durable `running`
    /// for explicit resume and no task is detached from process ownership.
    pub(crate) async fn drain_runtime_tasks(&self, bound: Duration) {
        let mut tasks = {
            let mut owned = self
                .runtime_tasks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *owned)
        };
        if tasks.is_empty() {
            return;
        }
        let drained = async {
            while let Some(result) = tasks.join_next().await {
                if let Err(error) = result
                    && !error.is_cancelled()
                {
                    tracing::warn!(
                        task.panicked = error.is_panic(),
                        "a runtime task failed during shutdown"
                    );
                }
            }
        };
        if tokio::time::timeout(bound, drained).await.is_err() {
            tracing::warn!("runtime task drain timed out; aborting remaining tasks");
            tasks.abort_all();
            while tasks.join_next().await.is_some() {}
        }
    }
}

fn studio_seed(config: &Config, agent_definitions: Vec<AgentDefinitionInput>) -> StudioCatalogSeed {
    let mut providers = Vec::with_capacity(2);
    if let Some(openai) = &config.llm.openai {
        providers.push(ProviderSeed {
            kind: ProviderKind::Openai,
            api_key: openai.api_key.clone(),
            models: openai.models.clone(),
        });
    }
    if let Some(deepseek) = &config.llm.deepseek {
        providers.push(ProviderSeed {
            kind: ProviderKind::Deepseek,
            api_key: deepseek.api_key.clone(),
            models: deepseek.models.clone(),
        });
    }
    StudioCatalogSeed {
        providers,
        agent_definitions,
    }
}

fn map_studio_error(error: StudioError) -> ApiError {
    let kind = match error {
        StudioError::NotFound => ErrorKind::AgentTemplateNotFound,
        StudioError::ModelNotConfigured => ErrorKind::ModelNotConfigured,
        StudioError::Database(_) | StudioError::Migration(_) | StudioError::NotInitialized => {
            ErrorKind::RuntimeUnavailable
        }
        StudioError::CatalogCorrupt { .. } => ErrorKind::Internal,
        StudioError::AlreadyExists
        | StudioError::PreconditionFailed
        | StudioError::AgentVersionUnchanged
        | StudioError::DeletionBlocked { .. }
        | StudioError::InvalidInput { .. } => ErrorKind::InvalidAgentTemplate,
        _ => ErrorKind::Internal,
    };
    ApiError::with_source(kind, error)
}

/// Adds a task to the shared process task set. This is a function rather than
/// a manager abstraction because both the state-owned SSE path and the
/// dispatcher hub need the same concrete `JoinSet` ownership boundary.
pub(crate) fn spawn_runtime_task(
    tasks: &RuntimeTasks,
    task: impl Future<Output = ()> + Send + 'static,
) {
    let mut tasks = tasks
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    while let Some(result) = tasks.try_join_next() {
        if let Err(error) = result
            && !error.is_cancelled()
        {
            tracing::warn!(task.panicked = error.is_panic(), "a runtime task failed");
        }
    }
    tasks.spawn(task);
}

/// Atomic admission gate: create/message/resume enter before any durable or
/// provider work; closing the gate rejects new durable work with a stable
/// 503 and lets the shutdown path wait for in-flight admissions.
#[derive(Debug)]
pub(crate) struct AdmissionGate {
    open: AtomicBool,
    in_flight: AtomicUsize,
    drained: Notify,
}

impl Default for AdmissionGate {
    fn default() -> Self {
        Self {
            open: AtomicBool::new(true),
            in_flight: AtomicUsize::new(0),
            drained: Notify::new(),
        }
    }
}

impl AdmissionGate {
    /// Enters the gate.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::ServiceShuttingDown`] once shutdown has begun.
    pub(crate) fn enter(&self) -> Result<AdmissionGuard<'_>, ApiError> {
        if !self.open.load(Ordering::Acquire) {
            return Err(ApiError::new(ErrorKind::ServiceShuttingDown));
        }
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        // Re-check after registering: a close between the first check and
        // the increment must not slip a new admission through.
        if !self.open.load(Ordering::Acquire) {
            self.leave();
            return Err(ApiError::new(ErrorKind::ServiceShuttingDown));
        }
        Ok(AdmissionGuard { gate: self })
    }

    /// Closes the gate; idempotent.
    pub(crate) fn close(&self) {
        self.open.store(false, Ordering::Release);
    }

    /// Waits until every entered admission has left.
    pub(crate) async fn wait_drained(&self) {
        // `Notify::notify_waiters` stores no permit. Register and enable the
        // waiter before reading the counter so the final guard drop cannot
        // land between the load and waiter registration.
        let notified = self.drained.notified();
        tokio::pin!(notified);
        loop {
            notified.as_mut().enable();
            if self.in_flight.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.as_mut().await;
            notified.set(self.drained.notified());
        }
    }

    fn leave(&self) {
        if self.in_flight.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.drained.notify_waiters();
        }
    }
}

/// RAII half of [`AdmissionGate::enter`].
pub(crate) struct AdmissionGuard<'a> {
    gate: &'a AdmissionGate,
}

impl Drop for AdmissionGuard<'_> {
    fn drop(&mut self) {
        self.gate.leave();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn closed_gate_rejects_new_admissions_and_drains() {
        let gate = Arc::new(AdmissionGate::default());
        let Ok(guard) = gate.enter() else {
            panic!("gate is open")
        };

        gate.close();
        let rejected = gate.enter();
        assert!(matches!(
            rejected.map(|_| ()),
            Err(error) if error.kind() == ErrorKind::ServiceShuttingDown
        ));

        let waiting = Arc::clone(&gate);
        let drained = tokio::spawn(async move {
            waiting.wait_drained().await;
        });
        tokio::task::yield_now().await;
        drop(guard);
        drained.await.expect("drain completes");
    }

    #[tokio::test]
    async fn empty_gate_reports_drained_without_waiting_for_a_notification() {
        let gate = AdmissionGate::default();

        tokio::time::timeout(Duration::from_millis(50), gate.wait_drained())
            .await
            .expect("an empty gate drains immediately");
    }

    #[tokio::test]
    async fn final_guard_drop_cannot_be_lost_while_a_drain_waiter_registers() {
        for _ in 0..256 {
            let gate = Arc::new(AdmissionGate::default());
            let guard = gate.enter().expect("gate is open");
            let waiting = Arc::clone(&gate);
            let waiter = tokio::spawn(async move {
                waiting.wait_drained().await;
            });

            tokio::task::yield_now().await;
            drop(guard);

            tokio::time::timeout(Duration::from_secs(1), waiter)
                .await
                .expect("drain waiter observes the final guard drop")
                .expect("drain waiter joins");
        }
    }
}
