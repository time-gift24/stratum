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
mod provenance;
mod registry;
mod sink;
mod state;
mod telemetry;
mod templates;
mod turn;

use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use stratum_config::{Config, ProviderConfig};
use stratum_core::ModelId;
use stratum_infra::{AgentRuntimeTailConfig, NatsAgentRuntimeTail};
use stratum_llm::{
    ApiKey, DeepSeekModel, DeepSeekProvider, DeepSeekThinking, LlmProviderManager, LlmTimeouts,
    OpenAICompatibleProvider,
};
use stratum_ontology::OntologyStore;
use stratum_postgres::PostgresBackend;

pub use dto::{
    AgentRuntimeCreated, AgentRuntimeStatusDto, AgentRuntimeView, AgentTemplateDto,
    AgentTemplatesResponse, CreateAgentRuntimeRequest, HistoryItemDto, HistoryResponse,
    LivenessResponse, ModelsResponse, PendingApprovalDto, ReadinessResponse, TurnAccepted,
};
pub use error::{ApiError, ErrorKind, ErrorResponse};
pub use frames::{AgentRuntimeProductEventV1, AgentRuntimeStreamFrameV1};
pub use host_error::HostError;
pub use http::router;
pub use state::AppState;
pub use telemetry::{TelemetryGuard, init_telemetry};

/// Well-known public OpenAI API endpoint. This is a product constant, not an
/// environment-specific value; deployments override it via
/// `[llm.openai].base_url` when routing through a gateway or compatible API.
const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
/// Well-known public DeepSeek API endpoint; same product-constant contract as
/// [`OPENAI_BASE_URL`], overridable via `[llm.deepseek].base_url`.
const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";
/// Reads a config file and serves until shutdown.
///
/// # Errors
///
/// Returns [`HostError`] when the file, configuration, runtime dependencies,
/// listener, or server fails.
pub async fn run_from_path(path: impl AsRef<Path>) -> Result<(), HostError> {
    let contents = tokio::fs::read_to_string(path).await?;
    let mut config = Config::parse(&contents)?;
    apply_deepseek_api_key_environment(&mut config)?;
    serve(config).await
}

fn apply_deepseek_api_key_environment(config: &mut Config) -> Result<(), HostError> {
    match std::env::var("DEEPSEEK_API_KEY") {
        Ok(api_key) => config
            .override_deepseek_api_key(api_key.into())
            .map_err(Into::into),
        Err(std::env::VarError::NotPresent) => Ok(()),
        Err(std::env::VarError::NotUnicode(_)) => Err(HostError::InvalidDeepSeekApiKeyEnvironment),
    }
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
    let api = config.require_api()?.clone();
    let postgres_url = config.require_postgres()?.url.as_str();
    let ontology_url = config.require_ontology()?.database_url.as_str();
    let shutdown_drain_bound = Duration::from_secs(api.shutdown_drain_timeout_seconds);
    let pg = PostgresBackend::connect(postgres_url).await?;
    let ontology = OntologyStore::connect(ontology_url).await?;
    let tail = connect_tail(&config).await;
    let providers = providers(&config)?;
    let state = Arc::new(AppState::new(pg, tail, providers, ontology, config).await?);

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

/// Builds the provider registry from `[llm]`.
///
/// # Errors
///
/// Returns [`HostError`] when a configured model cannot be registered.
pub fn providers(config: &Config) -> Result<LlmProviderManager, HostError> {
    let mut providers = LlmProviderManager::new();
    if let Some(provider) = &config.llm.openai {
        register_openai(&mut providers, provider)?;
    }
    if let Some(provider) = &config.llm.deepseek {
        register_deepseek(&mut providers, provider)?;
    }
    Ok(providers)
}

fn register_openai(
    providers: &mut LlmProviderManager,
    config: &ProviderConfig,
) -> Result<(), HostError> {
    let base_url = config.base_url.as_deref().unwrap_or(OPENAI_BASE_URL);
    let api_key = ApiKey::from(config.api_key.clone());
    let timeouts = provider_timeouts(config);
    for model in &config.models {
        let model_id = model_id("openai", model)?;
        providers.register(Arc::new(OpenAICompatibleProvider::new(
            base_url,
            api_key.clone(),
            model_id,
            timeouts,
        )))?;
    }
    Ok(())
}

fn register_deepseek(
    providers: &mut LlmProviderManager,
    config: &ProviderConfig,
) -> Result<(), HostError> {
    let api_key = ApiKey::from(config.api_key.clone());
    let timeouts = provider_timeouts(config);
    for model in &config.models {
        let adapter_model = match model.as_str() {
            "deepseek-v4-flash" => DeepSeekModel::V4Flash,
            "deepseek-v4-pro" => DeepSeekModel::V4Pro,
            _ => {
                return Err(HostError::UnsupportedDeepSeekModel {
                    model: model_id("deepseek", model)?,
                });
            }
        };
        providers.register(Arc::new(DeepSeekProvider::new(
            config.base_url.as_deref().unwrap_or(DEEPSEEK_BASE_URL),
            api_key.clone(),
            adapter_model,
            DeepSeekThinking::Disabled,
            timeouts,
        )))?;
    }
    Ok(())
}

fn provider_timeouts(config: &ProviderConfig) -> LlmTimeouts {
    LlmTimeouts::new(
        Duration::from_secs(config.connect_timeout_seconds),
        Duration::from_secs(config.request_timeout_seconds),
        Duration::from_secs(config.first_response_timeout_seconds),
        Duration::from_secs(config.stream_idle_timeout_seconds),
    )
}

fn model_id(provider: &'static str, model: &str) -> Result<ModelId, HostError> {
    ModelId::new(provider, model).map_err(|source| HostError::InvalidConfiguredModel {
        provider,
        model: model.to_owned(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::task::{Context, Poll};

    use super::{HostError, providers, wait_until};
    use stratum_config::Config;
    use stratum_core::ModelId;

    #[test]
    fn registers_every_configured_openai_and_deepseek_model() {
        let config = Config::parse(
            r#"
[agent]
templates_root = "."

[llm]
default = "openai:gpt-4.1-mini"

[llm.openai]
api_key = "openai-key"
models = ["gpt-4.1-mini", "gpt-4.1"]

[llm.deepseek]
api_key = "deepseek-key"
models = ["deepseek-v4-flash", "deepseek-v4-pro"]
"#,
        )
        .expect("config parses");

        let providers = providers(&config).expect("providers compose");

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
        let config = Config::parse(
            r#"
[agent]
templates_root = "."

[llm]
default = "deepseek:deepseek-v4-flash"

[llm.deepseek]
api_key = "deepseek-key"
models = ["deepseek-v4-flash", "deepseek-v3"]
"#,
        )
        .expect("config parses");

        assert!(matches!(
            providers(&config),
            Err(HostError::UnsupportedDeepSeekModel { .. })
        ));
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
