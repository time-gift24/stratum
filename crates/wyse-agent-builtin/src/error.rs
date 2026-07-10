//! Error types for built-in agent wiring.

use thiserror::Error;
use wyse_agent::AgentError;

/// Error returned while wiring a default agent.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DefaultAgentError {
    /// The agent builder rejected the supplied wiring.
    #[error("failed to build default agent")]
    Agent(#[from] AgentError),
}
