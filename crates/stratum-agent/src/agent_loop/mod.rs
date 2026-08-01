//! Foundational types and errors for the agent loop kernel.

mod error;
mod journal;
mod resume;
mod runner;
mod stream;
mod types;

pub use error::{AgentLoopBuildError, AgentLoopError, ProtocolError, ResumeError};
pub use runner::{AgentLoop, AgentLoopBuilder};
pub use types::{HookTimeouts, LoopCompletionReason, LoopContext, LoopLimits, LoopOutcome};
