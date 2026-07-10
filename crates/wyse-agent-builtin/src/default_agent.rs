use std::sync::Arc;

use wyse_agent::{Agent, AgentError};
use wyse_checkpoint::AgentCheckpoint;
use wyse_core::AgentId;
use wyse_infra::EventStreamBus;
use wyse_llm::LlmProvider;
use wyse_tools::BuiltinToolRegistry;

const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful assistant.";

/// Builds the no-tool default agent with an injected provider.
///
/// # Errors
///
/// Returns an error when the supplied agent wiring is incomplete.
pub fn build_default_agent(
    agent_id: AgentId,
    checkpoint: Arc<dyn AgentCheckpoint>,
    event_bus: Arc<dyn EventStreamBus>,
    llm_provider: Arc<dyn LlmProvider>,
) -> Result<Agent, AgentError> {
    Agent::builder()
        .id(agent_id)
        .name("default-agent")
        .system_prompt(DEFAULT_SYSTEM_PROMPT)
        .llm_provider(llm_provider)
        .tool_registry(Arc::new(BuiltinToolRegistry::default()))
        .event_bus(event_bus)
        .checkpoint(checkpoint)
        .build()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wyse_agent::AgentError;
    use wyse_checkpoint::FilesystemAgentCheckpoint;
    use wyse_core::AgentId;
    use wyse_filesystem::{Filesystem, LocalFilesystem, LocalFilesystemConfig, VirtualPath};
    use wyse_infra::event_stream_bus::InMemoryEventStreamBus;
    use wyse_llm::MockLlmProvider;

    use super::build_default_agent;

    #[test]
    fn build_default_agent_returns_agent_error() {
        let filesystem: Arc<dyn Filesystem> = Arc::new(
            LocalFilesystem::new(LocalFilesystemConfig {
                root: std::env::current_dir().expect("current directory"),
                max_file_bytes: None,
            })
            .expect("local filesystem"),
        );
        let checkpoint = Arc::new(FilesystemAgentCheckpoint::new(
            filesystem,
            VirtualPath::try_from("/").expect("root path"),
        ));
        let result: Result<_, AgentError> = build_default_agent(
            AgentId::new(),
            checkpoint,
            Arc::new(InMemoryEventStreamBus::default()),
            Arc::new(MockLlmProvider::new()),
        );

        assert!(result.is_ok());
    }
}
