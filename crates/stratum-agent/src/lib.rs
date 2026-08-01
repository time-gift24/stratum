//! Agent runtime loop for Stratum.

pub mod agent_loop;
pub mod definition;
pub mod error;
pub mod hook_runtime;
pub mod tool_executor;

pub(crate) mod r#loop;

pub use agent_loop::{
    AgentLoop, AgentLoopBuildError, AgentLoopBuilder, AgentLoopError, HookTimeouts,
    LoopCompletionReason, LoopContext, LoopLimits, LoopOutcome, ProtocolError,
};
pub use definition::{Agent, AgentBuilder, AgentConfig};
pub use error::AgentError;
pub use hook_runtime::{
    AfterToolCallDecision, AfterToolCallInput, AuthorizationOverride, DecideToolCallDecision,
    DecideToolCallInput, HookControl, HookRuntime, HookSnapshot, NoopHookRuntime,
    PrepareNextTurnDecision, PrepareNextTurnInput, ToolHookTarget, TransformContextDecision,
    TransformContextInput, TransformToolCallDecision, TransformToolCallInput,
    TransformToolCallModification,
};
pub use tool_executor::{ToolExecutor, ToolExecutorError};
