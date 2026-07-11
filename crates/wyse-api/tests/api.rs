use std::{collections::BTreeMap, fs, path::PathBuf, sync::Arc};

use async_trait::async_trait;
use chrono::Utc;
use wyse_api::{HostError, HostState};
use wyse_config::{Config, ResolvedAgentDefinition};
use wyse_core::{
    AgentEvent, AgentId, ChatMessage, EventSource, ModelId, RunId, RuntimeEvent, StreamEnvelope,
    TurnId,
};
use wyse_filesystem::{Filesystem, LocalFilesystem, LocalFilesystemConfig};
use wyse_infra::{EventStreamBus, event_stream_bus::InMemoryEventStreamBus};
use wyse_llm::{ChatRequest, ChatResponse, ChatStream, LlmError, LlmProvider, LlmProviderManager};
use wyse_store::{AgentStatus, AgentStore, FilesystemAgentStore};

struct Fixture {
    root: PathBuf,
    filesystem: Arc<dyn Filesystem>,
    config: Config,
    model: ModelId,
}

impl Fixture {
    async fn new() -> Self {
        let unique = AgentId::new();
        let root = std::env::temp_dir().join(format!("wyse-api-{unique}"));
        fs::create_dir_all(root.join("history")).expect("history directory is created");
        let filesystem: Arc<dyn Filesystem> = Arc::new(
            LocalFilesystem::new(LocalFilesystemConfig {
                root: root.clone(),
                max_file_bytes: None,
            })
            .expect("local filesystem is created"),
        );
        let model = ModelId::new("openai", "test-model").expect("model id is valid");
        let config = Config::parse(&format!(
            r#"
[agent]
storage_root = {root:?}

[llm]
default = "openai:test-model"

[llm.openai]
api_key = "test-key"
models = ["test-model"]
"#,
            root = root.to_string_lossy()
        ))
        .expect("config parses");
        Self {
            root,
            filesystem,
            config,
            model,
        }
    }

    async fn persist_agent(&self, name: &str, status: AgentStatus) -> AgentId {
        let agent_id = AgentId::new();
        let root = self.root.join("history").join(agent_id.to_string());
        fs::create_dir_all(&root).expect("agent directory is created");
        let definition = ResolvedAgentDefinition::parse(&format!(
            r#"
agent_name = "{name}"
model = "{}"
tools = ["echo"]
prompt = "be helpful"
"#,
            self.model
        ))
        .expect("definition parses");
        fs::write(
            root.join("definition.toml"),
            definition.encode().expect("definition encodes"),
        )
        .expect("definition is written");
        let store = FilesystemAgentStore::new(
            Arc::clone(&self.filesystem),
            format!("/history/{agent_id}")
                .parse()
                .expect("agent root is valid"),
        );
        store
            .initialize(agent_id, name.to_owned())
            .await
            .expect("store initializes");
        let run_id = RunId::new();
        let turn_id = TurnId::new();
        if status == AgentStatus::Running {
            store
                .update_state(status, Some(run_id), Some(turn_id), Default::default())
                .await
                .expect("state updates");
        } else if status != AgentStatus::Idle {
            store
                .append_message(StreamEnvelope {
                    business_seq: None,
                    run_id,
                    timestamp: Utc::now(),
                    source: EventSource::Run,
                    event: RuntimeEvent::Agent {
                        agent_id,
                        event: AgentEvent::Message {
                            turn_id,
                            message: ChatMessage::user("persisted message"),
                        },
                    },
                    metadata: BTreeMap::new(),
                })
                .await
                .expect("message is persisted");
            store
                .update_state(status, Some(run_id), Some(turn_id), Default::default())
                .await
                .expect("state updates");
        }
        agent_id
    }

    async fn restore_host(&self) -> Result<Arc<HostState>, HostError> {
        let mut providers = LlmProviderManager::new();
        providers
            .register(Arc::new(TestProvider(self.model.clone())))
            .expect("provider registers");
        HostState::restore(
            self.config.clone(),
            Arc::clone(&self.filesystem),
            Arc::new(InMemoryEventStreamBus::default()) as Arc<dyn EventStreamBus>,
            providers,
        )
        .await
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

struct TestProvider(ModelId);

#[async_trait]
impl LlmProvider for TestProvider {
    fn model_id(&self) -> ModelId {
        self.0.clone()
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
        Err(LlmError::MockExhausted)
    }

    async fn chat_stream(&self, _request: ChatRequest) -> Result<ChatStream, LlmError> {
        Err(LlmError::MockExhausted)
    }
}

#[tokio::test]
async fn restore_loads_complete_history_directories() {
    let fixture = Fixture::new().await;
    let agent_id = fixture
        .persist_agent("coding-agent", AgentStatus::Finished)
        .await;

    let host = fixture.restore_host().await.expect("host restores");

    assert!(host.agent(agent_id).is_some());
}

#[tokio::test]
async fn restore_marks_running_agents_as_needing_resume() {
    let fixture = Fixture::new().await;
    let agent_id = fixture
        .persist_agent("coding-agent", AgentStatus::Running)
        .await;

    let host = fixture.restore_host().await.expect("host restores");

    assert!(host.agent(agent_id).expect("agent exists").needs_resume());
}

#[tokio::test]
async fn restore_rejects_invalid_history_directory_id() {
    let fixture = Fixture::new().await;
    fs::create_dir(fixture.root.join("history/not-an-agent-id"))
        .expect("invalid directory is created");

    let error = match fixture.restore_host().await {
        Ok(_) => panic!("restore should fail"),
        Err(error) => error,
    };

    assert!(matches!(error, HostError::InvalidHistoryDirectory { .. }));
}

#[tokio::test]
async fn restore_rejects_corrupt_definition() {
    let fixture = Fixture::new().await;
    let agent_id = fixture
        .persist_agent("coding-agent", AgentStatus::Finished)
        .await;
    fs::write(
        fixture
            .root
            .join("history")
            .join(agent_id.to_string())
            .join("definition.toml"),
        "not = [valid",
    )
    .expect("definition is corrupted");

    let error = match fixture.restore_host().await {
        Ok(_) => panic!("restore should fail"),
        Err(error) => error,
    };

    assert!(matches!(error, HostError::Config(_)));
}

#[tokio::test]
async fn restore_rejects_definition_whose_model_was_removed() {
    let fixture = Fixture::new().await;
    fixture
        .persist_agent("coding-agent", AgentStatus::Finished)
        .await;
    let mut config = fixture.config.clone();
    config
        .llm
        .openai
        .as_mut()
        .expect("openai is configured")
        .models
        .clear();
    let mut providers = LlmProviderManager::new();
    providers
        .register(Arc::new(TestProvider(fixture.model.clone())))
        .expect("provider registers");

    let result = HostState::restore(
        config,
        Arc::clone(&fixture.filesystem),
        Arc::new(InMemoryEventStreamBus::default()),
        providers,
    )
    .await;
    let error = match result {
        Ok(_) => panic!("restore should fail"),
        Err(error) => error,
    };

    assert!(matches!(error, HostError::Config(_)));
}
