use std::{path::PathBuf, sync::Arc};

use clap::Parser;
use thiserror::Error;
use wyse_agent::{Agent, AgentError};
use wyse_agent_builtin::build_default_agent;
use wyse_core::{AgentId, ModelId, ModelIdParseError};
use wyse_filesystem::{
    Filesystem, FilesystemError, LocalFilesystem, LocalFilesystemConfig, VirtualPath,
    VirtualPathError,
};
use wyse_infra::{EventStreamBus, EventStreamBusError, event_stream_bus::InMemoryEventStreamBus};
use wyse_llm::{
    ApiKey, DeepSeekModel, DeepSeekProvider, DeepSeekThinking, LlmError, LlmProvider,
    OpenAICompatibleProvider,
};
use wyse_store::{AgentStore, FilesystemAgentStore, StoreError, StoreEventStreamBus};

const CONFIG_PATH: &str = "config.toml";
const DEFAULT_AGENT_NAME: &str = "default-agent";
const OPENAI_BASE_URL: &str = "https://api.openai.com/v1";
const DEEPSEEK_BASE_URL: &str = "https://api.deepseek.com";

#[derive(Parser)]
#[command(name = "stratum-repl")]
struct Args {
    #[arg(long)]
    resume: Option<AgentId>,
    #[arg(long)]
    debug: bool,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    stratum: StratumConfig,
    openai: Option<ProviderConfig>,
    deepseek: Option<ProviderConfig>,
}

impl Config {
    fn read() -> Result<Self, ReplError> {
        let contents = std::fs::read_to_string(CONFIG_PATH)?;
        toml::from_str(&contents).map_err(ReplError::from)
    }
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct StratumConfig {
    storage_root: PathBuf,
    model: ModelId,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderConfig {
    api_key: String,
}

struct Session {
    agent_id: AgentId,
    agent: Agent,
    bus: Arc<dyn EventStreamBus>,
    storage_root: PathBuf,
}

#[derive(Debug, Error)]
enum ReplError {
    #[error("failed to parse command line arguments")]
    Args(#[from] clap::Error),
    #[error("failed to read configuration")]
    Io(#[from] std::io::Error),
    #[error("failed to parse configuration")]
    Toml(#[from] toml::de::Error),
    #[error("invalid model id")]
    ModelId(#[from] ModelIdParseError),
    #[error("agent operation failed")]
    Agent(#[from] AgentError),
    #[error("agent store operation failed")]
    Store(#[from] StoreError),
    #[error("filesystem operation failed")]
    Filesystem(#[from] FilesystemError),
    #[error("invalid virtual path")]
    VirtualPath(#[from] VirtualPathError),
    #[error("event stream bus operation failed")]
    EventStreamBus(#[from] EventStreamBusError),
    #[error("llm operation failed")]
    Llm(#[from] LlmError),
    #[error("json encoding failed")]
    Json(#[from] serde_json::Error),
    #[error("unsupported provider: {provider}")]
    UnsupportedProvider { provider: String },
    #[error("unsupported model: {model}")]
    UnsupportedModel { model: ModelId },
    #[error("missing provider configuration: {provider}")]
    MissingProviderConfiguration { provider: &'static str },
}

#[tokio::main]
async fn main() -> Result<(), ReplError> {
    let args = Args::parse();
    let config = Config::read()?;
    let agent_id = args.resume.unwrap_or_else(AgentId::new);
    let session = compose_session(&config, agent_id, args.resume.is_none()).await?;

    let _ = (
        args.debug,
        session.agent_id,
        &session.agent,
        &session.bus,
        &session.storage_root,
    );
    Ok(())
}

async fn compose_session(
    config: &Config,
    agent_id: AgentId,
    initialize: bool,
) -> Result<Session, ReplError> {
    std::fs::create_dir_all(&config.stratum.storage_root)?;
    let filesystem: Arc<dyn Filesystem> = Arc::new(LocalFilesystem::new(LocalFilesystemConfig {
        root: config.stratum.storage_root.clone(),
        max_file_bytes: None,
    })?);
    let store = Arc::new(FilesystemAgentStore::new(filesystem, agent_root(agent_id)?));

    if initialize {
        store
            .initialize(agent_id, DEFAULT_AGENT_NAME.to_owned())
            .await?;
    } else {
        store.load_agent().await?;
    }

    let store: Arc<dyn AgentStore> = store;
    let bus: Arc<dyn EventStreamBus> = Arc::new(StoreEventStreamBus::new(
        store.clone(),
        Arc::new(InMemoryEventStreamBus::default()),
    ));
    let agent = build_default_agent(agent_id, store, bus.clone(), select_provider(config)?)?;

    Ok(Session {
        agent_id,
        agent,
        bus,
        storage_root: config.stratum.storage_root.clone(),
    })
}

fn agent_root(agent_id: AgentId) -> Result<VirtualPath, ReplError> {
    VirtualPath::try_from(format!("/{agent_id}").as_str()).map_err(ReplError::from)
}

fn select_provider(config: &Config) -> Result<Arc<dyn LlmProvider>, ReplError> {
    let model = &config.stratum.model;
    match model.provider_name() {
        "openai" => {
            let provider = config
                .openai
                .as_ref()
                .ok_or(ReplError::MissingProviderConfiguration { provider: "openai" })?;
            Ok(Arc::new(OpenAICompatibleProvider::new(
                OPENAI_BASE_URL,
                ApiKey::new(provider.api_key.clone()),
                model.clone(),
            )))
        }
        "deepseek" => {
            let provider =
                config
                    .deepseek
                    .as_ref()
                    .ok_or(ReplError::MissingProviderConfiguration {
                        provider: "deepseek",
                    })?;
            let deepseek_model = match model.model_name() {
                "deepseek-v4-flash" => DeepSeekModel::V4Flash,
                "deepseek-v4-pro" => DeepSeekModel::V4Pro,
                _ => {
                    return Err(ReplError::UnsupportedModel {
                        model: model.clone(),
                    });
                }
            };
            Ok(Arc::new(DeepSeekProvider::new(
                DEEPSEEK_BASE_URL,
                ApiKey::new(provider.api_key.clone()),
                deepseek_model,
                DeepSeekThinking::Disabled,
            )))
        }
        provider => Err(ReplError::UnsupportedProvider {
            provider: provider.to_owned(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use wyse_core::AgentId;

    use super::{Args, Config, ReplError, agent_root, select_provider};

    #[test]
    fn parses_resume_and_debug_arguments() -> Result<(), ReplError> {
        let agent_id = AgentId::new();

        let args =
            Args::try_parse_from(["stratum-repl", "--resume", &agent_id.to_string(), "--debug"])?;

        assert_eq!(args.resume, Some(agent_id));
        assert!(args.debug);
        Ok(())
    }

    #[test]
    fn accepts_minimal_stratum_and_openai_configuration() -> Result<(), ReplError> {
        let config: Config = toml::from_str(
            r#"
[stratum]
storage_root = "./.stratum/repl"
model = "openai:gpt-4.1-mini"

[openai]
api_key = "test-key"
"#,
        )?;

        assert_eq!(config.stratum.model.as_str(), "openai:gpt-4.1-mini");
        Ok(())
    }

    #[test]
    fn rejects_unknown_stratum_configuration() {
        let result = toml::from_str::<Config>(
            r#"
[stratum]
storage_root = "./.stratum/repl"
model = "openai:gpt-4.1-mini"
unexpected = true

[openai]
api_key = "test-key"
"#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn rejects_unsupported_provider_without_network_access() -> Result<(), ReplError> {
        let config: Config = toml::from_str(
            r#"
[stratum]
storage_root = "./.stratum/repl"
model = "custom:model"
"#,
        )?;

        match select_provider(&config) {
            Err(ReplError::UnsupportedProvider { .. }) => {}
            Err(error) => panic!("unexpected provider error: {error}"),
            Ok(_) => panic!("custom providers are unsupported"),
        }
        Ok(())
    }

    #[test]
    fn scopes_agent_store_to_agent_root() -> Result<(), ReplError> {
        let agent_id = AgentId::new();

        assert_eq!(agent_root(agent_id)?.as_str(), format!("/{agent_id}"));
        Ok(())
    }
}
