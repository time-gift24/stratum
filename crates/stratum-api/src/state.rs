//! Shared application state and the shutdown admission gate.

use std::future::Future;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use stratum_config::Config;
use stratum_infra::NatsAgentTail;
use stratum_llm::LlmProviderManager;
use stratum_postgres::PostgresBackend;
use tokio::sync::Notify;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::approval::ApprovalWaiters;
use crate::dispatcher::DispatcherHub;
use crate::error::{ApiError, ErrorKind};
use crate::host_error::HostError;
use crate::registry::TurnRegistry;
use crate::templates::TemplateCatalog;

/// Process-owned background tasks (dispatchers and SSE tail pumps). Every
/// spawned task stays in this set until it is joined during normal operation
/// or shutdown; dropping the set aborts any unfinished task.
pub(crate) type RuntimeTasks = Arc<Mutex<JoinSet<()>>>;

/// Shared state of the assembled API host.
pub struct AppState {
    pg: PostgresBackend,
    tail: Option<NatsAgentTail>,
    registry: TurnRegistry,
    dispatchers: DispatcherHub,
    providers: LlmProviderManager,
    waiters: Arc<ApprovalWaiters>,
    templates: TemplateCatalog,
    allowed_origins: Vec<String>,
    shutdown: CancellationToken,
    admission: AdmissionGate,
    runtime_tasks: RuntimeTasks,
    sse_keep_alive: Duration,
    approval_poll_interval: Duration,
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
        tail: Option<NatsAgentTail>,
        providers: LlmProviderManager,
        config: Config,
    ) -> Result<Self, HostError> {
        let shutdown = CancellationToken::new();
        let runtime_tasks = Arc::new(Mutex::new(JoinSet::new()));
        let api = config.api.clone().unwrap_or_default();
        let templates = TemplateCatalog::new(&config.agent.templates_root, config.clone()).await?;
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
            providers,
            waiters: Arc::new(ApprovalWaiters::default()),
            templates,
            allowed_origins,
            shutdown,
            admission: AdmissionGate::default(),
            runtime_tasks,
            sse_keep_alive: Duration::from_secs(api.sse_keep_alive_seconds),
            approval_poll_interval: Duration::from_secs(api.approval_poll_interval_seconds),
        })
    }

    /// Postgres execution store.
    pub(crate) fn pg(&self) -> &PostgresBackend {
        &self.pg
    }

    /// NATS tail, when realtime is connected.
    pub(crate) fn tail(&self) -> Option<&NatsAgentTail> {
        self.tail.as_ref()
    }

    /// Process-local hosting registry.
    pub(crate) fn registry(&self) -> &TurnRegistry {
        &self.registry
    }

    /// Per-agent realtime dispatcher hub.
    pub(crate) fn dispatchers(&self) -> &DispatcherHub {
        &self.dispatchers
    }

    /// Registered LLM providers.
    pub(crate) fn providers(&self) -> &LlmProviderManager {
        &self.providers
    }

    /// Process-local approval waiters.
    pub(crate) fn waiters(&self) -> &Arc<ApprovalWaiters> {
        &self.waiters
    }

    /// Read-only template catalog.
    pub(crate) fn templates(&self) -> &TemplateCatalog {
        &self.templates
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

    /// Configured durable approval fallback polling interval.
    pub(crate) const fn approval_poll_interval(&self) -> Duration {
        self.approval_poll_interval
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
    /// Returns [`ErrorKind::ServiceUnavailable`] once shutdown has begun.
    pub(crate) fn enter(&self) -> Result<AdmissionGuard<'_>, ApiError> {
        if !self.open.load(Ordering::Acquire) {
            return Err(ApiError::new(ErrorKind::ServiceUnavailable));
        }
        self.in_flight.fetch_add(1, Ordering::AcqRel);
        // Re-check after registering: a close between the first check and
        // the increment must not slip a new admission through.
        if !self.open.load(Ordering::Acquire) {
            self.leave();
            return Err(ApiError::new(ErrorKind::ServiceUnavailable));
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
            Err(error) if error.kind() == ErrorKind::ServiceUnavailable
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
