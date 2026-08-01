//! Hook runtime boundary executed by the agent loop kernel.
//!
//! The module separates the contract ([`runtime`]) from its no-op default
//! ([`noop`]), the individually versioned handler contract ([`handler`]), and
//! the ordered handler chain ([`chain`]). Typed hook failures reuse
//! `stratum-core`'s `HookFailure`; the loop-facing mapping lives in
//! [`crate::AgentLoopError`].

mod chain;
mod handler;
mod noop;
mod runtime;

pub use chain::ChainHookRuntime;
pub use handler::{HookHandler, HookHandlerDescriptor};
pub use noop::NoopHookRuntime;
pub use runtime::{
    AfterToolCallDecision, AfterToolCallInput, AuthorizationOverride, DecideToolCallDecision,
    DecideToolCallInput, HookControl, HookRuntime, HookSnapshot, PrepareNextTurnDecision,
    PrepareNextTurnInput, ToolHookTarget, TransformContextDecision, TransformContextInput,
    TransformToolCallDecision, TransformToolCallInput, TransformToolCallModification,
};
pub use stratum_core::ContextPatch;
