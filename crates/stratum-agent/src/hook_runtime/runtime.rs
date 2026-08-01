//! Hook runtime contract executed at the agent loop's decision boundaries.
//!
//! Typed failures reuse [`HookFailure`] from `stratum-core`; the loop-facing
//! mapping lives in [`crate::AgentLoopError`].

use async_trait::async_trait;
use serde_json::Value;
use stratum_core::{
    ChatMessage, ChatRole, ContextPatch, DangerLevel, ExtensionSetVersionId, HookFailure,
    TokenUsage, ToolCall, ToolKind, ToolSpec,
};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::LoopContext;

/// Control information the agent loop enforces for every hook invocation.
///
/// The loop stops waiting for a hook when the cancellation token fires or the
/// absolute deadline passes, regardless of what the runtime itself does. A
/// hook point without a configured deadline is bounded by cancellation only.
#[derive(Debug, Clone)]
pub struct HookControl {
    cancellation: CancellationToken,
    deadline: Option<Instant>,
}

impl HookControl {
    /// Creates control information from the turn token and an optional absolute
    /// deadline.
    #[must_use]
    pub fn new(cancellation: CancellationToken, deadline: Option<Instant>) -> Self {
        Self {
            cancellation,
            deadline,
        }
    }

    /// Returns the turn cancellation token shared with the hook runtime.
    #[must_use]
    pub const fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Returns the absolute deadline after which the loop stops waiting, or
    /// `None` when this hook point is bounded by cancellation only.
    #[must_use]
    pub const fn deadline(&self) -> Option<Instant> {
        self.deadline
    }
}

/// Borrowed read-side snapshot shared by every hook input.
///
/// The kernel builds snapshots at hook boundaries with zero allocation: it
/// borrows the committed context and copies the small usage observation. The
/// snapshot is read-only; point-specific payloads (the tool call, the tool
/// target, the produced result) stay in the individual input structures and
/// never enter the snapshot.
///
/// `context` semantics are pinned per hook point:
///
/// - `transform_context`: the request view basis — committed context plus any
///   one-shot injected messages waiting to be consumed by this request.
/// - `transform_tool_call` / `decide_tool_call`: the committed context at that
///   boundary, including the current assistant message and this cycle's
///   already-committed tool results.
/// - `after_tool_call`: the same committed context, excluding the current
///   not-yet-committed result; that result only appears in the input's
///   `result` payload.
/// - `prepare_next_turn`: the committed context including all of this cycle's
///   committed results.
///
/// Read-side state shared by all hooks is added here only; the five hook
/// inputs embed the snapshot and inherit new fields unchanged. Note that
/// adding a non-`Copy` field (for example an owned tool list) will require
/// dropping the `Copy` implementation.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct HookSnapshot<'a> {
    /// Zero-based model iteration the hook boundary belongs to.
    pub iteration: u64,
    /// Borrowed committed context at this hook boundary; see the type-level
    /// docs for the exact per-point semantics.
    pub context: &'a LoopContext,
    /// Token usage reported by the most recent model response in this run up
    /// to this boundary, or `None` when no provider response has reported
    /// usage yet. The kernel passes the value through without accumulating
    /// across calls; handlers needing cumulative semantics maintain their own
    /// totals.
    pub usage: Option<TokenUsage>,
}

impl<'a> HookSnapshot<'a> {
    /// Creates a snapshot from its parts, for handler tests and compositions
    /// outside the kernel.
    #[must_use]
    pub const fn new(iteration: u64, context: &'a LoopContext, usage: Option<TokenUsage>) -> Self {
        Self {
            iteration,
            context,
            usage,
        }
    }
}

/// Borrowed input to [`HookRuntime::transform_context`].
#[derive(Debug)]
pub struct TransformContextInput<'a> {
    /// Shared read-side snapshot whose context is the request view basis
    /// (committed context plus pending one-shot injections).
    pub snapshot: HookSnapshot<'a>,
}

/// Decision returned by [`HookRuntime::transform_context`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TransformContextDecision {
    /// Use the supplied context view unchanged.
    Unchanged,
    /// Apply an incremental [`ContextPatch`] to the context of the current
    /// model request only. The kernel validates the patch against the
    /// committed messages (`upto` bounds and tool_call/tool_result pairing),
    /// rejecting invalid patches as [`HookFailure::InvalidOutput`]. A patch
    /// never writes back to the committed transcript, never becomes a durable
    /// message, and never appears in the loop outcome.
    Patch(ContextPatch),
}

/// Borrowed view of the tool one tool hook invocation applies to.
///
/// The kernel resolves the target before any tool hook runs; handlers must
/// treat it as display and decision context and must not query the tool
/// registry themselves. `None` authorization means the call is pre-authorized
/// and carries no approval metadata.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct ToolHookTarget<'a> {
    /// Effective authorization metadata (`ToolKind`, `DangerLevel`) for this
    /// call: the registry-declared default unless `transform_tool_call`
    /// overrode it. The kernel carries this value without interpreting it.
    pub authorization: Option<(ToolKind, DangerLevel)>,
    /// Provider-visible specification of the tool being called.
    pub spec: &'a ToolSpec,
}

/// Borrowed input to [`HookRuntime::transform_tool_call`].
#[derive(Debug)]
pub struct TransformToolCallInput<'a> {
    /// Shared read-side snapshot whose context is the committed context at
    /// this boundary (current assistant message plus this cycle's committed
    /// tool results).
    pub snapshot: HookSnapshot<'a>,
    /// Provider tool call authorized by a `tool_calls` finish reason, carrying
    /// the original validated arguments.
    pub tool_call: &'a ToolCall,
    /// Resolved tool target with authorization metadata and specification.
    pub tool: &'a ToolHookTarget<'a>,
}

/// Decision returned by [`HookRuntime::transform_tool_call`].
///
/// The transform phase may only continue or modify the call; it can neither
/// block the call nor change its identity. Replacement arguments are
/// re-validated by the kernel before the decide phase, and an authorization
/// override becomes the effective authorization the decide and after phases
/// observe.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TransformToolCallDecision {
    /// Keep the original arguments and the registry-declared authorization.
    Continue,
    /// Modify the arguments and/or the effective authorization of the call.
    Modify(TransformToolCallModification),
}

impl TransformToolCallDecision {
    /// Enforces the decision contract before the loop applies it.
    pub(crate) fn check(&self) -> Result<(), HookFailure> {
        match self {
            Self::Modify(modification)
                if modification.arguments.is_none() && modification.authorization.is_none() =>
            {
                Err(HookFailure::InvalidOutput)
            }
            _ => Ok(()),
        }
    }
}

/// Per-call modifications returned by [`HookRuntime::transform_tool_call`].
///
/// Fields left as `None` keep their original values; a modification with
/// every field set to `None` is invalid output (use
/// [`TransformToolCallDecision::Continue`] instead).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TransformToolCallModification {
    /// Replacement arguments; `None` keeps the original arguments.
    pub arguments: Option<Value>,
    /// Overrides the effective authorization for this call; `None` keeps the
    /// registry-declared default.
    pub authorization: Option<AuthorizationOverride>,
}

impl TransformToolCallModification {
    /// Creates a modification from optional replacement arguments and an
    /// optional authorization override; `None` fields keep the original
    /// values.
    #[must_use]
    pub const fn new(
        arguments: Option<Value>,
        authorization: Option<AuthorizationOverride>,
    ) -> Self {
        Self {
            arguments,
            authorization,
        }
    }
}

/// Per-call authorization override returned by
/// [`HookRuntime::transform_tool_call`].
///
/// Overriding authorization is the handler's explicit responsibility: the
/// kernel carries the override to the decide and after phases without any
/// sanity checks, including checks against downgrades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum AuthorizationOverride {
    /// Mark this call pre-authorized regardless of the registry declaration.
    PreAuthorize,
    /// Replace the declared authorization metadata for this call.
    Set {
        /// Replacement tool kind.
        kind: ToolKind,
        /// Replacement danger level.
        danger: DangerLevel,
    },
}

/// Borrowed input to [`HookRuntime::decide_tool_call`].
#[derive(Debug)]
pub struct DecideToolCallInput<'a> {
    /// Shared read-side snapshot whose context is the committed context at
    /// this boundary (current assistant message plus this cycle's committed
    /// tool results).
    pub snapshot: HookSnapshot<'a>,
    /// Tool call carrying the final re-validated arguments exactly as they
    /// would be executed.
    pub tool_call: &'a ToolCall,
    /// Resolved tool target with authorization metadata and specification.
    pub tool: &'a ToolHookTarget<'a>,
}

/// Decision returned by [`HookRuntime::decide_tool_call`].
///
/// The decide phase may only execute or block; it can never modify arguments,
/// so the arguments a decider (for example an approval handler) sees are
/// exactly the arguments that execute.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum DecideToolCallDecision {
    /// Commit `ToolExecutionStarted` and execute the call.
    Execute,
    /// Skip execution, answering with a structured `hook_blocked` tool result.
    /// The reason must be non-empty and safe to show the model.
    Block {
        /// Model-visible block reason.
        reason: String,
    },
}

impl DecideToolCallDecision {
    /// Enforces the decision contract before the loop applies it.
    pub(crate) fn check(&self) -> Result<(), HookFailure> {
        match self {
            Self::Block { reason } if reason.trim().is_empty() => Err(HookFailure::InvalidOutput),
            _ => Ok(()),
        }
    }
}

/// Borrowed input to [`HookRuntime::after_tool_call`].
#[derive(Debug)]
pub struct AfterToolCallInput<'a> {
    /// Shared read-side snapshot whose context is the committed context at
    /// this boundary, excluding the current not-yet-committed `result`.
    pub snapshot: HookSnapshot<'a>,
    /// Tool call as it was executed, including hook-modified arguments.
    pub tool_call: &'a ToolCall,
    /// Resolved tool target with authorization metadata and specification.
    pub tool: &'a ToolHookTarget<'a>,
    /// Model-visible tool result produced by execution or a decide-phase block.
    pub result: &'a ChatMessage,
}

/// Decision returned by [`HookRuntime::after_tool_call`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum AfterToolCallDecision {
    /// Commit the produced tool result unchanged.
    Keep,
    /// Commit a replacement JSON result under the same call identity and role.
    ReplaceResult {
        /// Replacement JSON tool result.
        result: Value,
    },
}

/// Borrowed input to [`HookRuntime::prepare_next_turn`].
#[derive(Debug)]
pub struct PrepareNextTurnInput<'a> {
    /// Shared read-side snapshot whose context is the committed context
    /// including this iteration's assistant message and all committed tool
    /// results of the cycle.
    pub snapshot: HookSnapshot<'a>,
}

/// Decision returned by [`HookRuntime::prepare_next_turn`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum PrepareNextTurnDecision {
    /// Commit the iteration boundary and start the next model iteration.
    Continue,
    /// Commit the iteration boundary and finish the loop as hook-stopped.
    Stop,
    /// Append plain user messages to the next model request view only. The
    /// messages are consumed once and never become durable agent history.
    Inject {
        /// Non-empty plain user messages for the next request view.
        messages: Vec<ChatMessage>,
    },
}

impl PrepareNextTurnDecision {
    /// Enforces the decision contract before the loop applies it.
    pub(crate) fn check(&self) -> Result<(), HookFailure> {
        match self {
            Self::Inject { messages }
                if messages.is_empty() || !messages.iter().all(is_plain_user_message) =>
            {
                Err(HookFailure::InvalidOutput)
            }
            _ => Ok(()),
        }
    }
}

fn is_plain_user_message(message: &ChatMessage) -> bool {
    message.role == ChatRole::User
        && message.tool_calls.is_empty()
        && message.reasoning_content.is_none()
        && message.tool_call_id.is_none()
}

/// Composed hook policy runtime invoked at the agent loop's decision points.
///
/// A runtime is a single already-composed strategy boundary: it does not expose
/// handler lists, sessions, journals, or event buses. The loop enforces the
/// [`HookControl`] cancellation token and optional absolute deadline around
/// every call; hook futures must therefore be cancellation-safe and must not
/// start external side effects that cannot be abandoned safely before
/// returning a decision.
///
/// Returned failures must already be safe [`HookFailure`] classifications: the
/// loop never records hook inputs, tool payloads, or internal error text.
#[async_trait]
pub trait HookRuntime: Send + Sync {
    /// Transforms the context view of the current model request.
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the affected model
    /// request is then skipped and the loop fails closed.
    async fn transform_context<'a>(
        &self,
        input: TransformContextInput<'a>,
        control: HookControl,
    ) -> Result<TransformContextDecision, HookFailure>;

    /// Transforms the arguments and optionally the effective authorization of
    /// one authorized tool call.
    ///
    /// Runs after the original arguments validate and before the final
    /// re-validation and the decide phase. The kernel carries an authorization
    /// override to the later phases without interpreting it.
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the affected tool call is
    /// then neither decided nor executed and the loop fails closed.
    async fn transform_tool_call<'a>(
        &self,
        input: TransformToolCallInput<'a>,
        control: HookControl,
    ) -> Result<TransformToolCallDecision, HookFailure>;

    /// Decides whether one re-validated tool call executes.
    ///
    /// Runs after the final argument re-validation and before
    /// `ToolExecutionStarted`. Interactive approval is an ordinary handler at
    /// this phase; the point has no default deadline and is bounded by
    /// cancellation only unless configured otherwise.
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the affected tool call is
    /// then not executed and the loop fails closed.
    async fn decide_tool_call<'a>(
        &self,
        input: DecideToolCallInput<'a>,
        control: HookControl,
    ) -> Result<DecideToolCallDecision, HookFailure>;

    /// Processes one model-visible tool result before it is committed.
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the untransformed tool
    /// result is then not committed and the loop fails closed.
    async fn after_tool_call<'a>(
        &self,
        input: AfterToolCallInput<'a>,
        control: HookControl,
    ) -> Result<AfterToolCallDecision, HookFailure>;

    /// Controls the next model iteration after a committed tool cycle.
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the next iteration then
    /// does not start and the loop fails closed.
    async fn prepare_next_turn<'a>(
        &self,
        input: PrepareNextTurnInput<'a>,
        control: HookControl,
    ) -> Result<PrepareNextTurnDecision, HookFailure>;

    /// Reports the immutable ordered extension set version this runtime pins,
    /// or `None` when the runtime is not a versioned handler chain.
    ///
    /// The loop durably commits a reported version with `LoopStarted`; resume
    /// compares the recorded version against the version the re-injected
    /// runtime reports and fails closed on a mismatch. The default `None`
    /// marks the runtime as unpinned and skips the check.
    #[must_use]
    fn extension_set_version(&self) -> Option<ExtensionSetVersionId> {
        None
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stratum_core::CallId;

    use super::*;

    #[test]
    fn block_decisions_require_a_non_empty_reason() {
        for reason in ["", "   "] {
            let decision = DecideToolCallDecision::Block {
                reason: reason.to_owned(),
            };
            assert_eq!(decision.check(), Err(HookFailure::InvalidOutput));
        }
        let decision = DecideToolCallDecision::Block {
            reason: "policy denied".to_owned(),
        };
        assert_eq!(decision.check(), Ok(()));
        assert_eq!(DecideToolCallDecision::Execute.check(), Ok(()));
    }

    #[test]
    fn modify_decisions_require_at_least_one_change() {
        let no_change = TransformToolCallDecision::Modify(TransformToolCallModification {
            arguments: None,
            authorization: None,
        });
        assert_eq!(no_change.check(), Err(HookFailure::InvalidOutput));

        let arguments_only = TransformToolCallDecision::Modify(TransformToolCallModification {
            arguments: Some(json!({"value": 1})),
            authorization: None,
        });
        assert_eq!(arguments_only.check(), Ok(()));
        let authorization_only = TransformToolCallDecision::Modify(TransformToolCallModification {
            arguments: None,
            authorization: Some(AuthorizationOverride::PreAuthorize),
        });
        assert_eq!(authorization_only.check(), Ok(()));
        assert_eq!(TransformToolCallDecision::Continue.check(), Ok(()));
    }

    #[test]
    fn inject_decisions_require_plain_user_messages() {
        let valid = ChatMessage::user("hook note");
        let cases: Vec<(Vec<ChatMessage>, bool)> = vec![
            (vec![], false),
            (vec![valid.clone()], true),
            (vec![ChatMessage::assistant("forged")], false),
            (vec![ChatMessage::system("forged")], false),
            (
                vec![ChatMessage::tool(CallId::from("call-1"), json!({}))],
                false,
            ),
            (
                vec![ChatMessage::user("note").with_reasoning_content("forged")],
                false,
            ),
            (
                vec![ChatMessage::user("note").with_tool_calls(vec![ToolCall {
                    call_id: CallId::from("call-1"),
                    name: "echo".to_owned(),
                    arguments: json!({}),
                }])],
                false,
            ),
            (
                vec![{
                    let mut message = ChatMessage::user("note");
                    message.tool_call_id = Some(CallId::from("call-1"));
                    message
                }],
                false,
            ),
            (vec![valid.clone(), ChatMessage::assistant("forged")], false),
        ];
        for (messages, expected) in cases {
            let decision = PrepareNextTurnDecision::Inject { messages };
            assert_eq!(decision.check().is_ok(), expected);
        }
        let decision = PrepareNextTurnDecision::Continue;
        assert!(decision.check().is_ok());
        let decision = PrepareNextTurnDecision::Stop;
        assert!(decision.check().is_ok());
    }

    #[test]
    fn hook_control_exposes_the_shared_token_and_deadline() {
        let cancellation = CancellationToken::new();
        let deadline = Instant::now();
        let control = HookControl::new(cancellation.clone(), Some(deadline));

        assert!(!control.cancellation().is_cancelled());
        assert_eq!(control.deadline(), Some(deadline));
        cancellation.cancel();
        assert!(control.cancellation().is_cancelled());

        let control = HookControl::new(CancellationToken::new(), None);
        assert_eq!(control.deadline(), None);
    }

    #[test]
    fn snapshot_is_the_shared_envelope_of_all_five_inputs() {
        let context = LoopContext::new("be precise").with_messages(vec![ChatMessage::user("hi")]);
        let usage = TokenUsage {
            input_tokens: 3,
            output_tokens: 2,
            total_tokens: 5,
        };
        let snapshot = HookSnapshot {
            iteration: 4,
            context: &context,
            usage: Some(usage),
        };
        // `Copy` lets handlers pass the snapshot on without borrow constraints.
        let copied = snapshot;

        let tool_call = ToolCall {
            call_id: CallId::from("call-1"),
            name: "echo".to_owned(),
            arguments: json!({}),
        };
        let spec = ToolSpec::builder()
            .name("echo")
            .description("records calls")
            .input_schema(json!({"type": "object"}))
            .build();
        let target = ToolHookTarget {
            authorization: None,
            spec: &spec,
        };
        let result = ChatMessage::tool(tool_call.call_id.clone(), json!({"ok": true}));

        // One snapshot value constructs every hook input unchanged: a new
        // shared field on `HookSnapshot` is inherited by all five inputs.
        let transform = TransformContextInput { snapshot };
        let transform_tool = TransformToolCallInput {
            snapshot,
            tool_call: &tool_call,
            tool: &target,
        };
        let decide = DecideToolCallInput {
            snapshot,
            tool_call: &tool_call,
            tool: &target,
        };
        let after = AfterToolCallInput {
            snapshot,
            tool_call: &tool_call,
            tool: &target,
            result: &result,
        };
        let prepare = PrepareNextTurnInput { snapshot };

        let embedded = [
            transform.snapshot,
            transform_tool.snapshot,
            decide.snapshot,
            after.snapshot,
            prepare.snapshot,
        ];
        for snapshot in embedded {
            assert_eq!(snapshot.iteration, copied.iteration);
            assert_eq!(snapshot.usage, copied.usage);
            assert!(std::ptr::eq(snapshot.context, copied.context));
        }
    }
}
