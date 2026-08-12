//! Shared fixtures for the stratum-api container integration tests.
//!
//! These tests run against the real Postgres and NATS services of
//! `docker-compose.test.yml` (project `stratum-api-test`) and are marked
//! `#[ignore]`; run them through the crate `Makefile` (`make
//! test-integration`), which brings the stack up and tears it down.

#![allow(dead_code)] // helpers are used by a subset of the test cases

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use futures_util::StreamExt;
use stratum_api::{AppState, router};
use stratum_config::Config;
use stratum_core::{ChatMessage, ModelConfig, ModelId};
use stratum_infra::{AgentRuntimeTailConfig, NatsAgentRuntimeTail};
use stratum_llm::{
    ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, ConfigurableLlmProvider, LlmError,
    LlmProvider, LlmProviderManager,
};
use stratum_postgres::PostgresBackend;
use tower::ServiceExt;

/// Postgres of the test compose stack.
///
/// `make test-integration` injects the dynamically published host port. The
/// fixed default remains available for manually managed `make test-up` stacks.
pub fn pg_url() -> String {
    std::env::var("STRATUM_API_TEST_PG_URL")
        .unwrap_or_else(|_| "postgres://stratum:stratum@127.0.0.1:45433/stratum_test".to_owned())
}

/// NATS of the test compose stack.
///
/// `make test-integration` injects the dynamically published host port. The
/// fixed default remains available for manually managed `make test-up` stacks.
pub fn nats_url() -> String {
    std::env::var("STRATUM_API_TEST_NATS_URL")
        .unwrap_or_else(|_| "nats://127.0.0.1:44228".to_owned())
}

/// The single mock model every test template uses.
pub const TEST_MODEL: &str = "openai:test-model";

/// One scripted LLM call outcome.
#[derive(Debug)]
pub enum Script {
    /// The provider streams these events and finishes.
    Events(Vec<ChatStreamEvent>),
    /// The provider stream never produces anything (the call hangs until the
    /// turn is cancelled or the process ends).
    Pending,
}

/// Queue-backed configurable provider shared across per-turn `configure`
/// calls, so a turn resumed by a fresh runtime continues the same script.
#[derive(Debug)]
struct MockInner {
    model: ModelId,
    queue: Mutex<VecDeque<Script>>,
    calls: AtomicUsize,
    requests: Mutex<Vec<Vec<ChatMessage>>>,
}

/// Shared handle a test uses to drive and inspect the mock while turns run.
#[derive(Debug, Clone)]
pub struct MockProvider {
    inner: Arc<MockInner>,
}

impl MockProvider {
    pub fn new(script: Vec<Script>) -> Self {
        Self {
            inner: Arc::new(MockInner {
                model: ModelId::new("openai", "test-model").expect("static model id is valid"),
                queue: Mutex::new(script.into()),
                calls: AtomicUsize::new(0),
                requests: Mutex::new(Vec::new()),
            }),
        }
    }

    /// One-call text answer script.
    pub fn text(answer: &str) -> Vec<Script> {
        vec![Script::Events(vec![
            ChatStreamEvent::TextDelta {
                delta: answer.to_owned(),
            },
            ChatStreamEvent::Finished {
                finish_reason: stratum_llm::FinishReason::Stop,
                usage: Some(stratum_core::TokenUsage {
                    input_tokens: 3,
                    output_tokens: 2,
                    total_tokens: 5,
                }),
            },
        ])]
    }

    /// One tool call followed by a final text answer.
    pub fn tool_call_then_text(call_id: &str, arguments: &str, answer: &str) -> Vec<Script> {
        vec![
            Script::Events(vec![
                ChatStreamEvent::ToolCallDelta(stratum_core::ToolCallDelta {
                    index: 0,
                    call_id: Some(stratum_core::CallId::from(call_id)),
                    name: Some("echo".to_owned()),
                    arguments_delta: arguments.to_owned(),
                }),
                ChatStreamEvent::Finished {
                    finish_reason: stratum_llm::FinishReason::ToolCalls,
                    usage: Some(stratum_core::TokenUsage {
                        input_tokens: 4,
                        output_tokens: 1,
                        total_tokens: 5,
                    }),
                },
            ]),
            Script::Events(vec![
                ChatStreamEvent::TextDelta {
                    delta: answer.to_owned(),
                },
                ChatStreamEvent::Finished {
                    finish_reason: stratum_llm::FinishReason::Stop,
                    usage: Some(stratum_core::TokenUsage {
                        input_tokens: 6,
                        output_tokens: 2,
                        total_tokens: 8,
                    }),
                },
            ]),
        ]
    }

    pub fn calls(&self) -> usize {
        self.inner.calls.load(Ordering::SeqCst)
    }

    /// Messages of every streamed call, in call order.
    pub fn captured_messages(&self) -> Vec<Vec<ChatMessage>> {
        self.inner
            .requests
            .lock()
            .expect("requests lock is not poisoned")
            .clone()
    }
}

#[async_trait::async_trait]
impl LlmProvider for MockProvider {
    fn model_id(&self) -> ModelId {
        self.inner.model.clone()
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
        Err(LlmError::UnsupportedCapability("chat"))
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<ChatStream, LlmError> {
        self.inner.calls.fetch_add(1, Ordering::SeqCst);
        self.inner
            .requests
            .lock()
            .expect("requests lock is not poisoned")
            .push(request.messages);
        let script = self
            .inner
            .queue
            .lock()
            .expect("mock queue lock is not poisoned")
            .pop_front();
        match script {
            Some(Script::Events(events)) => Ok(Box::pin(futures_util::stream::iter(
                events.into_iter().map(Ok),
            ))),
            Some(Script::Pending) => std::future::pending().await,
            None => Err(LlmError::MockExhausted),
        }
    }
}

impl ConfigurableLlmProvider for MockProvider {
    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": { "test_mode": { "type": "string" } },
            "additionalProperties": false,
            "default": {}
        })
    }

    fn default_model_config(&self) -> ModelConfig {
        ModelConfig::new(self.inner.model.clone(), serde_json::Map::new())
    }

    fn configure(
        &self,
        parameters: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<Arc<dyn LlmProvider>, LlmError> {
        let valid_test_override = parameters.len() == 1
            && parameters
                .get("test_mode")
                .is_some_and(serde_json::Value::is_string);
        if parameters.is_empty() || valid_test_override {
            // The configured provider shares the script queue, so a resumed
            // turn continues where the crashed one stopped.
            Ok(Arc::new(self.clone()))
        } else {
            Err(LlmError::InvalidModelParameters {
                model: self.inner.model.clone(),
            })
        }
    }
}

/// One assembled in-process host against the real compose services.
pub struct Fixture {
    pub root: PathBuf,
    pub state: Arc<AppState>,
    pub app: Router,
    pub provider: MockProvider,
}

impl Fixture {
    /// Assembles a host with the given template files and provider script.
    pub async fn new(templates: &[(&str, &str)], script: Vec<Script>) -> Self {
        Self::with_tail(templates, script, default_tail_config().await).await
    }

    /// Assembles a host without NATS (realtime degraded).
    pub async fn without_nats(templates: &[(&str, &str)], script: Vec<Script>) -> Self {
        Self::with_tail(templates, script, None).await
    }

    /// Assembles a host with a caller-supplied tail.
    pub async fn with_tail(
        templates: &[(&str, &str)],
        script: Vec<Script>,
        tail: Option<NatsAgentRuntimeTail>,
    ) -> Self {
        let root = std::env::temp_dir().join(format!("stratum-api-test-{}", uuid_v7()));
        std::fs::create_dir_all(&root).expect("temporary template root is created");
        for (name, contents) in templates {
            std::fs::write(root.join(format!("{name}.toml")), contents)
                .expect("template is written");
        }
        let config = test_config(&root);
        let provider = Arc::new(MockProvider::new(script));
        let mut providers = LlmProviderManager::new();
        providers
            .register(provider.clone() as Arc<dyn ConfigurableLlmProvider>)
            .expect("mock provider registers");
        let pg = PostgresBackend::connect(&pg_url())
            .await
            .expect("postgres connects");
        let state = Arc::new(
            AppState::new(pg, tail, providers, config)
                .await
                .expect("state assembles"),
        );
        let app = router(Arc::clone(&state));
        Self {
            root,
            state,
            app,
            provider: provider.as_ref().clone(),
        }
    }

    /// Adds or replaces one template file (hot catalog).
    pub fn write_template(&self, name: &str, contents: &str) {
        std::fs::write(self.root.join(format!("{name}.toml")), contents)
            .expect("template is written");
    }

    /// Removes one template file.
    pub fn remove_template(&self, name: &str) {
        std::fs::remove_file(self.root.join(format!("{name}.toml"))).expect("template is removed");
    }

    /// Issues one request against the real router.
    pub async fn request(&self, request: Request<Body>) -> Response<Body> {
        self.app
            .clone()
            .oneshot(request)
            .await
            .expect("router answers")
    }

    /// Issues one JSON request.
    pub async fn json(
        &self,
        method: &str,
        uri: &str,
        body: Option<serde_json::Value>,
        idempotency_key: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let mut builder = Request::builder().method(method).uri(uri);
        if let Some(key) = idempotency_key {
            builder = builder.header("Idempotency-Key", key);
        }
        let body = match body {
            Some(value) => Body::from(serde_json::to_vec(&value).expect("body serializes")),
            None => Body::empty(),
        };
        let request = builder
            .header("content-type", "application/json")
            .body(body)
            .expect("request builds");
        let response = self.request(request).await;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body collects");
        let json = if bytes.is_empty() {
            serde_json::Value::Null
        } else {
            serde_json::from_slice(&bytes).expect("response is json")
        };
        (status, json)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A second host over the same stores, simulating a process restart (empty
/// registry, empty dispatchers).
pub async fn restarted(fixture: &Fixture, script: Vec<Script>) -> Fixture {
    let tail = default_tail_config().await;
    let config = test_config(&fixture.root);
    let provider = Arc::new(MockProvider::new(script));
    let mut providers = LlmProviderManager::new();
    providers
        .register(provider.clone() as Arc<dyn ConfigurableLlmProvider>)
        .expect("mock provider registers");
    let pg = PostgresBackend::connect(&pg_url())
        .await
        .expect("postgres connects");
    let state = Arc::new(
        AppState::new(pg, tail, providers, config)
            .await
            .expect("state assembles"),
    );
    let app = router(Arc::clone(&state));
    Fixture {
        root: fixture.root.clone(),
        state,
        app,
        provider: provider.as_ref().clone(),
    }
}

/// Connects the default test tail (shared stream).
pub async fn default_tail_config() -> Option<NatsAgentRuntimeTail> {
    let mut config = AgentRuntimeTailConfig::default();
    config.url = nats_url();
    NatsAgentRuntimeTail::connect(config).await.ok()
}

fn test_config(root: &Path) -> Config {
    let nats_url = nats_url();
    Config::parse(&format!(
        r#"
[agent]
templates_root = {root:?}

[llm]
default = "openai:test-model"

[llm.openai]
api_key = "test-key"
models = ["test-model"]

[api]
bind = "127.0.0.1:0"

[nats]
url = {nats_url:?}
stream_name = "AGENT_RUNTIME_TAIL"
subject_prefix = "events.agent"
replicas = 1
max_age_seconds = 3600
max_bytes = 67108864
max_messages = 100000

[postgres]
url = "postgres://unused:unused@127.0.0.1:1/unused"
"#,
        root = root.to_string_lossy(),
    ))
    .expect("test config parses")
}

pub fn uuid_v7() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Polls until the probe produces a value or the deadline passes.
pub async fn wait_until<T>(deadline_secs: u64, mut probe: impl AsyncFnMut() -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(deadline_secs);
    loop {
        if let Some(value) = probe().await {
            return value;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "condition was not met within {deadline_secs}s"
        );
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

/// Reads the AgentRuntime view JSON.
pub async fn view(fixture: &Fixture, agent_runtime_id: &str) -> serde_json::Value {
    let (status, body) = fixture
        .json(
            "GET",
            &format!("/v1/agent-runtimes/{agent_runtime_id}"),
            None,
            None,
        )
        .await;
    assert_eq!(status, StatusCode::OK, "view loads: {body}");
    body
}

/// One parsed SSE event.
#[derive(Debug)]
pub struct SseEvent {
    pub id: Option<String>,
    pub data: serde_json::Value,
}

/// Reads SSE events until `stop` matches one (or `max` events were read).
pub async fn read_sse_until(
    response: Response<Body>,
    max: usize,
    stop: impl Fn(&SseEvent) -> bool,
) -> Vec<SseEvent> {
    let mut stream = response.into_body().into_data_stream();
    let mut buffer = String::new();
    let mut events = Vec::new();
    while events.len() < max {
        let Some(chunk) = stream.next().await else {
            break;
        };
        let chunk = chunk.expect("body chunk");
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buffer.find("\n\n") {
            let raw = buffer[..end].to_owned();
            buffer.drain(..end + 2);
            if raw.starts_with(':') || raw.trim().is_empty() {
                continue;
            }
            let mut id = None;
            let mut data = String::new();
            for line in raw.lines() {
                if let Some(value) = line.strip_prefix("id:") {
                    id = Some(value.trim().to_owned());
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push_str(value.trim());
                }
            }
            let event = SseEvent {
                id,
                data: serde_json::from_str(&data).expect("sse data is json"),
            };
            let done = stop(&event);
            events.push(event);
            if done {
                return events;
            }
        }
    }
    events
}

/// Reads up to `count` SSE events from a streaming response.
pub async fn read_sse_events(response: Response<Body>, count: usize) -> Vec<SseEvent> {
    let mut stream = response.into_body().into_data_stream();
    let mut buffer = String::new();
    let mut events = Vec::new();
    while events.len() < count {
        let Some(chunk) = stream.next().await else {
            break;
        };
        let chunk = chunk.expect("body chunk");
        buffer.push_str(&String::from_utf8_lossy(&chunk));
        while let Some(end) = buffer.find("\n\n") {
            let raw = buffer[..end].to_owned();
            buffer.drain(..end + 2);
            if raw.starts_with(':') || raw.trim().is_empty() {
                continue;
            }
            let mut id = None;
            let mut data = String::new();
            for line in raw.lines() {
                if let Some(value) = line.strip_prefix("id:") {
                    id = Some(value.trim().to_owned());
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push_str(value.trim());
                }
            }
            events.push(SseEvent {
                id,
                data: serde_json::from_str(&data).expect("sse data is json"),
            });
        }
    }
    events
}
