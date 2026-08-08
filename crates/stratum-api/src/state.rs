//! Shared application state and the shutdown admission gate.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use stratum_config::Config;
use stratum_infra::NatsAgentTail;
use stratum_llm::LlmProviderManager;
use stratum_postgres::PostgresBackend;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

use crate::approval::ApprovalWaiters;
use crate::dispatcher::DispatcherHub;
use crate::error::{ApiError, ErrorKind};
use crate::host_error::HostError;
use crate::registry::TurnRegistry;
use crate::templates::TemplateCatalog;

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
        let templates = TemplateCatalog::new(&config.agent.templates_root, config.clone()).await?;
        let allowed_origins = config
            .api
            .as_ref()
            .map_or_else(Vec::new, |api| api.allowed_origins.clone());
        Ok(Self {
            dispatchers: DispatcherHub::new(pg.clone(), tail.clone(), shutdown.clone()),
            pg,
            tail,
            registry: TurnRegistry::default(),
            providers,
            waiters: Arc::new(ApprovalWaiters::default()),
            templates,
            allowed_origins,
            shutdown,
            admission: AdmissionGate::default(),
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

    /// Begins shutdown: closes admission, ends SSE streams, and unblocks
    /// admission waits. Turn tokens are never signalled and no terminal event
    /// is written on behalf of the process.
    pub(crate) fn initiate_shutdown(&self) {
        self.admission.close();
        self.shutdown.cancel();
    }

    /// Bounded wait for managed turn tasks after shutdown starts; tasks that
    /// outlive the bound stay durable `running` for an explicit resume.
    pub(crate) async fn drain_managed_tasks(&self, bound: Duration) {
        let tasks = self.registry.take_tasks();
        if tasks.is_empty() {
            return;
        }
        let drained = async {
            for task in tasks {
                if task.await.is_err() {
                    tracing::warn!("a managed turn task panicked during shutdown");
                }
            }
        };
        if tokio::time::timeout(bound, drained).await.is_err() {
            tracing::warn!(
                "managed turn drain timed out; unfinished turns stay durable running for resume"
            );
        }
    }
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
        while self.in_flight.load(Ordering::Acquire) > 0 {
            self.drained.notified().await;
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
}
