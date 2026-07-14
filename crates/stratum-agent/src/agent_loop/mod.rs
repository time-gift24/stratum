//! Foundational types and errors for the agent loop kernel.

mod error;
mod types;

pub use error::{AgentLoopError, LoopLimit, ProtocolError};
pub use types::{LoopContext, LoopLimits, LoopOutcome};
