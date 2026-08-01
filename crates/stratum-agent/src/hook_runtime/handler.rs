//! Ordered hook handler contract composed by [`ChainHookRuntime`].
//!
//! A handler is a first-class, individually versioned hook implementation: it
//! shares every input and decision type with [`HookRuntime`], implements only
//! the points it cares about (every method defaults to its no-op decision),
//! and exposes an immutable version identity through [`HookHandlerDescriptor`].
//! Chain semantics (ordering, short-circuiting, injection merging) live in
//! [`ChainHookRuntime`](super::ChainHookRuntime), not here.
//!
//! [`ChainHookRuntime`]: super::ChainHookRuntime

use async_trait::async_trait;
use stratum_core::{HookFailure, HookHandlerVersionId};

use super::{
    AfterToolCallDecision, AfterToolCallInput, DecideToolCallDecision, DecideToolCallInput,
    HookControl, PrepareNextTurnDecision, PrepareNextTurnInput, TransformContextDecision,
    TransformContextInput, TransformToolCallDecision, TransformToolCallInput,
};

/// Immutable identity of one hook handler.
///
/// The version id participates in the chain's extension set version: a
/// handler's decision behavior must never change without its version id
/// changing, otherwise resume-time chain verification loses its meaning.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct HookHandlerDescriptor {
    /// Immutable version identity of the handler.
    pub version_id: HookHandlerVersionId,
}

impl HookHandlerDescriptor {
    /// Creates a descriptor from the handler's immutable version identity.
    #[must_use]
    pub const fn new(version_id: HookHandlerVersionId) -> Self {
        Self { version_id }
    }
}

/// One ordered member of a hook handler chain.
///
/// The five methods mirror [`HookRuntime`] point for point and share its
/// input and decision types; unimplemented points keep their no-op default.
/// A handler observes the output view of the handlers before it in the chain
/// (see [`ChainHookRuntime`](super::ChainHookRuntime) for the exact per-point
/// chain semantics) and receives the kernel's [`HookControl`] unchanged.
///
/// Returned failures must already be safe [`HookFailure`] classifications,
/// exactly as for [`HookRuntime`]: any handler failure or invalid decision
/// fails the whole hook point closed.
#[async_trait]
pub trait HookHandler: Send + Sync {
    /// Returns the handler's immutable version identity.
    fn descriptor(&self) -> HookHandlerDescriptor;

    /// Handler-side view of [`HookRuntime::transform_context`].
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the whole hook point
    /// then fails closed and later handlers are not called.
    async fn transform_context<'a>(
        &self,
        _input: TransformContextInput<'a>,
        _control: HookControl,
    ) -> Result<TransformContextDecision, HookFailure> {
        Ok(TransformContextDecision::Unchanged)
    }

    /// Handler-side view of [`HookRuntime::transform_tool_call`].
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the whole hook point
    /// then fails closed and later handlers are not called.
    async fn transform_tool_call<'a>(
        &self,
        _input: TransformToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<TransformToolCallDecision, HookFailure> {
        Ok(TransformToolCallDecision::Continue)
    }

    /// Handler-side view of [`HookRuntime::decide_tool_call`].
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the whole hook point
    /// then fails closed and later handlers are not called.
    async fn decide_tool_call<'a>(
        &self,
        _input: DecideToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<DecideToolCallDecision, HookFailure> {
        Ok(DecideToolCallDecision::Execute)
    }

    /// Handler-side view of [`HookRuntime::after_tool_call`].
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the whole hook point
    /// then fails closed and later handlers are not called.
    async fn after_tool_call<'a>(
        &self,
        _input: AfterToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<AfterToolCallDecision, HookFailure> {
        Ok(AfterToolCallDecision::Keep)
    }

    /// Handler-side view of [`HookRuntime::prepare_next_turn`].
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the whole hook point
    /// then fails closed and later handlers are not called.
    async fn prepare_next_turn<'a>(
        &self,
        _input: PrepareNextTurnInput<'a>,
        _control: HookControl,
    ) -> Result<PrepareNextTurnDecision, HookFailure> {
        Ok(PrepareNextTurnDecision::Continue)
    }
}
