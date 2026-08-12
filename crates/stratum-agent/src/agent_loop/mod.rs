//! Foundational types and errors for the agent loop kernel.

mod compaction;
mod error;
mod journal;
mod resume;
mod runner;
mod stream;
mod types;

pub use compaction::COMPACTION_MARKER_PREFIX;
pub use error::{AgentLoopBuildError, AgentLoopError, ProtocolError, ResumeError};
pub use runner::{AgentLoop, AgentLoopBuilder, PreparedResume};
pub use types::{HookTimeouts, LoopCompletionReason, LoopContext, LoopLimits, LoopOutcome};

pub(crate) use runner::{apply_context_patch, validate_context_patch};
