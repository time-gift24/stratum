use std::sync::Arc;

use wyse_agent::Agent;
use wyse_infra::EventStreamBus;
use wyse_llm::LlmProvider;
use wyse_tools::BuiltinToolRegistry;

use crate::DefaultAgentError;

const DEFAULT_SYSTEM_PROMPT: &str = "You are a helpful assistant.";

/// Builds the no-tool default agent with an injected provider.
///
/// # Errors
///
/// Returns an error when the supplied agent wiring is incomplete.
pub fn build_default_agent(
    event_bus: Arc<dyn EventStreamBus>,
    llm_provider: Arc<dyn LlmProvider>,
) -> Result<Agent, DefaultAgentError> {
    Ok(Agent::builder()
        .name("default-agent")
        .system_prompt(DEFAULT_SYSTEM_PROMPT)
        .llm_provider(llm_provider)
        .tool_registry(Arc::new(BuiltinToolRegistry::default()))
        .event_bus(event_bus)
        .build()?)
}
