//! HTTP assembly and orchestration layer of the Postgres-first agent runtime.
//!
//! `stratum-api` is the only assembly crate: it owns the process registry,
//! the Postgres-backed sink adapters injected into the kernel, the approval
//! handler and resolver, the per-AgentRuntime realtime dispatcher, and the HTTP/SSE
//! surface. Postgres is the only durable truth and the core readiness
//! dependency; NATS is a short, lossy observation channel whose failure only
//! degrades realtime. The kernel (`stratum-agent`) stays storage-agnostic and
//! never sees Session, Turn, or sequence concepts beyond its typed events.
//!
//! `main.rs` stays thin; everything reusable lives here.

mod approval;
mod baseline;
mod dispatcher;
mod dto;
mod error;
mod frames;
mod host_error;
mod http;
mod management_dto;
mod provenance;
mod registry;
mod scheduler;
mod sink;
mod state;
mod studio_management;
mod telemetry;
mod turn;

#[cfg(test)]
extern crate self as stratum_api;

#[cfg(test)]
#[path = "../tests/api.rs"]
mod integration_api;
#[cfg(test)]
#[path = "../tests/common/mod.rs"]
mod integration_common;
#[cfg(test)]
#[path = "../tests/ontology_api.rs"]
mod integration_ontology_api;
#[cfg(test)]
#[path = "../tests/studio_db_only.rs"]
mod integration_studio_db_only;

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use stratum_config::Config;
use stratum_core::ModelId;
use stratum_infra::{AgentRuntimeTailConfig, NatsAgentRuntimeTail};
use stratum_llm::{
    ApiKey, ChatMessage, ChatRequest, DeepSeekModel, DeepSeekProvider, DeepSeekThinking, LlmError,
    LlmProvider, LlmProviderManager, LlmTimeouts, OpenAICompatibleProvider,
};
use stratum_ontology::OntologyStore;
use stratum_postgres::PostgresBackend;
use stratum_studio::{ProviderKind, RuntimeProvider, StudioStore};

pub use dto::{
    AgentRuntimeCreated, AgentRuntimeStatusDto, AgentRuntimeView, AgentTemplateDto,
    AgentTemplatesResponse, CreateAgentRuntimeRequest, CreateScheduleRequest, HistoryItemDto,
    HistoryResponse, LivenessResponse, ModelsResponse, Pagination, PendingApprovalDto,
    ReadinessResponse, ScheduleSessionStatus, ScheduleSessionView, ScheduleSessionsPage,
    ScheduleView, SchedulesPage, TurnAccepted,
};
pub use error::{ApiError, ErrorKind, ErrorResponse};
pub use frames::{AgentRuntimeProductEventV1, AgentRuntimeStreamFrameV1};
pub use host_error::HostError;
pub use http::router;
pub use state::AppState;
pub use telemetry::{TelemetryGuard, init_telemetry};

/// Well-known public OpenAI API endpoint owned by the built-in adapter.
const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
/// Well-known public DeepSeek API endpoint owned by the built-in adapter.
const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
/// Fixed operational policy shared by the built-in provider adapters.
const PROVIDER_TIMEOUTS: LlmTimeouts = LlmTimeouts::new(
    Duration::from_secs(10),
    Duration::from_secs(120),
    Duration::from_secs(30),
    Duration::from_secs(60),
);
/// Hard bound for one transient Provider Model message test.
const PROVIDER_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Safe failure of one real-message Model test.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ModelProbeError {
    /// Studio could not provide the requested credential snapshot.
    #[error("studio provider snapshot is unavailable")]
    Studio(#[from] stratum_studio::StudioError),
    /// The requested model is not configured under this Provider.
    #[error("model is not configured for the provider")]
    ModelNotConfigured,
    /// The trusted adapter for this model cannot be assembled.
    #[error("model adapter cannot be assembled")]
    Adapter(#[from] HostError),
    /// The provider rejected the credential (401/403).
    #[error("provider rejected the model test credential")]
    Credentials,
    /// The provider does not serve this model (upstream 404).
    #[error("model is not available at the provider")]
    ModelNotAvailable,
    /// The message test timed out, failed in transport, or was rejected.
    #[error("provider model test failed")]
    Failed,
}

/// Builds trusted adapters for Provider records read from Studio PostgreSQL.
///
/// The HTTP client is process-scoped transport state, not catalog state. Its
/// clones share reqwest's connection pool while every build still binds fresh
/// DB-derived credentials and model membership into new adapters.
pub(crate) struct ProviderFactory {
    runtime_client: reqwest::Client,
}

impl Default for ProviderFactory {
    fn default() -> Self {
        let runtime_client = reqwest::Client::builder()
            .connect_timeout(PROVIDER_TIMEOUTS.connect())
            .build()
            // Invariant: the builder only fails on TLS backend
            // misconfiguration; setting a connect timeout cannot make it fail.
            .expect("reqwest client with connect timeout builds");
        Self { runtime_client }
    }
}

impl ProviderFactory {
    fn build(
        &self,
        provider_records: Vec<RuntimeProvider>,
    ) -> Result<LlmProviderManager, HostError> {
        let mut providers = LlmProviderManager::new();
        for provider in provider_records {
            match provider.kind {
                ProviderKind::Openai => register_openai_models(
                    &mut providers,
                    provider,
                    &self.runtime_client,
                    OPENAI_BASE_URL,
                )?,
                ProviderKind::Deepseek => register_deepseek_models(
                    &mut providers,
                    provider,
                    &self.runtime_client,
                    DEEPSEEK_BASE_URL,
                )?,
            }
        }
        Ok(providers)
    }

    pub(crate) fn validate_model(&self, kind: ProviderKind, name: &str) -> Result<(), HostError> {
        match kind {
            ProviderKind::Openai => {
                model_id("openai", name)?;
                Ok(())
            }
            ProviderKind::Deepseek => {
                deepseek_model(name)?;
                Ok(())
            }
        }
    }

    /// Builds the trusted adapter for exactly one model of one Provider
    /// snapshot, reusing the registration-path builder parameters.
    fn build_model_adapter(
        &self,
        provider: RuntimeProvider,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, HostError> {
        let api_key = ApiKey::from(provider.api_key);
        match provider.kind {
            ProviderKind::Openai => Ok(Arc::new(openai_adapter(
                &self.runtime_client,
                OPENAI_BASE_URL,
                api_key,
                model,
            )?)),
            ProviderKind::Deepseek => Ok(Arc::new(deepseek_adapter(
                &self.runtime_client,
                DEEPSEEK_BASE_URL,
                api_key,
                model,
            )?)),
        }
    }
}

fn openai_adapter(
    client: &reqwest::Client,
    base_url: &str,
    api_key: ApiKey,
    model: &str,
) -> Result<OpenAICompatibleProvider, HostError> {
    Ok(OpenAICompatibleProvider::builder()
        .client(client.clone())
        .base_url(base_url)
        .api_key(api_key)
        .model(model_id("openai", model)?)
        .timeouts(PROVIDER_TIMEOUTS)
        .build())
}

fn deepseek_adapter(
    client: &reqwest::Client,
    base_url: &str,
    api_key: ApiKey,
    model: &str,
) -> Result<DeepSeekProvider, HostError> {
    Ok(DeepSeekProvider::builder()
        .client(client.clone())
        .base_url(base_url)
        .api_key(api_key)
        .model(deepseek_model(model)?)
        .thinking(DeepSeekThinking::Disabled)
        .timeouts(PROVIDER_TIMEOUTS)
        .build())
}

fn register_openai_models(
    providers: &mut LlmProviderManager,
    provider: RuntimeProvider,
    client: &reqwest::Client,
    base_url: &str,
) -> Result<(), HostError> {
    let api_key = ApiKey::from(provider.api_key);
    for model in provider.models {
        providers.register(Arc::new(openai_adapter(
            client,
            base_url,
            api_key.clone(),
            &model,
        )?))?;
    }
    Ok(())
}

fn register_deepseek_models(
    providers: &mut LlmProviderManager,
    provider: RuntimeProvider,
    client: &reqwest::Client,
    base_url: &str,
) -> Result<(), HostError> {
    let api_key = ApiKey::from(provider.api_key);
    for model in provider.models {
        providers.register(Arc::new(deepseek_adapter(
            client,
            base_url,
            api_key.clone(),
            &model,
        )?))?;
    }
    Ok(())
}

/// Sends one real minimal message through the model's adapter and returns the
/// round-trip latency in milliseconds. The request carries a single user
/// message and no tools; the wall clock is bounded by
/// [`PROVIDER_PROBE_TIMEOUT`] independently of the adapter's own policy.
async fn probe_model_chat(provider: &dyn LlmProvider) -> Result<u64, ModelProbeError> {
    let request = ChatRequest::new(provider.model_id()).with_message(ChatMessage::user("ping"));
    let started = Instant::now();
    let result = tokio::time::timeout(PROVIDER_PROBE_TIMEOUT, provider.chat(request)).await;
    let latency_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    match result {
        Ok(Ok(_)) => Ok(latency_ms),
        Ok(Err(error)) => Err(classify_model_probe_error(error)),
        Err(_) => Err(ModelProbeError::Failed),
    }
}

/// Classifies one chat failure into a safe typed probe error. Provider status
/// payloads stay inside the discarded source; nothing upstream-derived crosses
/// into the classification.
fn classify_model_probe_error(error: LlmError) -> ModelProbeError {
    match error {
        LlmError::ProviderStatus(status) if matches!(status.status(), 401 | 403) => {
            ModelProbeError::Credentials
        }
        LlmError::ProviderStatus(status) if status.status() == 404 => {
            ModelProbeError::ModelNotAvailable
        }
        _ => ModelProbeError::Failed,
    }
}

/// Reads a config file and serves until shutdown.
///
/// # Errors
///
/// Returns [`HostError`] when the file, configuration, runtime dependencies,
/// listener, or server fails.
pub async fn run_from_path(path: impl AsRef<Path>) -> Result<(), HostError> {
    let contents = tokio::fs::read_to_string(path).await?;
    let config = Config::parse(&contents)?;
    serve(config).await
}

/// Composes providers, Postgres, the NATS tail, the shared state, and the
/// HTTP listener, then serves until SIGTERM/SIGINT with a graceful drain.
///
/// # Errors
///
/// Returns [`HostError`] when configuration or any core runtime dependency
/// cannot be initialized. A NATS connection failure degrades realtime and is
/// not fatal.
pub async fn serve(config: Config) -> Result<(), HostError> {
    config.validate()?;
    let api = config.require_api()?.clone();
    let postgres_url = config.require_postgres()?.url.as_str();
    let ontology_url = config.require_ontology()?.database_url.as_str();
    let studio_url = config.require_studio()?.database_url.as_str();
    let shutdown_drain_bound = Duration::from_secs(api.shutdown_drain_timeout_seconds);
    let pg = PostgresBackend::connect(postgres_url).await?;
    let ontology = OntologyStore::connect(ontology_url).await?;
    let studio = StudioStore::connect(studio_url).await?;
    let tail = connect_tail(&config).await;
    let state = Arc::new(AppState::with_studio(pg, tail, ontology, config, studio).await?);
    scheduler::reconcile(&state)
        .await
        .map_err(HostError::Scheduler)?;
    scheduler::start(&state);

    let listener = tokio::net::TcpListener::bind(api.bind).await?;
    let shutdown = state.shutdown_token();
    let server = axum::serve(listener, router(Arc::clone(&state)))
        .with_graceful_shutdown(shutdown.cancelled_owned());
    let mut server = Box::pin(async move { server.await });
    let mut signal_result = None;
    let server_result = tokio::select! {
        result = &mut server => Some(result),
        signal = shutdown_signal() => {
            signal_result = Some(signal);
            None
        }
    };
    // Close admission and end SSE streams, drain in-flight requests, then
    // boundedly wait for managed turn tasks. The request middleware observes
    // this token and drops any still-pending handler future, allowing Axum's
    // connection tasks to finish without converting shutdown into a Turn
    // cancellation or durable terminal event.
    state.initiate_shutdown();
    let shutdown_deadline = tokio::time::Instant::now()
        .checked_add(shutdown_drain_bound)
        .unwrap_or_else(|| {
            tracing::warn!(
                "shutdown drain bound exceeds the platform clock range; using an immediate deadline"
            );
            tokio::time::Instant::now()
        });
    let server_result = match server_result {
        Some(result) => {
            drop(server);
            Some(result)
        }
        None => {
            let result = wait_until(shutdown_deadline, server).await;
            if result.is_none() {
                tracing::warn!("http graceful shutdown timed out; terminating the server");
            }
            result
        }
    };
    let signal_result = signal_result.unwrap_or(Ok(())).map_err(HostError::from);
    let server_result = server_result.unwrap_or(Ok(())).map_err(HostError::from);
    let serve_result = signal_result.and(server_result);
    if wait_until(shutdown_deadline, state.admission().wait_drained())
        .await
        .is_none()
    {
        tracing::warn!("http admission drain timed out");
    }
    state
        .drain_runtime_tasks(
            shutdown_deadline.saturating_duration_since(tokio::time::Instant::now()),
        )
        .await;
    state.dispatchers().clear();
    serve_result
}

/// Waits for one shutdown phase within the process-wide drain deadline and
/// drops its future when the remaining budget expires.
async fn wait_until<F>(deadline: tokio::time::Instant, future: F) -> Option<F::Output>
where
    F: Future,
{
    tokio::pin!(future);
    tokio::select! {
        biased;
        output = &mut future => Some(output),
        () = tokio::time::sleep_until(deadline) => None,
    }
}

/// SIGTERM or SIGINT, whichever arrives first.
async fn shutdown_signal() -> std::io::Result<()> {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    {
        let mut sigterm =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = ctrl_c => result,
            _ = sigterm.recv() => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        ctrl_c.await
    }
}

/// Connects the NATS tail; any failure degrades realtime (the core Postgres
/// commands keep working).
async fn connect_tail(config: &Config) -> Option<NatsAgentRuntimeTail> {
    let nats = config.nats.as_ref()?;
    let mut tail_config = AgentRuntimeTailConfig::default();
    tail_config.url.clone_from(&nats.url);
    tail_config.stream_name.clone_from(&nats.stream_name);
    tail_config.subject_prefix.clone_from(&nats.subject_prefix);
    tail_config.replicas = nats.replicas;
    tail_config.max_age = Duration::from_secs(nats.max_age_seconds);
    tail_config.max_bytes = nats.max_bytes;
    tail_config.max_messages = nats.max_messages;
    let connect_timeout = Duration::from_secs(nats.connect_timeout_seconds);
    match tokio::time::timeout(connect_timeout, NatsAgentRuntimeTail::connect(tail_config)).await {
        Ok(Ok(tail)) => Some(tail),
        Ok(Err(error)) => {
            tracing::error!(error = %error, "nats tail connect failed; realtime is degraded");
            None
        }
        Err(_) => {
            tracing::error!("nats tail connect timed out; realtime is degraded");
            None
        }
    }
}

async fn providers_from_studio(
    studio: &StudioStore,
    factory: &ProviderFactory,
) -> Result<LlmProviderManager, HostError> {
    factory.build(studio.runtime_providers().await?)
}

fn deepseek_model(model: &str) -> Result<DeepSeekModel, HostError> {
    match model {
        "deepseek-v4-flash" => Ok(DeepSeekModel::V4Flash),
        "deepseek-v4-pro" => Ok(DeepSeekModel::V4Pro),
        _ => Err(HostError::UnsupportedDeepSeekModel {
            model: model_id("deepseek", model)?,
        }),
    }
}

fn model_id(provider: &'static str, model: &str) -> Result<ModelId, HostError> {
    ModelId::new(provider, model).map_err(|source| HostError::InvalidManagedModel {
        provider,
        model: model.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::future::Future;
    use std::net::SocketAddr;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    use axum::{
        Json, Router, body::Body, extract::ConnectInfo, http::StatusCode, response::Response,
        routing::post,
    };
    use serde_json::json;
    use stratum_llm::ChatRequest;

    use super::{
        HostError, ModelProbeError, ProviderFactory, probe_model_chat, register_openai_models,
        wait_until,
    };
    use stratum_core::ModelId;
    use stratum_studio::{ProviderKind, RuntimeProvider};

    #[test]
    fn registers_every_database_provider_model() {
        let records = vec![
            RuntimeProvider {
                kind: ProviderKind::Openai,
                api_key: "openai-key".into(),
                models: vec!["gpt-4.1-mini".to_owned(), "gpt-4.1".to_owned()],
            },
            RuntimeProvider {
                kind: ProviderKind::Deepseek,
                api_key: "deepseek-key".into(),
                models: vec!["deepseek-v4-flash".to_owned(), "deepseek-v4-pro".to_owned()],
            },
        ];

        let providers = ProviderFactory::default()
            .build(records)
            .expect("providers compose");

        for model in [
            "openai:gpt-4.1-mini",
            "openai:gpt-4.1",
            "deepseek:deepseek-v4-flash",
            "deepseek:deepseek-v4-pro",
        ] {
            let model: ModelId = model.parse().expect("model id parses");
            assert_eq!(
                providers.get(&model).expect("provider exists").model_id(),
                model
            );
        }
    }

    #[test]
    fn rejects_deepseek_models_not_supported_by_the_adapter() {
        let records = vec![RuntimeProvider {
            kind: ProviderKind::Deepseek,
            api_key: "deepseek-key".into(),
            models: vec!["deepseek-v4-flash".to_owned(), "deepseek-v3".to_owned()],
        }];

        assert!(matches!(
            ProviderFactory::default().build(records),
            Err(HostError::UnsupportedDeepSeekModel { .. })
        ));
    }

    #[tokio::test]
    async fn model_adapters_share_the_factory_http_connection_pool() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let peers = Arc::new(Mutex::new(BTreeSet::<SocketAddr>::new()));
        let observed_peers = Arc::clone(&peers);
        let app = Router::new().route(
            "/chat/completions",
            post(move |ConnectInfo(peer): ConnectInfo<SocketAddr>| {
                let observed_peers = Arc::clone(&observed_peers);
                async move {
                    observed_peers
                        .lock()
                        .expect("peer set lock is available")
                        .insert(peer);
                    Json(json!({
                        "choices": [{
                            "message": {"role": "assistant", "content": "ok"},
                            "finish_reason": "stop"
                        }]
                    }))
                }
            }),
        );
        let server = tokio::spawn(async move {
            axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await
            .expect("mock provider server runs");
        });
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test client builds");
        let records = RuntimeProvider {
            kind: ProviderKind::Openai,
            api_key: "openai-key".into(),
            models: vec!["model-a".to_owned(), "model-b".to_owned()],
        };
        let mut providers = stratum_llm::LlmProviderManager::new();
        register_openai_models(
            &mut providers,
            records,
            &client,
            &format!("http://{address}"),
        )
        .expect("providers compose");

        for name in ["openai:model-a", "openai:model-b"] {
            let model: ModelId = name.parse().expect("model id parses");
            providers
                .get(&model)
                .expect("provider exists")
                .chat(ChatRequest::new(model))
                .await
                .expect("mock provider responds");
        }

        assert_eq!(
            peers.lock().expect("peer set lock is available").len(),
            1,
            "model adapters must reuse the factory client's connection pool"
        );
        server.abort();
    }

    async fn chat_completions_server(
        status: StatusCode,
        body: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let server = tokio::spawn(async move {
            let app = Router::new().route(
                "/chat/completions",
                post(move || async move {
                    Response::builder()
                        .status(status)
                        .body(Body::from(body))
                        .expect("response builds")
                }),
            );
            axum::serve(listener, app)
                .await
                .expect("mock chat server runs");
        });
        (format!("http://{address}"), server)
    }

    fn loopback_probe_adapter(base_url: &str) -> Arc<dyn stratum_llm::LlmProvider> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .build()
            .expect("test client builds");
        let records = RuntimeProvider {
            kind: ProviderKind::Openai,
            api_key: "model-probe-secret".into(),
            models: vec!["model-a".to_owned()],
        };
        let mut providers = stratum_llm::LlmProviderManager::new();
        register_openai_models(&mut providers, records, &client, base_url)
            .expect("providers compose");
        providers
            .get(&"openai:model-a".parse().expect("model id parses"))
            .expect("provider exists")
    }

    #[tokio::test]
    async fn model_probe_reports_latency_for_a_real_message() {
        let (base_url, server) = chat_completions_server(
            StatusCode::OK,
            r#"{"choices":[{"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}]}"#,
        )
        .await;
        let adapter = loopback_probe_adapter(&base_url);

        let latency_ms = probe_model_chat(adapter.as_ref())
            .await
            .expect("mock provider answers the test message");

        assert!(latency_ms < 10_000);
        server.abort();
    }

    #[tokio::test]
    async fn model_probe_maps_credential_rejection_without_leaking_secrets() {
        let sentinel = "model-probe-secret";
        let (base_url, server) = chat_completions_server(
            StatusCode::UNAUTHORIZED,
            r#"{"error":{"message":"invalid key model-probe-secret"}}"#,
        )
        .await;
        let adapter = loopback_probe_adapter(&base_url);

        let error = probe_model_chat(adapter.as_ref())
            .await
            .expect_err("credential rejection fails");

        assert!(matches!(error, ModelProbeError::Credentials));
        assert!(!format!("{error:?}").contains(sentinel));
        server.abort();
    }

    #[tokio::test]
    async fn model_probe_maps_a_model_unknown_to_the_upstream() {
        let (base_url, server) = chat_completions_server(
            StatusCode::NOT_FOUND,
            r#"{"error":{"message":"model not found"}}"#,
        )
        .await;
        let adapter = loopback_probe_adapter(&base_url);

        let error = probe_model_chat(adapter.as_ref())
            .await
            .expect_err("unknown upstream model fails");

        assert!(matches!(error, ModelProbeError::ModelNotAvailable));
        server.abort();
    }

    #[tokio::test(start_paused = true)]
    async fn model_probe_timeout_is_bounded_and_sanitized() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener binds");
        let address = listener.local_addr().expect("listener has an address");
        let server = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.expect("probe connects");
            std::future::pending::<()>().await;
        });
        let adapter = loopback_probe_adapter(&format!("http://{address}"));

        let error = probe_model_chat(adapter.as_ref())
            .await
            .expect_err("stalled provider times out");

        assert!(matches!(error, ModelProbeError::Failed));
        assert!(!format!("{error:?}").contains("model-probe-secret"));
        server.abort();
    }

    struct PendingUntilDropped(Arc<AtomicBool>);

    impl Future for PendingUntilDropped {
        type Output = ();

        fn poll(self: Pin<&mut Self>, _context: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Pending
        }
    }

    impl Drop for PendingUntilDropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    #[tokio::test]
    async fn bounded_shutdown_phase_drops_a_future_that_does_not_finish() {
        let dropped = Arc::new(AtomicBool::new(false));

        let result = wait_until(
            tokio::time::Instant::now(),
            PendingUntilDropped(Arc::clone(&dropped)),
        )
        .await;

        assert!(result.is_none());
        assert!(dropped.load(Ordering::Acquire));
    }
}
