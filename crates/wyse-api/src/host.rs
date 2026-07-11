//! Hosted agent registry and startup recovery.

use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use wyse_agent::Agent;
use wyse_config::{Config, ConfigError, ResolvedAgentDefinition};
use wyse_core::{AgentId, DangerLevel, ToolKind};
use wyse_filesystem::{FileType, Filesystem, VirtualPath};
use wyse_infra::EventStreamBus;
use wyse_llm::LlmProviderManager;
use wyse_store::{AgentStatus, AgentStore, FilesystemAgentStore, StoreEventStreamBus};
use wyse_tools::{BuiltinToolRegistry, EchoTool, ToolPermissionMode, ToolRegistry};

use crate::HostError;

const HISTORY_ROOT: &str = "/history";
const DEFINITION_FILE: &str = "definition.toml";

/// Shared runtime state for all recovered agents.
pub struct HostState {
    agents: RwLock<HashMap<AgentId, Arc<HostedAgent>>>,
    #[allow(
        dead_code,
        reason = "retained for the next API endpoint assembly tasks"
    )]
    filesystem: Arc<dyn Filesystem>,
    #[allow(
        dead_code,
        reason = "retained for the next API endpoint assembly tasks"
    )]
    event_bus: Arc<dyn EventStreamBus>,
    #[allow(
        dead_code,
        reason = "retained for the next API endpoint assembly tasks"
    )]
    providers: Arc<LlmProviderManager>,
    #[allow(
        dead_code,
        reason = "retained for the next API endpoint assembly tasks"
    )]
    config: Arc<Config>,
}

/// One composed agent and its durable store.
pub struct HostedAgent {
    /// Agent runtime.
    pub agent: Agent,
    /// Durable state and complete message history.
    pub store: Arc<dyn AgentStore>,
    needs_resume: AtomicBool,
}

impl HostedAgent {
    /// Returns whether startup found an interrupted running turn.
    #[must_use]
    pub fn needs_resume(&self) -> bool {
        self.needs_resume.load(Ordering::Acquire)
    }
}

impl HostState {
    /// Restores every persisted agent under `history/`.
    ///
    /// # Errors
    ///
    /// Returns [`HostError`] when any directory, definition, provider, tool, store, or
    /// complete history is invalid. No partial registry is returned.
    pub async fn restore(
        config: Config,
        filesystem: Arc<dyn Filesystem>,
        event_bus: Arc<dyn EventStreamBus>,
        providers: LlmProviderManager,
    ) -> Result<Arc<Self>, HostError> {
        let history_root = VirtualPath::try_from(HISTORY_ROOT).map_err(|source| {
            wyse_filesystem::FilesystemError::InvalidVirtualPath {
                path: HISTORY_ROOT.to_owned(),
                source,
            }
        })?;
        let entries = filesystem.list_dir(&history_root).await?;
        let mut agents = HashMap::with_capacity(entries.len());

        for entry in entries {
            let agent_id = parse_history_entry(&entry)?;
            let root = agent_root(agent_id)?;
            let definition_path = child_path(&root, DEFINITION_FILE)?;
            let bytes = filesystem.read_file(&definition_path).await?;
            let input = std::str::from_utf8(&bytes)
                .map_err(|source| HostError::InvalidDefinitionEncoding { source })?;
            let definition = ResolvedAgentDefinition::parse(input)?;
            validate_definition_model(&config, &definition)?;
            let store: Arc<dyn AgentStore> =
                Arc::new(FilesystemAgentStore::new(Arc::clone(&filesystem), root));
            let state = store.load_agent().await?;
            let expected_name = definition.agent_name.as_str();
            if state.agent_id != agent_id || state.name != expected_name {
                return Err(HostError::IdentityMismatch {
                    expected_id: agent_id,
                    actual_id: state.agent_id,
                    expected_name: expected_name.to_owned(),
                    actual_name: state.name,
                });
            }

            let provider = providers.get(&definition.model)?;
            let registry = tool_registry(&definition)?;
            let agent_bus: Arc<dyn EventStreamBus> = Arc::new(StoreEventStreamBus::new(
                Arc::clone(&store),
                Arc::clone(&event_bus),
            ));
            let agent = Agent::builder()
                .id(agent_id)
                .name(expected_name)
                .system_prompt(definition.prompt)
                .llm_provider(provider)
                .tool_registry(registry)
                .event_bus(agent_bus)
                .store(Arc::clone(&store))
                .build()?;
            let needs_resume = state.status == AgentStatus::Running;
            if !needs_resume {
                agent.load_history().await?;
            }
            agents.insert(
                agent_id,
                Arc::new(HostedAgent {
                    agent,
                    store,
                    needs_resume: AtomicBool::new(needs_resume),
                }),
            );
        }

        Ok(Arc::new(Self {
            agents: RwLock::new(agents),
            filesystem,
            event_bus,
            providers: Arc::new(providers),
            config: Arc::new(config),
        }))
    }

    /// Returns a hosted agent without performing I/O.
    #[must_use]
    pub fn agent(&self, agent_id: AgentId) -> Option<Arc<HostedAgent>> {
        self.agents
            .read()
            .expect("host registry lock should not be poisoned")
            .get(&agent_id)
            .map(Arc::clone)
    }
}

fn parse_history_entry(entry: &wyse_filesystem::DirEntry) -> Result<AgentId, HostError> {
    let agent_id = entry
        .file_name
        .parse::<AgentId>()
        .ok()
        .filter(|id| id.as_uuid().get_version_num() == 7)
        .filter(|id| id.to_string() == entry.file_name)
        .ok_or_else(|| HostError::InvalidHistoryDirectory {
            name: entry.file_name.clone(),
        })?;
    if entry.file_type != FileType::Directory || entry.path != agent_root(agent_id)? {
        return Err(HostError::InvalidHistoryDirectory {
            name: entry.file_name.clone(),
        });
    }
    Ok(agent_id)
}

fn agent_root(agent_id: AgentId) -> Result<VirtualPath, HostError> {
    let path = format!("{HISTORY_ROOT}/{agent_id}");
    VirtualPath::try_from(path.as_str()).map_err(|source| {
        wyse_filesystem::FilesystemError::InvalidVirtualPath { path, source }.into()
    })
}

fn child_path(root: &VirtualPath, child: &str) -> Result<VirtualPath, HostError> {
    let path = format!("{}/{child}", root.as_str());
    VirtualPath::try_from(path.as_str()).map_err(|source| {
        wyse_filesystem::FilesystemError::InvalidVirtualPath { path, source }.into()
    })
}

fn tool_registry(definition: &ResolvedAgentDefinition) -> Result<Arc<dyn ToolRegistry>, HostError> {
    let mut registry = BuiltinToolRegistry::new(ToolPermissionMode::RequireApproval);
    for name in &definition.tools {
        if name.as_str() != "echo" {
            return Err(HostError::ToolNotAvailable { name: name.clone() });
        }
        registry.register(Arc::new(EchoTool::new()), ToolKind::Read, DangerLevel::Low)?;
    }
    Ok(Arc::new(registry))
}

fn validate_definition_model(
    config: &Config,
    definition: &ResolvedAgentDefinition,
) -> Result<(), HostError> {
    let provider = match definition.model.provider_name() {
        "deepseek" => config.llm.deepseek.as_ref(),
        "openai" => config.llm.openai.as_ref(),
        _ => None,
    };
    if provider.is_some_and(|provider| {
        provider
            .models
            .iter()
            .any(|model| model == definition.model.model_name())
    }) {
        return Ok(());
    }
    Err(ConfigError::ModelNotConfigured {
        model: definition.model.clone(),
    }
    .into())
}
