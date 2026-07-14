//! Agent runtime loop for Stratum.

pub mod agent_loop;
pub mod definition;
pub mod error;

pub(crate) mod r#loop;

pub use agent_loop::{
    AgentLoopError, LoopContext, LoopLimit, LoopLimits, LoopOutcome, ProtocolError,
};
pub use definition::{Agent, AgentBuilder, AgentConfig};
pub use error::AgentError;
