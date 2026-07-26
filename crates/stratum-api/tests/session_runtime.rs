use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, Response, StatusCode},
};
use futures_util::{StreamExt, stream};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use stratum_api::{AgentCreated, AgentView, HostState, TurnAccepted, router};
use stratum_config::Config;
use stratum_core::{
    AgentEvent, AgentId, AgentLocation, EventCursor, HistoryPage, ModelConfig, ModelId,
    RuntimeEvent, SessionId, StreamEnvelope, TurnId,
};
use stratum_filesystem::{Filesystem, LocalFilesystem, LocalFilesystemConfig};
use stratum_infra::{EventStreamBus, event_stream_bus::InMemoryEventStreamBus};
use stratum_llm::{
    ApiKey, ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, ConfigurableLlmProvider,
    DeepSeekModel, DeepSeekProvider, DeepSeekThinking, FinishReason, LlmError, LlmProvider,
    LlmProviderManager,
};
use stratum_store::AgentStatus;
use tokio::{sync::Notify, time::timeout};
use tower::ServiceExt;

const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";

struct IsolatedRoot {
    path: PathBuf,
    complete: bool,
}

impl IsolatedRoot {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("stratum-{label}-{}", SessionId::new()));
        fs::create_dir_all(path.join("history")).expect("history directory is created");
        fs::create_dir(path.join("templates")).expect("templates directory is created");
        Self {
            path,
            complete: false,
        }
    }

    fn finish(mut self) {
        fs::remove_dir_all(&self.path).expect("successful test removes isolated data");
        self.complete = true;
    }
}

impl Drop for IsolatedRoot {
    fn drop(&mut self) {
        if !self.complete {
            eprintln!(
                "session runtime test data retained for diagnosis: {}",
                self.path.display()
            );
        }
    }
}

#[derive(Clone)]
struct ControlledProvider {
    model: ModelId,
    responses: Arc<Mutex<VecDeque<ControlledResponse>>>,
}

enum ControlledResponse {
    Immediate(&'static str),
    Gated {
        entered: Arc<Notify>,
        release: Arc<Notify>,
        text: &'static str,
    },
}

impl ControlledProvider {
    fn new(model: ModelId, responses: Vec<ControlledResponse>) -> Self {
        Self {
            model,
            responses: Arc::new(Mutex::new(responses.into())),
        }
    }
}

#[async_trait]
impl LlmProvider for ControlledProvider {
    fn model_id(&self) -> ModelId {
        self.model.clone()
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
        Err(LlmError::UnsupportedCapability("chat"))
    }

    async fn chat_stream(&self, _request: ChatRequest) -> Result<ChatStream, LlmError> {
        let response = self
            .responses
            .lock()
            .expect("response queue lock is not poisoned")
            .pop_front()
            .ok_or(LlmError::MockExhausted)?;
        let text = match response {
            ControlledResponse::Immediate(text) => text,
            ControlledResponse::Gated {
                entered,
                release,
                text,
            } => {
                entered.notify_one();
                release.notified().await;
                text
            }
        };
        Ok(Box::pin(stream::iter([
            Ok(ChatStreamEvent::TextDelta {
                delta: text.to_owned(),
            }),
            Ok(ChatStreamEvent::Finished {
                finish_reason: FinishReason::Stop,
                usage: None,
            }),
        ])))
    }
}

impl ConfigurableLlmProvider for ControlledProvider {
    fn parameter_schema(&self) -> Value {
        json!({"type": "object", "additionalProperties": false, "default": {}})
    }

    fn default_model_config(&self) -> ModelConfig {
        ModelConfig::new(self.model.clone(), Map::new())
    }

    fn configure(&self, parameters: &Map<String, Value>) -> Result<Arc<dyn LlmProvider>, LlmError> {
        if parameters.is_empty() {
            Ok(Arc::new(self.clone()))
        } else {
            Err(LlmError::InvalidModelParameters {
                model: self.model.clone(),
            })
        }
    }
}

struct Harness {
    config: Config,
    filesystem: Arc<dyn Filesystem>,
    bus: Arc<InMemoryEventStreamBus>,
}

impl Harness {
    fn new(root: &Path, model: &ModelId) -> Self {
        let config = config(root, model);
        fs::write(
            root.join("templates/coding-agent.toml"),
            format!("model = \"{model}\"\ntools = []\nprompt = \"be concise\"\n"),
        )
        .expect("template is written");
        let filesystem: Arc<dyn Filesystem> = Arc::new(
            LocalFilesystem::new(LocalFilesystemConfig {
                root: root.to_path_buf(),
                max_file_bytes: None,
            })
            .expect("local filesystem is created"),
        );
        Self {
            config,
            filesystem,
            bus: Arc::new(InMemoryEventStreamBus::default()),
        }
    }

    async fn restore(&self, provider: Arc<dyn ConfigurableLlmProvider>) -> Arc<HostState> {
        let mut providers = LlmProviderManager::new();
        providers.register(provider).expect("provider registers");
        HostState::restore(
            self.config.clone(),
            Arc::clone(&self.filesystem),
            Arc::clone(&self.bus) as Arc<dyn EventStreamBus>,
            providers,
        )
        .await
        .expect("host restores")
    }
}

#[derive(Default)]
struct SseCapture {
    cursors: Vec<EventCursor>,
    envelopes: Vec<StreamEnvelope>,
}

#[tokio::test]
async fn session_runtime_survives_restart_and_isolates_agent_history() {
    let root = IsolatedRoot::new("session-runtime-e2e");
    let model = ModelId::new("openai", "deterministic").expect("model id is valid");
    let entered = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let provider = Arc::new(ControlledProvider::new(
        model.clone(),
        vec![
            ControlledResponse::Immediate("first answer"),
            ControlledResponse::Immediate("second answer"),
            ControlledResponse::Gated {
                entered: Arc::clone(&entered),
                release: Arc::clone(&release),
                text: "third answer",
            },
            ControlledResponse::Immediate("other agent answer"),
        ],
    ));
    let harness = Harness::new(&root.path, &model);
    let session_id = SessionId::new();
    let host = harness.restore(provider.clone()).await;
    let app = router(Arc::clone(&host));

    let first_stream = open_session_stream(&app, session_id, None).await;
    let first_capture = tokio::spawn(collect_sse_until_terminals(first_stream, 1));
    let created: AgentCreated = response_json(
        post_json(
            &app,
            "/v1/agents",
            json!({
                "agent_name": "coding-agent",
                "session_id": session_id,
                "text": "first question"
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let first_capture = timeout(Duration::from_secs(5), first_capture)
        .await
        .expect("first Turn emits a terminal event")
        .expect("SSE collector completes");
    assert_agent_events(
        &first_capture.envelopes,
        session_id,
        created.agent_id,
        created.turn_id,
    );
    assert_message_sequences(&first_capture.envelopes, created.agent_id, &[1, 2]);
    let first_cursor = *first_capture.cursors.last().expect("first cursor exists");
    let first_state = host
        .agent(created.agent_id)
        .expect("created Agent is hosted")
        .store
        .load_agent()
        .await
        .expect("first state loads");
    let first_snapshot = first_state
        .turn_runtime_snapshot
        .clone()
        .expect("first Turn snapshot is persisted");
    let first_history = history(&app, created.agent_id).await;
    assert_eq!(first_history.events.len(), 2);

    host.shutdown().await;
    let restarted = harness.restore(provider.clone()).await;
    let restarted_state = restarted
        .agent(created.agent_id)
        .expect("Agent is restored")
        .store
        .load_agent()
        .await
        .expect("restored state loads");
    assert_eq!(restarted_state.session_id, Some(session_id));
    assert_eq!(restarted_state.turn_runtime_snapshot, Some(first_snapshot));
    let app = router(Arc::clone(&restarted));

    let reconnect = open_session_stream(&app, session_id, Some(first_cursor)).await;
    let second_capture = tokio::spawn(collect_sse_until_terminals(reconnect, 1));
    let second: TurnAccepted = response_json(
        post_json(
            &app,
            &format!("/v1/agents/{}/messages", created.agent_id),
            json!({"text": "second question"}),
        )
        .await,
        StatusCode::ACCEPTED,
    )
    .await;
    let second_capture = timeout(Duration::from_secs(5), second_capture)
        .await
        .expect("second Turn emits a terminal event")
        .expect("SSE collector completes");
    assert_eq!(second.session_id, session_id);
    assert_ne!(second.turn_id, created.turn_id);
    assert_agent_events(
        &second_capture.envelopes,
        session_id,
        created.agent_id,
        second.turn_id,
    );
    assert_message_sequences(&second_capture.envelopes, created.agent_id, &[3, 4]);
    assert!(
        second_capture
            .cursors
            .first()
            .is_some_and(|cursor| cursor.transport_sequence() > first_cursor.transport_sequence())
    );
    let second_cursor = *second_capture.cursors.last().expect("second cursor exists");
    let second_history = history(&app, created.agent_id).await;
    assert_eq!(second_history.events.len(), 4);
    assert_deduplicated_history(&second_history, &second_capture.envelopes, created.agent_id);

    let shared_stream = open_session_stream(&app, session_id, Some(second_cursor)).await;
    let shared_capture = tokio::spawn(collect_sse_until_terminals(shared_stream, 2));
    let third = timeout(Duration::from_secs(5), async {
        loop {
            let response = post_json(
                &app,
                &format!("/v1/agents/{}/messages", created.agent_id),
                json!({"text": "hold the Session"}),
            )
            .await;
            if response.status() == StatusCode::ACCEPTED {
                return decode_json::<TurnAccepted>(response).await;
            }
            assert_eq!(response.status(), StatusCode::CONFLICT);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Session reservation is released after the second Turn");
    timeout(Duration::from_secs(5), entered.notified())
        .await
        .expect("third Turn enters the controlled provider");
    let conflict = post_json(
        &app,
        "/v1/agents",
        json!({
            "agent_name": "coding-agent",
            "session_id": session_id,
            "text": "must conflict"
        }),
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    let conflict: Value = decode_json(conflict).await;
    assert_eq!(conflict["error"]["code"], "agent_busy");
    release.notify_one();
    wait_for_status(&app, created.agent_id, AgentStatus::Finished).await;

    let second_agent = timeout(Duration::from_secs(5), async {
        loop {
            let response = post_json(
                &app,
                "/v1/agents",
                json!({
                    "agent_name": "coding-agent",
                    "session_id": session_id,
                    "text": "independent history"
                }),
            )
            .await;
            if response.status() == StatusCode::CREATED {
                return decode_json::<AgentCreated>(response).await;
            }
            assert_eq!(response.status(), StatusCode::CONFLICT);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Session reservation is released after the third Turn");
    let shared_capture = timeout(Duration::from_secs(5), shared_capture)
        .await
        .expect("shared Session stream observes both terminal events")
        .expect("SSE collector completes");
    assert!(shared_capture.envelopes.iter().any(|envelope| matches!(
        envelope.event,
        RuntimeEvent::Agent { agent_id, .. } if agent_id == created.agent_id
    )));
    assert!(shared_capture.envelopes.iter().any(|envelope| matches!(
        envelope.event,
        RuntimeEvent::Agent { agent_id, .. } if agent_id == second_agent.agent_id
    )));
    assert_eq!(third.session_id, session_id);
    assert_eq!(second_agent.session_id, session_id);
    let first_agent_history = history(&app, created.agent_id).await;
    let second_agent_history = history(&app, second_agent.agent_id).await;
    assert_eq!(first_agent_history.events.len(), 6);
    assert_eq!(second_agent_history.events.len(), 2);
    assert_message_sequences(&second_agent_history.events, second_agent.agent_id, &[1, 2]);

    let legacy = post_json(
        &app,
        "/v1/agents",
        json!({
            "agent_name": "coding-agent",
            "text": "legacy",
            "run_id": SessionId::new(),
            "source": "run"
        }),
    )
    .await;
    assert_eq!(legacy.status(), StatusCode::BAD_REQUEST);

    restarted.shutdown().await;
    root.finish();
}

#[tokio::test]
#[ignore = "requires DEEPSEEK_API_KEY and live DeepSeek access"]
async fn live_deepseek_session_survives_host_restart() {
    let api_key = std::env::var("DEEPSEEK_API_KEY")
        .expect("DEEPSEEK_API_KEY must be exported before running the ignored test");
    let root = IsolatedRoot::new("deepseek-session-e2e");
    let model = ModelId::new("deepseek", "deepseek-v4-flash").expect("model id is valid");
    let harness = Harness::new(&root.path, &model);
    let provider = || -> Arc<dyn ConfigurableLlmProvider> {
        Arc::new(DeepSeekProvider::new(
            DEEPSEEK_BASE_URL,
            ApiKey::new(api_key.clone()),
            DeepSeekModel::V4Flash,
            DeepSeekThinking::Disabled,
        ))
    };
    let session_id = SessionId::new();
    let host = harness.restore(provider()).await;
    let app = router(Arc::clone(&host));
    let stream = open_session_stream(&app, session_id, None).await;
    let capture = tokio::spawn(collect_sse_until_terminals(stream, 1));
    let created: AgentCreated = response_json(
        post_json(
            &app,
            "/v1/agents",
            json!({
                "agent_name": "coding-agent",
                "session_id": session_id,
                "text": "Reply with one short sentence."
            }),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    let capture = timeout(Duration::from_secs(180), capture)
        .await
        .expect("live first Turn reaches a terminal event")
        .expect("SSE collector completes");
    let cursor = *capture.cursors.last().expect("first cursor exists");
    assert_agent_events(
        &capture.envelopes,
        session_id,
        created.agent_id,
        created.turn_id,
    );
    host.shutdown().await;

    let restarted = harness.restore(provider()).await;
    let app = router(Arc::clone(&restarted));
    let stream = open_session_stream(&app, session_id, Some(cursor)).await;
    let capture = tokio::spawn(collect_sse_until_terminals(stream, 1));
    let second: TurnAccepted = response_json(
        post_json(
            &app,
            &format!("/v1/agents/{}/messages", created.agent_id),
            json!({"text": "Reply with another short sentence."}),
        )
        .await,
        StatusCode::ACCEPTED,
    )
    .await;
    let capture = timeout(Duration::from_secs(180), capture)
        .await
        .expect("live second Turn reaches a terminal event")
        .expect("SSE collector completes");
    assert_eq!(second.session_id, session_id);
    assert_ne!(second.turn_id, created.turn_id);
    assert_agent_events(
        &capture.envelopes,
        session_id,
        created.agent_id,
        second.turn_id,
    );
    assert_eq!(history(&app, created.agent_id).await.events.len(), 4);
    assert_directory_excludes_secret(&root.path, api_key.as_bytes());
    restarted.shutdown().await;
    root.finish();
}

fn config(root: &Path, model: &ModelId) -> Config {
    Config::parse(&format!(
        r#"
[agent]
storage_root = {root:?}

[llm]
default = "{model}"

[llm.openai]
api_key = "not-used"
models = ["deterministic"]

[llm.deepseek]
api_key = "not-used"
models = ["deepseek-v4-flash"]

[api]
bind = "127.0.0.1:0"
"#,
        root = root.to_string_lossy(),
    ))
    .expect("test config parses")
}

async fn post_json(app: &Router, path: &str, value: Value) -> Response<Body> {
    app.clone()
        .oneshot(
            Request::post(path)
                .header("content-type", "application/json")
                .body(Body::from(value.to_string()))
                .expect("request builds"),
        )
        .await
        .expect("request completes")
}

async fn response_json<T: DeserializeOwned>(response: Response<Body>, status: StatusCode) -> T {
    if response.status() != status {
        let actual = response.status();
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("error response body is readable");
        panic!(
            "unexpected response status: expected {status}, got {actual}: {}",
            String::from_utf8_lossy(&body)
        );
    }
    decode_json(response).await
}

async fn decode_json<T: DeserializeOwned>(response: Response<Body>) -> T {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body is readable");
    serde_json::from_slice(&body).expect("response body decodes")
}

async fn open_session_stream(
    app: &Router,
    session_id: SessionId,
    after: Option<EventCursor>,
) -> Response<Body> {
    let query = after.map_or_else(
        || "replay=all".to_owned(),
        |cursor| format!("after_cursor={}", cursor.transport_sequence()),
    );
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/v1/sessions/{session_id}/events?{query}"))
                .body(Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("SSE request completes");
    assert_eq!(response.status(), StatusCode::OK);
    response
}

async fn collect_sse_until_terminals(response: Response<Body>, target: usize) -> SseCapture {
    let mut body = response.into_body().into_data_stream();
    let mut pending = String::new();
    let mut capture = SseCapture::default();
    let mut terminals = 0;
    while let Some(chunk) = body.next().await {
        let chunk = chunk.expect("SSE body chunk is valid");
        pending.push_str(std::str::from_utf8(&chunk).expect("SSE is UTF-8"));
        pending = pending.replace("\r\n", "\n");
        while let Some(boundary) = pending.find("\n\n") {
            let block = pending[..boundary].to_owned();
            pending.drain(..boundary + 2);
            let mut cursor = None;
            let mut data = Vec::new();
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("id: ") {
                    cursor = value
                        .parse::<u64>()
                        .ok()
                        .map(EventCursor::from_transport_sequence);
                } else if let Some(value) = line.strip_prefix("data: ") {
                    data.push(value);
                }
            }
            if data.is_empty() {
                continue;
            }
            let envelope: StreamEnvelope =
                serde_json::from_str(&data.join("\n")).expect("SSE data is an envelope");
            if let Some(cursor) = cursor {
                capture.cursors.push(cursor);
            }
            if matches!(
                envelope.event,
                RuntimeEvent::Agent {
                    event: AgentEvent::Finished { .. }
                        | AgentEvent::Failed { .. }
                        | AgentEvent::Cancelled { .. },
                    ..
                }
            ) {
                terminals += 1;
            }
            capture.envelopes.push(envelope);
            if terminals == target {
                return capture;
            }
        }
    }
    panic!("SSE stream closed before {target} terminal events");
}

async fn history(app: &Router, agent_id: AgentId) -> HistoryPage {
    response_json(
        app.clone()
            .oneshot(
                Request::get(format!(
                    "/v1/agents/{agent_id}/messages?after_seq=0&limit=256"
                ))
                .body(Body::empty())
                .expect("history request builds"),
            )
            .await
            .expect("history request completes"),
        StatusCode::OK,
    )
    .await
}

async fn wait_for_status(app: &Router, agent_id: AgentId, expected: AgentStatus) {
    timeout(Duration::from_secs(5), async {
        loop {
            let view: AgentView = response_json(
                app.clone()
                    .oneshot(
                        Request::get(format!("/v1/agents/{agent_id}"))
                            .body(Body::empty())
                            .expect("view request builds"),
                    )
                    .await
                    .expect("view request completes"),
                StatusCode::OK,
            )
            .await;
            if view.status == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Agent reaches expected status");
}

fn assert_agent_events(
    envelopes: &[StreamEnvelope],
    session_id: SessionId,
    agent_id: AgentId,
    turn_id: TurnId,
) {
    assert!(!envelopes.is_empty());
    for envelope in envelopes {
        assert_eq!(envelope.session_id, session_id);
        let RuntimeEvent::Agent {
            agent_id: actual_agent_id,
            turn_id: actual_turn_id,
            location,
            ..
        } = &envelope.event
        else {
            panic!("Turn stream should contain Agent events");
        };
        assert_eq!(*actual_agent_id, agent_id);
        assert_eq!(*actual_turn_id, turn_id);
        assert_eq!(*location, AgentLocation::Direct);
    }
}

fn assert_message_sequences(envelopes: &[StreamEnvelope], agent_id: AgentId, expected: &[u64]) {
    let actual = envelopes
        .iter()
        .filter_map(|envelope| match &envelope.event {
            RuntimeEvent::Agent {
                agent_id: actual_agent_id,
                event: AgentEvent::Message { message_seq, .. },
                ..
            } if *actual_agent_id == agent_id => Some(*message_seq),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

fn assert_deduplicated_history(history: &HistoryPage, live: &[StreamEnvelope], agent_id: AgentId) {
    let keys = history
        .events
        .iter()
        .chain(live)
        .filter_map(|envelope| match &envelope.event {
            RuntimeEvent::Agent {
                agent_id,
                event: AgentEvent::Message { message_seq, .. },
                ..
            } => Some((*agent_id, *message_seq)),
            _ => None,
        })
        .collect::<HashSet<_>>();
    assert_eq!(keys.len(), history.events.len());
    assert!(
        keys.iter()
            .all(|(actual_agent_id, _)| *actual_agent_id == agent_id)
    );
}

fn assert_directory_excludes_secret(path: &Path, secret: &[u8]) {
    for entry in fs::read_dir(path).expect("test data directory is readable") {
        let entry = entry.expect("directory entry is readable");
        let file_type = entry.file_type().expect("file type is readable");
        if file_type.is_dir() {
            assert_directory_excludes_secret(&entry.path(), secret);
        } else if file_type.is_file() {
            let contents = fs::read(entry.path()).expect("persisted file is readable");
            assert!(
                !contents
                    .windows(secret.len())
                    .any(|window| window == secret),
                "persisted runtime data must not contain DEEPSEEK_API_KEY"
            );
        }
    }
}
