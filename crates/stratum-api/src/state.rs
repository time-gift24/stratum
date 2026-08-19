//! Shared application state and the shutdown admission gate.

use std::collections::BTreeSet;
use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use stratum_config::Config;
use stratum_core::{AgentName, AgentVersionTag, ModelConfig, ModelId, ToolName};
use stratum_infra::NatsAgentRuntimeTail;
use stratum_llm::{LlmError, LlmProviderManager};
use stratum_ontology::OntologyStore;
use stratum_postgres::PostgresBackend;
use stratum_studio::{
    AgentDefinitionInput, ModelCatalogSnapshot, ProviderKind, RuntimeProvider, StudioError,
    StudioStore,
};
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::approval::ApprovalWaiters;
use crate::dispatcher::DispatcherHub;
use crate::error::{ApiError, ErrorKind};
use crate::host_error::HostError;
use crate::registry::TurnRegistry;
use crate::turn::build_tool_registry;
use crate::{ModelProbeError, ProviderFactory, probe_model_chat, providers_from_studio};

/// Process-owned background tasks (dispatchers and SSE tail pumps). Every
/// spawned task stays in this set until it is joined during normal operation
/// or shutdown; dropping the set aborts any unfinished task.
pub(crate) type RuntimeTasks = Arc<Mutex<JoinSet<()>>>;

/// Fully resolved definition used for a new AgentRuntime creation.
///
/// It comes from the mutable Studio authoring catalog. The caller persists the
/// result into the immutable execution ledger before a runtime exists.
pub(crate) struct RuntimeAgentDefinition {
    pub(crate) agent_version: AgentVersionTag,
    pub(crate) model: ModelConfig,
    pub(crate) tools: Vec<ToolName>,
    pub(crate) prompt: String,
}

/// Shared state of the assembled API host.
pub struct AppState {
    pg: PostgresBackend,
    tail: Option<NatsAgentRuntimeTail>,
    registry: TurnRegistry,
    dispatchers: DispatcherHub,
    provider_source: ProviderSource,
    provider_factory: ProviderFactory,
    studio: StudioStore,
    management_enabled: bool,
    ontology: OntologyStore,
    waiters: Arc<ApprovalWaiters>,
    allowed_origins: Vec<String>,
    shutdown: CancellationToken,
    admission: AdmissionGate,
    runtime_tasks: RuntimeTasks,
    sse_keep_alive: Duration,
    readiness_timeout: Duration,
}

/// Concrete source of Provider adapters for newly-started work.
///
/// Production hosts always use `Studio`; `Injected` is the existing mock
/// boundary used by integration tests. Even injected registries must exactly
/// match the Model identities persisted in Studio.
enum ProviderSource {
    Studio,
    #[cfg(test)]
    Injected(Arc<LlmProviderManager>),
}

impl AppState {
    /// Assembles shared state from already-connected dependencies.
    ///
    /// `providers` is an injected adapter registry whose model identities must
    /// correspond to records in `studio`. The default host assembly uses
    /// [`Self::with_studio`] to derive that registry directly from the database.
    ///
    /// # Errors
    ///
    /// Returns [`HostError`] when the required Studio configuration is absent
    /// or the persisted Agent definitions do not match the injected registry.
    #[cfg(test)]
    pub(crate) async fn new(
        pg: PostgresBackend,
        tail: Option<NatsAgentRuntimeTail>,
        providers: LlmProviderManager,
        ontology: OntologyStore,
        studio: StudioStore,
        config: Config,
    ) -> Result<Self, HostError> {
        validate_studio_models(&studio, &providers).await?;
        validate_studio_definitions(&studio, &providers).await?;
        Self::assemble(
            pg,
            tail,
            ontology,
            config,
            studio,
            ProviderSource::Injected(Arc::new(providers)),
            ProviderFactory::default(),
        )
    }

    fn assemble(
        pg: PostgresBackend,
        tail: Option<NatsAgentRuntimeTail>,
        ontology: OntologyStore,
        config: Config,
        studio: StudioStore,
        provider_source: ProviderSource,
        provider_factory: ProviderFactory,
    ) -> Result<Self, HostError> {
        config.validate()?;
        let management_enabled = config.require_studio()?.management_enabled;
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
            provider_source,
            provider_factory,
            studio,
            management_enabled,
            ontology,
            waiters: Arc::new(ApprovalWaiters::default()),
            allowed_origins,
            shutdown,
            admission: AdmissionGate::default(),
            runtime_tasks,
            sse_keep_alive: Duration::from_secs(api.sse_keep_alive_seconds),
            readiness_timeout: Duration::from_millis(api.readiness_timeout_ms),
        })
    }

    /// Assembles state with Provider adapters derived exclusively from Studio.
    ///
    /// An empty Studio Provider/model catalog is valid and produces an empty
    /// runtime registry. No configuration or filesystem source is imported.
    ///
    /// # Errors
    ///
    /// Returns [`HostError`] when Studio cannot be read, a persisted model is
    /// unsupported, or the required Studio configuration is absent.
    pub async fn with_studio(
        pg: PostgresBackend,
        tail: Option<NatsAgentRuntimeTail>,
        ontology: OntologyStore,
        config: Config,
        studio: StudioStore,
    ) -> Result<Self, HostError> {
        let factory = ProviderFactory::default();
        let providers = providers_from_studio(&studio, &factory).await?;
        validate_studio_models(&studio, &providers).await?;
        validate_studio_definitions(&studio, &providers).await?;
        Self::assemble(
            pg,
            tail,
            ontology,
            config,
            studio,
            ProviderSource::Studio,
            factory,
        )
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

    /// Builds the Provider snapshot used by newly-started work.
    ///
    /// Production reads Studio on every snapshot boundary, so a committed
    /// management write cannot race with or leave behind a stale in-memory
    /// catalog. A Turn keeps the returned provider `Arc` for its own lifetime.
    pub(crate) async fn providers(&self) -> Result<Arc<LlmProviderManager>, HostError> {
        match &self.provider_source {
            ProviderSource::Studio => Ok(Arc::new(
                providers_from_studio(&self.studio, &self.provider_factory).await?,
            )),
            #[cfg(test)]
            ProviderSource::Injected(providers) => Ok(Arc::clone(providers)),
        }
    }

    /// Loads management Model rows and adapter inputs from one catalog-locked
    /// Studio snapshot.
    pub(crate) async fn studio_model_catalog(&self) -> Result<ModelCatalogSnapshot, HostError> {
        Ok(self.studio.model_catalog_snapshot().await?)
    }

    /// Builds only the adapters needed to render the requested management
    /// Model page or detail response.
    pub(crate) fn providers_for_studio_models(
        &self,
        mut runtime_providers: Vec<RuntimeProvider>,
        models: &[ModelId],
    ) -> Result<Arc<LlmProviderManager>, HostError> {
        for provider in &mut runtime_providers {
            provider.models.retain(|name| {
                models.iter().any(|model| {
                    model.provider_name() == provider.kind.as_str() && model.model_name() == name
                })
            });
        }
        runtime_providers.retain(|provider| !provider.models.is_empty());
        Ok(Arc::new(self.provider_factory.build(runtime_providers)?))
    }

    /// Canonical Studio catalog used by runtime and management reads.
    pub(crate) const fn studio(&self) -> &StudioStore {
        &self.studio
    }

    /// Whether loopback-only Studio mutation routes are exposed.
    pub(crate) const fn management_enabled(&self) -> bool {
        self.management_enabled
    }

    /// Resolves the current authoring definition for a future AgentRuntime.
    pub(crate) async fn resolve_agent_definition(
        &self,
        agent_name: &AgentName,
    ) -> Result<RuntimeAgentDefinition, ApiError> {
        let definition = self
            .studio
            .agent_definition(agent_name)
            .await
            .map_err(map_studio_error)?
            .value;
        Ok(RuntimeAgentDefinition {
            agent_version: definition.agent_version,
            model: definition.model,
            tools: definition.tools,
            prompt: definition.prompt,
        })
    }

    /// Lists the current definition catalog used for new AgentRuntimes.
    pub(crate) async fn list_agent_templates(
        &self,
    ) -> Result<Vec<crate::dto::AgentTemplateDto>, ApiError> {
        self.studio
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
            })
    }

    /// Checks whether a new Studio model can be assembled by this binary.
    pub(crate) fn validate_studio_model(
        &self,
        kind: ProviderKind,
        name: &str,
    ) -> Result<(), HostError> {
        self.provider_factory.validate_model(kind, name)
    }

    /// Resolves the parameter schema for a prospective Studio model before its
    /// database mutation starts.
    ///
    /// The trusted adapter is built from the current Studio credential
    /// snapshot. Callers can therefore commit the Model and construct its HTTP
    /// response without a second catalog read after the durable write.
    ///
    /// # Errors
    ///
    /// Returns [`HostError`] when the Provider catalog cannot be read, the
    /// requested Provider is absent, or its adapter cannot be assembled.
    pub(crate) async fn prospective_studio_model_schema(
        &self,
        kind: ProviderKind,
        name: &str,
    ) -> Result<serde_json::Value, HostError> {
        let mut provider = self
            .studio
            .runtime_providers()
            .await?
            .into_iter()
            .find(|provider| provider.kind == kind)
            .ok_or(StudioError::NotFound)?;
        provider.models.clear();
        provider.models.push(name.to_owned());
        let providers = self.provider_factory.build(vec![provider])?;
        let model =
            ModelId::new(kind.as_str(), name).map_err(|source| HostError::InvalidManagedModel {
                provider: kind.as_str(),
                model: name.to_owned(),
                source,
            })?;
        Ok(providers.get(&model)?.parameter_schema())
    }

    /// Sends one real minimal message through the requested Studio Model's
    /// adapter using the current credential snapshot, and returns the
    /// round-trip latency in milliseconds. The model must be configured under
    /// this Provider in the Studio catalog; no health state is persisted.
    pub(crate) async fn test_studio_model(
        &self,
        kind: ProviderKind,
        name: &str,
    ) -> Result<u64, ModelProbeError> {
        let provider = self
            .studio
            .runtime_providers()
            .await?
            .into_iter()
            .find(|provider| provider.kind == kind)
            .ok_or(stratum_studio::StudioError::NotFound)?;
        if !provider.models.iter().any(|model| model == name) {
            return Err(ModelProbeError::ModelNotConfigured);
        }
        let adapter = self.provider_factory.build_model_adapter(provider, name)?;
        probe_model_chat(adapter.as_ref()).await
    }

    /// Validates one prospective Agent definition against the DB-derived
    /// Provider registry before Studio commits it.
    pub(crate) async fn validate_studio_definition(
        &self,
        definition: &AgentDefinitionInput,
    ) -> Result<(), ApiError> {
        let providers = self
            .providers()
            .await
            .map_err(|error| ApiError::with_source(ErrorKind::RuntimeUnavailable, error))?;
        validate_model_config(&providers, &definition.model)
            .map_err(studio_model_validation_error)?;
        build_tool_registry(&definition.tools).map_err(|error| {
            ApiError::with_field_violation(ErrorKind::InvalidStudioResource, "tools", error)
        })?;
        Ok(())
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

async fn validate_studio_definitions(
    studio: &StudioStore,
    providers: &LlmProviderManager,
) -> Result<(), HostError> {
    for definition in studio.list_agent_definitions().await? {
        if build_tool_registry(&definition.value.tools).is_err() {
            return Err(StudioError::CatalogCorrupt {
                field: "agent_definition.tools",
            }
            .into());
        }
        validate_model_config(providers, &definition.value.model)?;
    }
    Ok(())
}

async fn validate_studio_models(
    studio: &StudioStore,
    providers: &LlmProviderManager,
) -> Result<(), HostError> {
    let expected = studio
        .list_models()
        .await?
        .into_iter()
        .map(|model| model.value.model)
        .collect::<BTreeSet<_>>();
    let actual = providers
        .models()
        .into_iter()
        .map(|model| model.model)
        .collect::<BTreeSet<_>>();
    if actual == expected {
        Ok(())
    } else {
        Err(HostError::ProviderCatalogMismatch)
    }
}

fn validate_model_config(
    providers: &LlmProviderManager,
    model: &ModelConfig,
) -> Result<(), LlmError> {
    providers.configure(model).map(|_| ())
}

fn studio_model_validation_error(error: LlmError) -> ApiError {
    let field = studio_model_validation_field(&error);
    ApiError::with_field_violation(ErrorKind::InvalidStudioResource, field, error)
}

fn studio_model_validation_field(error: &LlmError) -> &'static str {
    match error {
        LlmError::ProviderNotFound { .. } => "model",
        LlmError::InvalidModelParameters { .. } => "model_parameters",
        _ => "model",
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

    #[test]
    fn studio_model_validation_targets_the_rejected_field() {
        let model = stratum_core::ModelId::new("deepseek", "deepseek-chat")
            .expect("test model id is valid");

        let missing = LlmError::ProviderNotFound {
            model: model.clone(),
        };
        let invalid = LlmError::InvalidModelParameters { model };

        assert_eq!(studio_model_validation_field(&missing), "model");
        assert_eq!(studio_model_validation_field(&invalid), "model_parameters");
    }

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
