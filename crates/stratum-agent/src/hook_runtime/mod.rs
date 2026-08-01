//! Hook runtime boundary executed by the agent loop kernel.
//!
//! The module separates the contract ([`runtime`]) from its no-op default
//! ([`noop`]). Typed hook failures reuse `stratum-core`'s `HookFailure`; the
//! loop-facing mapping lives in [`crate::AgentLoopError`].

mod noop;
mod runtime;

pub use noop::NoopHookRuntime;
pub use runtime::{
    AfterToolCallDecision, AfterToolCallInput, AuthorizationOverride, DecideToolCallDecision,
    DecideToolCallInput, HookControl, HookRuntime, HookSnapshot, PrepareNextTurnDecision,
    PrepareNextTurnInput, ToolHookTarget, TransformContextDecision, TransformContextInput,
    TransformToolCallDecision, TransformToolCallInput, TransformToolCallModification,
};
pub use stratum_core::ContextPatch;
