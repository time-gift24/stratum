//! Public agent runtime definitions.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use tokio_util::sync::CancellationToken;
use wyse_core::{AgentId, ChatMessage, ChatRole, ModelId, RunId};
use wyse_infra::event_stream_bus::{EventStream, EventStreamBus};
use wyse_llm::LlmProvider;
use wyse_tools::ToolRegistry;

use crate::AgentError;

/// Runtime tuning for an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    /// Maximum LLM turns in one run.
    pub max_turns: usize,
    /// Maximum tool calls accepted from one assistant turn.
    pub max_tool_calls_per_turn: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 16,
            max_tool_calls_per_turn: 16,
        }
    }
}

/// Stream handle returned by [`Agent::stream`].
pub struct AgentStream {
    /// Run identity for this stream.
    pub run_id: RunId,
    /// Live event stream for the run.
    pub events: EventStream,
    /// Cancellation handle for this run.
    pub cancel: CancellationToken,
}

/// Stateful agent that owns conversation history.
pub struct Agent {
    id: AgentId,
    name: String,
    system_prompt: String,
    llm_provider: Arc<dyn LlmProvider>,
    model: ModelId,
    tool_registry: Arc<dyn ToolRegistry>,
    event_bus: Arc<dyn EventStreamBus>,
    config: AgentConfig,
    history: Arc<Mutex<Vec<ChatMessage>>>,
    active: Arc<AtomicBool>,
}

impl Agent {
    /// Creates an agent builder.
    #[must_use]
    pub fn builder() -> AgentBuilder {
        AgentBuilder::default()
    }

    /// Starts streaming one user message through the agent.
    ///
    /// # Errors
    ///
    /// Returns an error if the input message role is not `User`, another run is
    /// active, or subscribing to the event bus fails.
    pub async fn stream(&self, message: ChatMessage) -> Result<AgentStream, AgentError> {
        if message.role != ChatRole::User {
            return Err(AgentError::InvalidInputMessageRole { role: message.role });
        }

        if self.active.swap(true, Ordering::SeqCst) {
            return Err(AgentError::RunAlreadyActive);
        }

        let run_id = RunId::new();
        let events = self.event_bus.subscribe_run(run_id).await?;
        let cancel = CancellationToken::new();
        self.active.store(false, Ordering::SeqCst);

        Ok(AgentStream {
            run_id,
            events,
            cancel,
        })
    }
}

/// Builder for [`Agent`].
#[derive(Default)]
pub struct AgentBuilder {
    id: Option<AgentId>,
    name: Option<String>,
    system_prompt: Option<String>,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    model: Option<ModelId>,
    tool_registry: Option<Arc<dyn ToolRegistry>>,
    event_bus: Option<Arc<dyn EventStreamBus>>,
    config: Option<AgentConfig>,
}

impl AgentBuilder {
    /// Sets the agent id.
    #[must_use]
    pub fn id(mut self, id: AgentId) -> Self {
        self.id = Some(id);
        self
    }

    /// Sets the agent name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the system prompt.
    #[must_use]
    pub fn system_prompt(mut self, system_prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(system_prompt.into());
        self
    }

    /// Sets the LLM provider.
    #[must_use]
    pub fn llm_provider(mut self, llm_provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(llm_provider);
        self
    }

    /// Sets the model id.
    #[must_use]
    pub fn model(mut self, model: ModelId) -> Self {
        self.model = Some(model);
        self
    }

    /// Sets the tool registry.
    #[must_use]
    pub fn tool_registry(mut self, tool_registry: Arc<dyn ToolRegistry>) -> Self {
        self.tool_registry = Some(tool_registry);
        self
    }

    /// Sets the event bus.
    #[must_use]
    pub fn event_bus(mut self, event_bus: Arc<dyn EventStreamBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Sets runtime config.
    #[must_use]
    pub fn config(mut self, config: AgentConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Builds an [`Agent`].
    ///
    /// # Errors
    ///
    /// Returns an error when a required builder field is missing.
    pub fn build(self) -> Result<Agent, AgentError> {
        Ok(Agent {
            id: self.id.unwrap_or_default(),
            name: self
                .name
                .ok_or(AgentError::MissingBuilderField { field: "name" })?,
            system_prompt: self.system_prompt.ok_or(AgentError::MissingBuilderField {
                field: "system_prompt",
            })?,
            llm_provider: self.llm_provider.ok_or(AgentError::MissingBuilderField {
                field: "llm_provider",
            })?,
            model: self
                .model
                .ok_or(AgentError::MissingBuilderField { field: "model" })?,
            tool_registry: self.tool_registry.ok_or(AgentError::MissingBuilderField {
                field: "tool_registry",
            })?,
            event_bus: self
                .event_bus
                .ok_or(AgentError::MissingBuilderField { field: "event_bus" })?,
            config: self.config.unwrap_or_default(),
            history: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wyse_core::{ChatMessage, ModelId};
    use wyse_infra::event_stream_bus::InMemoryEventStreamBus;
    use wyse_llm::MockLlmProvider;
    use wyse_tools::BuiltinToolRegistry;

    use super::*;

    fn test_agent() -> Agent {
        Agent::builder()
            .name("test-agent")
            .system_prompt("be helpful")
            .llm_provider(Arc::new(MockLlmProvider::new()))
            .model(ModelId::from("mock-model"))
            .tool_registry(Arc::new(BuiltinToolRegistry::default()))
            .event_bus(Arc::new(InMemoryEventStreamBus::default()))
            .build()
            .expect("agent should build")
    }

    #[tokio::test]
    async fn stream_rejects_non_user_message() {
        let agent = test_agent();

        let error = match agent.stream(ChatMessage::assistant("nope")).await {
            Ok(_) => panic!("assistant input should be rejected"),
            Err(error) => error,
        };

        assert!(matches!(error, AgentError::InvalidInputMessageRole { .. }));
    }
}
