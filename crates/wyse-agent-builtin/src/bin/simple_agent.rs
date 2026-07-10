use std::{io::Write, sync::Arc};

use futures_util::StreamExt;
use wyse_agent::AgentError;
use wyse_agent_builtin::{DefaultAgentError, build_default_agent};
use wyse_core::{AgentEvent, ChatMessage, ModelId, ModelIdParseError, RuntimeEvent};
use wyse_infra::{
    EventStreamBus,
    event_stream_bus::{EventStreamBusError, InMemoryEventStreamBus},
};
use wyse_llm::ApiKey;

#[derive(Debug, thiserror::Error)]
enum SimpleAgentError {
    #[error("missing environment variable: {name}")]
    MissingEnvironment { name: &'static str },
    #[error("environment variable is not valid unicode")]
    InvalidEnvironment(#[source] std::env::VarError),
    #[error("missing prompt argument")]
    MissingPrompt,
    #[error("expected exactly one prompt argument")]
    TooManyArguments,
    #[error("invalid model id")]
    ModelId(#[from] ModelIdParseError),
    #[error("default agent setup failed")]
    DefaultAgent(#[from] DefaultAgentError),
    #[error("agent start failed")]
    AgentStart(#[from] AgentError),
    #[error("event stream failed")]
    EventStream(#[from] EventStreamBusError),
    #[error("failed to encode event")]
    Encode(#[from] serde_json::Error),
    #[error("failed to write event")]
    Write(#[from] std::io::Error),
    #[error("agent run failed")]
    AgentFailed,
    #[error("agent run cancelled")]
    AgentCancelled,
    #[error("event stream closed before the agent finished")]
    StreamClosed,
}

fn required_environment(name: &'static str) -> Result<String, SimpleAgentError> {
    match std::env::var(name) {
        Ok(value) => Ok(value),
        Err(std::env::VarError::NotPresent) => Err(SimpleAgentError::MissingEnvironment { name }),
        Err(error) => Err(SimpleAgentError::InvalidEnvironment(error)),
    }
}

fn prompt_from_args(mut args: impl Iterator<Item = String>) -> Result<String, SimpleAgentError> {
    let prompt = args.next().ok_or(SimpleAgentError::MissingPrompt)?;
    if args.next().is_some() {
        return Err(SimpleAgentError::TooManyArguments);
    }
    Ok(prompt)
}

#[tokio::main]
async fn main() -> Result<(), SimpleAgentError> {
    let api_key = ApiKey::new(required_environment("API_KEY")?);
    let model: ModelId = required_environment("MODEL")?.parse()?;
    let prompt = prompt_from_args(std::env::args().skip(1))?;
    let bus = Arc::new(InMemoryEventStreamBus::default());
    let event_bus: Arc<dyn EventStreamBus> = bus.clone();
    let agent = build_default_agent(event_bus, api_key, &model)?;
    let run_id = agent.run_turn(ChatMessage::user(prompt)).await?;
    let mut stream = bus.subscribe_run(run_id).await?;
    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();

    while let Some(envelope) = stream.next().await {
        let envelope = envelope?;
        serde_json::to_writer(&mut stdout, &envelope)?;
        writeln!(stdout)?;
        stdout.flush()?;

        match &envelope.event {
            RuntimeEvent::Agent {
                event: AgentEvent::Finished { .. },
                ..
            } => return Ok(()),
            RuntimeEvent::Agent {
                event: AgentEvent::Failed { .. },
                ..
            } => return Err(SimpleAgentError::AgentFailed),
            RuntimeEvent::Agent {
                event: AgentEvent::Cancelled,
                ..
            } => return Err(SimpleAgentError::AgentCancelled),
            _ => {}
        }
    }

    Err(SimpleAgentError::StreamClosed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_from_args_requires_exactly_one_argument() {
        assert!(matches!(
            prompt_from_args(Vec::<String>::new().into_iter()),
            Err(SimpleAgentError::MissingPrompt)
        ));
        assert!(matches!(
            prompt_from_args(["one".to_owned(), "two".to_owned()].into_iter()),
            Err(SimpleAgentError::TooManyArguments)
        ));
    }
}
