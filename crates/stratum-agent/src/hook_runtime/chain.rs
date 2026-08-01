//! Ordered hook handler chain executed as one [`HookRuntime`].
//!
//! [`ChainHookRuntime`] is the composition boundary the kernel sees: it holds
//! an ordered `Vec<Arc<dyn HookHandler>>` fixed at construction and implements
//! the five hook points with chain semantics, so the kernel's cancellation,
//! deadline, and journal machinery applies unchanged to the chain as a whole.
//!
//! Per-point chain semantics:
//!
//! - `transform_context`, `transform_tool_call`, `after_tool_call` thread a
//!   view through the handlers in order: each handler observes the previous
//!   handler's output (`Cow`, so a chain that modifies nothing never copies).
//! - `decide_tool_call` returns the first `Block`, short-circuiting the rest
//!   of the chain.
//! - `prepare_next_turn` short-circuits on the first `Stop` (discarding
//!   injections collected so far — a stopped loop has no next turn) and merges
//!   multiple `Inject` decisions into one, in handler order.
//! - Any handler failure or invalid decision fails the whole hook point
//!   closed; later handlers are not called.
//!
//! Every handler receives the kernel's [`HookControl`] unchanged; the chain
//! adds no timeout concept of its own.
//!
//! The chain's extension set version is fixed at construction: the SHA-256 of
//! the ordered handler version ids, folded into a deterministic UUIDv8-layout
//! [`ExtensionSetVersionId`] (first 16 digest bytes, RFC 9562 variant and
//! version bits). Rebuilding the same handlers in the same order reproduces
//! the version; any membership, order, or handler version change alters it.
//! The kernel commits the version with `LoopStarted` and refuses a resume
//! whose re-injected chain reports a different version.

use std::{borrow::Cow, fmt, sync::Arc};

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use stratum_core::{ChatMessage, ContextPatch, ExtensionSetVersionId, HookFailure};

use super::{
    AfterToolCallDecision, AfterToolCallInput, AuthorizationOverride, DecideToolCallDecision,
    DecideToolCallInput, HookControl, HookHandler, HookRuntime, HookSnapshot,
    PrepareNextTurnDecision, PrepareNextTurnInput, ToolHookTarget, TransformContextDecision,
    TransformContextInput, TransformToolCallDecision, TransformToolCallInput,
    TransformToolCallModification,
};
use crate::LoopContext;
use crate::agent_loop::{apply_context_patch, validate_context_patch};

/// Ordered hook handler chain composed behind the single [`HookRuntime`]
/// boundary; see the module documentation for the per-point chain semantics.
pub struct ChainHookRuntime {
    handlers: Vec<Arc<dyn HookHandler>>,
    extension_set_version: ExtensionSetVersionId,
}

impl ChainHookRuntime {
    /// Fixes the handler order and computes the chain's extension set version
    /// from the ordered handler version ids. An empty chain is valid and
    /// behaves like [`super::NoopHookRuntime`] with a fixed version.
    #[must_use]
    pub fn new(handlers: Vec<Arc<dyn HookHandler>>) -> Self {
        Self {
            extension_set_version: chain_version(&handlers),
            handlers,
        }
    }
}

impl fmt::Debug for ChainHookRuntime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChainHookRuntime")
            .field("handler_count", &self.handlers.len())
            .field("extension_set_version", &self.extension_set_version)
            .finish()
    }
}

#[async_trait]
impl HookRuntime for ChainHookRuntime {
    async fn transform_context<'a>(
        &self,
        input: TransformContextInput<'a>,
        control: HookControl,
    ) -> Result<TransformContextDecision, HookFailure> {
        let mut view: Cow<'_, LoopContext> = Cow::Borrowed(input.snapshot.context);
        let mut patches: Vec<ContextPatch> = Vec::new();
        for handler in &self.handlers {
            let snapshot = HookSnapshot {
                context: &view,
                ..input.snapshot
            };
            let decision = handler
                .transform_context(TransformContextInput { snapshot }, control.clone())
                .await?;
            if let TransformContextDecision::Patch(patch) = decision {
                // Each patch must apply cleanly to the view the next handler
                // observes; the kernel re-validates the composed patch against
                // the committed transcript before applying it.
                validate_context_patch(&view.messages, &patch)?;
                apply_context_patch(view.to_mut(), &patch);
                patches.push(patch);
            }
        }
        Ok(match patches.len() {
            0 => TransformContextDecision::Unchanged,
            // A single-handler chain reports its patch unwrapped.
            1 => TransformContextDecision::Patch(patches.pop().expect("one patch collected")),
            _ => TransformContextDecision::Patch(ContextPatch::Composite(patches)),
        })
    }

    async fn transform_tool_call<'a>(
        &self,
        input: TransformToolCallInput<'a>,
        control: HookControl,
    ) -> Result<TransformToolCallDecision, HookFailure> {
        let mut current: Cow<'_, stratum_core::ToolCall> = Cow::Borrowed(input.tool_call);
        let original_authorization = input.tool.authorization;
        let mut authorization = original_authorization;
        let mut arguments_modified = false;
        for handler in &self.handlers {
            let target = ToolHookTarget {
                authorization,
                spec: input.tool.spec,
            };
            let decision = handler
                .transform_tool_call(
                    TransformToolCallInput {
                        snapshot: input.snapshot,
                        tool_call: &current,
                        tool: &target,
                    },
                    control.clone(),
                )
                .await?;
            decision.check()?;
            if let TransformToolCallDecision::Modify(modification) = decision {
                if let Some(arguments) = modification.arguments {
                    current.to_mut().arguments = arguments;
                    arguments_modified = true;
                }
                authorization = match modification.authorization {
                    None => authorization,
                    Some(AuthorizationOverride::PreAuthorize) => None,
                    Some(AuthorizationOverride::Set { kind, danger }) => Some((kind, danger)),
                };
            }
        }
        // The kernel-facing decision carries the net change relative to the
        // original call: no handler modification at all collapses to Continue.
        let arguments = arguments_modified.then(|| current.arguments.clone());
        let authorization_override =
            (authorization != original_authorization).then_some(match authorization {
                None => AuthorizationOverride::PreAuthorize,
                Some((kind, danger)) => AuthorizationOverride::Set { kind, danger },
            });
        if arguments.is_none() && authorization_override.is_none() {
            return Ok(TransformToolCallDecision::Continue);
        }
        Ok(TransformToolCallDecision::Modify(
            TransformToolCallModification::new(arguments, authorization_override),
        ))
    }

    async fn decide_tool_call<'a>(
        &self,
        input: DecideToolCallInput<'a>,
        control: HookControl,
    ) -> Result<DecideToolCallDecision, HookFailure> {
        for handler in &self.handlers {
            let decision = handler
                .decide_tool_call(
                    DecideToolCallInput {
                        snapshot: input.snapshot,
                        tool_call: input.tool_call,
                        tool: input.tool,
                    },
                    control.clone(),
                )
                .await?;
            decision.check()?;
            // The first block settles the call; approval-style handlers may
            // have side effects, so later handlers are never asked.
            if matches!(decision, DecideToolCallDecision::Block { .. }) {
                return Ok(decision);
            }
        }
        Ok(DecideToolCallDecision::Execute)
    }

    async fn after_tool_call<'a>(
        &self,
        input: AfterToolCallInput<'a>,
        control: HookControl,
    ) -> Result<AfterToolCallDecision, HookFailure> {
        let mut current: Cow<'_, ChatMessage> = Cow::Borrowed(input.result);
        let mut replacement = None;
        for handler in &self.handlers {
            let decision = handler
                .after_tool_call(
                    AfterToolCallInput {
                        snapshot: input.snapshot,
                        tool_call: input.tool_call,
                        tool: input.tool,
                        result: &current,
                    },
                    control.clone(),
                )
                .await?;
            if let AfterToolCallDecision::ReplaceResult { result } = decision {
                current = Cow::Owned(ChatMessage::tool(
                    input.tool_call.call_id.clone(),
                    result.clone(),
                ));
                replacement = Some(result);
            }
        }
        Ok(match replacement {
            None => AfterToolCallDecision::Keep,
            Some(result) => AfterToolCallDecision::ReplaceResult { result },
        })
    }

    async fn prepare_next_turn<'a>(
        &self,
        input: PrepareNextTurnInput<'a>,
        control: HookControl,
    ) -> Result<PrepareNextTurnDecision, HookFailure> {
        let mut injected = Vec::new();
        for handler in &self.handlers {
            let decision = handler
                .prepare_next_turn(
                    PrepareNextTurnInput {
                        snapshot: input.snapshot,
                    },
                    control.clone(),
                )
                .await?;
            decision.check()?;
            match decision {
                PrepareNextTurnDecision::Continue => {}
                // A stopped loop has no next turn, so injections collected so
                // far are discarded with the short-circuit.
                PrepareNextTurnDecision::Stop => return Ok(PrepareNextTurnDecision::Stop),
                PrepareNextTurnDecision::Inject { messages } => injected.extend(messages),
            }
        }
        if injected.is_empty() {
            return Ok(PrepareNextTurnDecision::Continue);
        }
        Ok(PrepareNextTurnDecision::Inject { messages: injected })
    }

    fn extension_set_version(&self) -> Option<ExtensionSetVersionId> {
        Some(self.extension_set_version)
    }
}

/// Derives the chain's extension set version: the SHA-256 of the ordered
/// handler version id bytes, folded into a deterministic UUIDv8-layout UUID.
fn chain_version(handlers: &[Arc<dyn HookHandler>]) -> ExtensionSetVersionId {
    let mut hasher = Sha256::new();
    for handler in handlers {
        hasher.update(handler.descriptor().version_id.as_uuid().as_bytes());
    }
    let digest = hasher.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    ExtensionSetVersionId::from(uuid::Builder::from_custom_bytes(bytes).into_uuid())
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex, time::Duration};

    use serde_json::{Value, json};
    use stratum_core::{
        CallId, DangerLevel, HookHandlerVersionId, TokenUsage, ToolCall, ToolKind, ToolSpec,
    };
    use tokio::time::Instant;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{HookHandlerDescriptor, NoopHookRuntime};

    /// Owned observation of one handler invocation, recorded in call order.
    #[derive(Debug, Clone, PartialEq)]
    enum HandlerCall {
        TransformContext {
            system_prompt: String,
            messages: Vec<ChatMessage>,
        },
        TransformToolCall {
            arguments: Value,
            authorization: Option<(ToolKind, DangerLevel)>,
        },
        DecideToolCall {
            arguments: Value,
            authorization: Option<(ToolKind, DangerLevel)>,
        },
        AfterToolCall {
            result: ChatMessage,
        },
        PrepareNextTurn,
    }

    /// Programmable outcome of one handler invocation.
    #[derive(Debug, Clone)]
    enum Action<T> {
        Return(Result<T, HookFailure>),
        CancelThenReturn(T),
    }

    /// Shared ordered log of handler invocations across one chain.
    type CallLog = Arc<Mutex<Vec<(&'static str, HandlerCall)>>>;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ObservedControl {
        deadline: Option<Instant>,
        token_cancelled: bool,
    }

    struct ScriptableHandler {
        name: &'static str,
        version_id: HookHandlerVersionId,
        log: CallLog,
        controls: Arc<Mutex<Vec<ObservedControl>>>,
        transforms: Mutex<VecDeque<Action<TransformContextDecision>>>,
        tool_transforms: Mutex<VecDeque<Action<TransformToolCallDecision>>>,
        decides: Mutex<VecDeque<Action<DecideToolCallDecision>>>,
        afters: Mutex<VecDeque<Action<AfterToolCallDecision>>>,
        prepares: Mutex<VecDeque<Action<PrepareNextTurnDecision>>>,
    }

    impl ScriptableHandler {
        fn new(name: &'static str, version_id: HookHandlerVersionId, log: &CallLog) -> Self {
            Self {
                name,
                version_id,
                log: Arc::clone(log),
                controls: Arc::new(Mutex::new(Vec::new())),
                transforms: Mutex::new(VecDeque::new()),
                tool_transforms: Mutex::new(VecDeque::new()),
                decides: Mutex::new(VecDeque::new()),
                afters: Mutex::new(VecDeque::new()),
                prepares: Mutex::new(VecDeque::new()),
            }
        }

        fn with_transforms(
            self,
            actions: impl IntoIterator<Item = Action<TransformContextDecision>>,
        ) -> Self {
            self.transforms
                .lock()
                .expect("action lock should not be poisoned")
                .extend(actions);
            self
        }

        fn with_tool_transforms(
            self,
            actions: impl IntoIterator<Item = Action<TransformToolCallDecision>>,
        ) -> Self {
            self.tool_transforms
                .lock()
                .expect("action lock should not be poisoned")
                .extend(actions);
            self
        }

        fn with_decides(
            self,
            actions: impl IntoIterator<Item = Action<DecideToolCallDecision>>,
        ) -> Self {
            self.decides
                .lock()
                .expect("action lock should not be poisoned")
                .extend(actions);
            self
        }

        fn with_afters(
            self,
            actions: impl IntoIterator<Item = Action<AfterToolCallDecision>>,
        ) -> Self {
            self.afters
                .lock()
                .expect("action lock should not be poisoned")
                .extend(actions);
            self
        }

        fn with_prepares(
            self,
            actions: impl IntoIterator<Item = Action<PrepareNextTurnDecision>>,
        ) -> Self {
            self.prepares
                .lock()
                .expect("action lock should not be poisoned")
                .extend(actions);
            self
        }

        fn record(&self, call: HandlerCall, control: &HookControl) {
            self.controls
                .lock()
                .expect("control lock should not be poisoned")
                .push(ObservedControl {
                    deadline: control.deadline(),
                    token_cancelled: control.cancellation().is_cancelled(),
                });
            self.log
                .lock()
                .expect("call log lock should not be poisoned")
                .push((self.name, call));
        }

        fn pop<T>(&self, queue: &Mutex<VecDeque<Action<T>>>, noop: T) -> Action<T> {
            queue
                .lock()
                .expect("action lock should not be poisoned")
                .pop_front()
                .unwrap_or(Action::Return(Ok(noop)))
        }
    }

    fn resolve<T>(action: Action<T>, control: &HookControl) -> Result<T, HookFailure> {
        match action {
            Action::Return(result) => result,
            Action::CancelThenReturn(decision) => {
                control.cancellation().cancel();
                Ok(decision)
            }
        }
    }

    #[async_trait]
    impl HookHandler for ScriptableHandler {
        fn descriptor(&self) -> HookHandlerDescriptor {
            HookHandlerDescriptor::new(self.version_id)
        }

        async fn transform_context<'a>(
            &self,
            input: TransformContextInput<'a>,
            control: HookControl,
        ) -> Result<TransformContextDecision, HookFailure> {
            self.record(
                HandlerCall::TransformContext {
                    system_prompt: input.snapshot.context.system_prompt.clone(),
                    messages: input.snapshot.context.messages.clone(),
                },
                &control,
            );
            let action = self.pop(&self.transforms, TransformContextDecision::Unchanged);
            resolve(action, &control)
        }

        async fn transform_tool_call<'a>(
            &self,
            input: TransformToolCallInput<'a>,
            control: HookControl,
        ) -> Result<TransformToolCallDecision, HookFailure> {
            self.record(
                HandlerCall::TransformToolCall {
                    arguments: input.tool_call.arguments.clone(),
                    authorization: input.tool.authorization,
                },
                &control,
            );
            let action = self.pop(&self.tool_transforms, TransformToolCallDecision::Continue);
            resolve(action, &control)
        }

        async fn decide_tool_call<'a>(
            &self,
            input: DecideToolCallInput<'a>,
            control: HookControl,
        ) -> Result<DecideToolCallDecision, HookFailure> {
            self.record(
                HandlerCall::DecideToolCall {
                    arguments: input.tool_call.arguments.clone(),
                    authorization: input.tool.authorization,
                },
                &control,
            );
            let action = self.pop(&self.decides, DecideToolCallDecision::Execute);
            resolve(action, &control)
        }

        async fn after_tool_call<'a>(
            &self,
            input: AfterToolCallInput<'a>,
            control: HookControl,
        ) -> Result<AfterToolCallDecision, HookFailure> {
            self.record(
                HandlerCall::AfterToolCall {
                    result: input.result.clone(),
                },
                &control,
            );
            let action = self.pop(&self.afters, AfterToolCallDecision::Keep);
            resolve(action, &control)
        }

        async fn prepare_next_turn<'a>(
            &self,
            input: PrepareNextTurnInput<'a>,
            control: HookControl,
        ) -> Result<PrepareNextTurnDecision, HookFailure> {
            let _ = input;
            self.record(HandlerCall::PrepareNextTurn, &control);
            let action = self.pop(&self.prepares, PrepareNextTurnDecision::Continue);
            resolve(action, &control)
        }
    }

    fn handler(
        name: &'static str,
        version_id: HookHandlerVersionId,
        log: &CallLog,
    ) -> Arc<ScriptableHandler> {
        Arc::new(ScriptableHandler::new(name, version_id, log))
    }

    fn calls(log: &CallLog) -> Vec<(&'static str, HandlerCall)> {
        log.lock()
            .expect("call log lock should not be poisoned")
            .clone()
    }

    fn build_chain(handlers: Vec<Arc<ScriptableHandler>>) -> ChainHookRuntime {
        ChainHookRuntime::new(handlers.into_iter().map(|handler| handler as _).collect())
    }

    fn test_context() -> LoopContext {
        LoopContext::new("be precise").with_messages(vec![
            ChatMessage::user("first"),
            ChatMessage::user("second"),
        ])
    }

    fn test_tool_call() -> ToolCall {
        ToolCall {
            call_id: CallId::from("call-1"),
            name: "echo".to_owned(),
            arguments: json!({"value": 1}),
        }
    }

    fn test_spec() -> ToolSpec {
        ToolSpec::builder()
            .name("echo")
            .description("records calls")
            .input_schema(json!({"type": "object"}))
            .build()
    }

    fn test_control() -> HookControl {
        HookControl::new(CancellationToken::new(), None)
    }

    fn modify_arguments(arguments: Value) -> Action<TransformToolCallDecision> {
        Action::Return(Ok(TransformToolCallDecision::Modify(
            TransformToolCallModification::new(Some(arguments), None),
        )))
    }

    #[tokio::test]
    async fn transform_tool_call_threads_arguments_and_authorization_in_order() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let first = ScriptableHandler::new("first", HookHandlerVersionId::new(), &log)
            .with_tool_transforms([modify_arguments(json!({"value": 2}))]);
        let second = ScriptableHandler::new("second", HookHandlerVersionId::new(), &log)
            .with_tool_transforms([Action::Return(Ok(TransformToolCallDecision::Modify(
                TransformToolCallModification::new(
                    Some(json!({"value": 3})),
                    Some(AuthorizationOverride::PreAuthorize),
                ),
            )))]);
        let chain = build_chain(vec![Arc::new(first), Arc::new(second)]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: Some((ToolKind::Write, DangerLevel::High)),
            spec: &spec,
        };

        let decision = chain
            .transform_tool_call(
                TransformToolCallInput {
                    snapshot,
                    tool_call: &tool_call,
                    tool: &target,
                },
                test_control(),
            )
            .await
            .expect("chain should succeed");

        assert_eq!(
            decision,
            TransformToolCallDecision::Modify(TransformToolCallModification::new(
                Some(json!({"value": 3})),
                Some(AuthorizationOverride::PreAuthorize),
            ))
        );
        assert_eq!(
            calls(&log),
            vec![
                (
                    "first",
                    HandlerCall::TransformToolCall {
                        arguments: json!({"value": 1}),
                        authorization: Some((ToolKind::Write, DangerLevel::High)),
                    },
                ),
                (
                    "second",
                    HandlerCall::TransformToolCall {
                        arguments: json!({"value": 2}),
                        authorization: Some((ToolKind::Write, DangerLevel::High)),
                    },
                ),
            ]
        );
    }

    #[tokio::test]
    async fn transform_tool_call_net_no_change_collapses_to_continue() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let identity = ScriptableHandler::new("identity", HookHandlerVersionId::new(), &log)
            .with_tool_transforms([Action::Return(Ok(TransformToolCallDecision::Modify(
                TransformToolCallModification::new(
                    None,
                    Some(AuthorizationOverride::Set {
                        kind: ToolKind::Write,
                        danger: DangerLevel::High,
                    }),
                ),
            )))]);
        let chain = build_chain(vec![Arc::new(identity)]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: Some((ToolKind::Write, DangerLevel::High)),
            spec: &spec,
        };

        let decision = chain
            .transform_tool_call(
                TransformToolCallInput {
                    snapshot,
                    tool_call: &tool_call,
                    tool: &target,
                },
                test_control(),
            )
            .await
            .expect("chain should succeed");

        assert_eq!(decision, TransformToolCallDecision::Continue);
    }

    #[tokio::test]
    async fn transform_tool_call_invalid_decision_fails_closed_and_stops_the_chain() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let invalid = ScriptableHandler::new("invalid", HookHandlerVersionId::new(), &log)
            .with_tool_transforms([Action::Return(Ok(TransformToolCallDecision::Modify(
                TransformToolCallModification::new(None, None),
            )))]);
        let later = handler("later", HookHandlerVersionId::new(), &log);
        let chain = build_chain(vec![Arc::new(invalid), later]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: None,
            spec: &spec,
        };

        let result = chain
            .transform_tool_call(
                TransformToolCallInput {
                    snapshot,
                    tool_call: &tool_call,
                    tool: &target,
                },
                test_control(),
            )
            .await;

        assert_eq!(result, Err(HookFailure::InvalidOutput));
        assert_eq!(calls(&log).len(), 1, "the later handler must not run");
    }

    #[tokio::test]
    async fn transform_context_threads_patches_and_composes_in_order() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let first = ScriptableHandler::new("first", HookHandlerVersionId::new(), &log)
            .with_transforms([Action::Return(Ok(TransformContextDecision::Patch(
                ContextPatch::ReplaceSystemPrompt("patched prompt".to_owned()),
            )))]);
        let second = ScriptableHandler::new("second", HookHandlerVersionId::new(), &log)
            .with_transforms([Action::Return(Ok(TransformContextDecision::Patch(
                ContextPatch::DropHistory { upto: 1 },
            )))]);
        let chain = build_chain(vec![Arc::new(first), Arc::new(second)]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);

        let decision = chain
            .transform_context(TransformContextInput { snapshot }, test_control())
            .await
            .expect("chain should succeed");

        assert_eq!(
            decision,
            TransformContextDecision::Patch(ContextPatch::Composite(vec![
                ContextPatch::ReplaceSystemPrompt("patched prompt".to_owned()),
                ContextPatch::DropHistory { upto: 1 },
            ]))
        );
        assert_eq!(
            calls(&log),
            vec![
                (
                    "first",
                    HandlerCall::TransformContext {
                        system_prompt: "be precise".to_owned(),
                        messages: context.messages.clone(),
                    },
                ),
                (
                    "second",
                    HandlerCall::TransformContext {
                        system_prompt: "patched prompt".to_owned(),
                        messages: context.messages.clone(),
                    },
                ),
            ]
        );
    }

    #[tokio::test]
    async fn transform_context_single_patch_stays_unwrapped() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let only = ScriptableHandler::new("only", HookHandlerVersionId::new(), &log)
            .with_transforms([Action::Return(Ok(TransformContextDecision::Patch(
                ContextPatch::DropHistory { upto: 2 },
            )))]);
        let chain = build_chain(vec![Arc::new(only)]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);

        let decision = chain
            .transform_context(TransformContextInput { snapshot }, test_control())
            .await
            .expect("chain should succeed");

        assert_eq!(
            decision,
            TransformContextDecision::Patch(ContextPatch::DropHistory { upto: 2 })
        );
    }

    #[tokio::test]
    async fn transform_context_invalid_patch_fails_closed_and_stops_the_chain() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let invalid = ScriptableHandler::new("invalid", HookHandlerVersionId::new(), &log)
            .with_transforms([Action::Return(Ok(TransformContextDecision::Patch(
                ContextPatch::DropHistory { upto: 99 },
            )))]);
        let later = handler("later", HookHandlerVersionId::new(), &log);
        let chain = build_chain(vec![Arc::new(invalid), later]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);

        let result = chain
            .transform_context(TransformContextInput { snapshot }, test_control())
            .await;

        assert_eq!(result, Err(HookFailure::InvalidOutput));
        assert_eq!(calls(&log).len(), 1, "the later handler must not run");
    }

    #[tokio::test]
    async fn after_tool_call_threads_replacements_in_order() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let first =
            ScriptableHandler::new("first", HookHandlerVersionId::new(), &log).with_afters([
                Action::Return(Ok(AfterToolCallDecision::ReplaceResult {
                    result: json!({"stage": "first"}),
                })),
            ]);
        let second = ScriptableHandler::new("second", HookHandlerVersionId::new(), &log)
            .with_afters([Action::Return(Ok(AfterToolCallDecision::ReplaceResult {
                result: json!({"stage": "second"}),
            }))]);
        let chain = build_chain(vec![Arc::new(first), Arc::new(second)]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: None,
            spec: &spec,
        };
        let original = ChatMessage::tool(tool_call.call_id.clone(), json!({"stage": "original"}));

        let decision = chain
            .after_tool_call(
                AfterToolCallInput {
                    snapshot,
                    tool_call: &tool_call,
                    tool: &target,
                    result: &original,
                },
                test_control(),
            )
            .await
            .expect("chain should succeed");

        assert_eq!(
            decision,
            AfterToolCallDecision::ReplaceResult {
                result: json!({"stage": "second"}),
            }
        );
        let observed = calls(&log);
        assert_eq!(
            observed[0],
            (
                "first",
                HandlerCall::AfterToolCall {
                    result: original.clone(),
                },
            )
        );
        assert_eq!(
            observed[1],
            (
                "second",
                HandlerCall::AfterToolCall {
                    result: ChatMessage::tool(tool_call.call_id.clone(), json!({"stage": "first"}),),
                },
            )
        );
    }

    #[tokio::test]
    async fn after_tool_call_without_replacements_keeps_the_result() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let chain = build_chain(vec![
            handler("a", HookHandlerVersionId::new(), &log),
            handler("b", HookHandlerVersionId::new(), &log),
        ]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: None,
            spec: &spec,
        };
        let original = ChatMessage::tool(tool_call.call_id.clone(), json!({"ok": true}));

        let decision = chain
            .after_tool_call(
                AfterToolCallInput {
                    snapshot,
                    tool_call: &tool_call,
                    tool: &target,
                    result: &original,
                },
                test_control(),
            )
            .await
            .expect("chain should succeed");

        assert_eq!(decision, AfterToolCallDecision::Keep);
    }

    #[tokio::test]
    async fn decide_short_circuits_on_the_first_block() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let blocker = ScriptableHandler::new("blocker", HookHandlerVersionId::new(), &log)
            .with_decides([Action::Return(Ok(DecideToolCallDecision::Block {
                reason: "policy denied".to_owned(),
            }))]);
        let chain = build_chain(vec![
            handler("approver", HookHandlerVersionId::new(), &log),
            Arc::new(blocker),
            handler("never", HookHandlerVersionId::new(), &log),
        ]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: None,
            spec: &spec,
        };

        let decision = chain
            .decide_tool_call(
                DecideToolCallInput {
                    snapshot,
                    tool_call: &tool_call,
                    tool: &target,
                },
                test_control(),
            )
            .await
            .expect("chain should succeed");

        assert_eq!(
            decision,
            DecideToolCallDecision::Block {
                reason: "policy denied".to_owned(),
            }
        );
        let observed = calls(&log);
        assert_eq!(observed.len(), 2, "the handler after a block must not run");
        assert_eq!(observed[0].0, "approver");
        assert_eq!(observed[1].0, "blocker");
    }

    #[tokio::test]
    async fn decide_all_execute_and_empty_reason_blocks_fail_closed() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let chain = build_chain(vec![
            handler("a", HookHandlerVersionId::new(), &log),
            handler("b", HookHandlerVersionId::new(), &log),
        ]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: None,
            spec: &spec,
        };
        let decide_input = || DecideToolCallInput {
            snapshot,
            tool_call: &tool_call,
            tool: &target,
        };

        let decision = chain
            .decide_tool_call(decide_input(), test_control())
            .await
            .expect("all-execute chain should execute");
        assert_eq!(decision, DecideToolCallDecision::Execute);

        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let invalid = ScriptableHandler::new("invalid", HookHandlerVersionId::new(), &log)
            .with_decides([Action::Return(Ok(DecideToolCallDecision::Block {
                reason: "  ".to_owned(),
            }))]);
        let later = handler("later", HookHandlerVersionId::new(), &log);
        let chain = build_chain(vec![Arc::new(invalid), later]);

        let result = chain.decide_tool_call(decide_input(), test_control()).await;
        assert_eq!(result, Err(HookFailure::InvalidOutput));
        assert_eq!(calls(&log).len(), 1, "the later handler must not run");
    }

    #[tokio::test]
    async fn prepare_merges_injections_in_handler_order() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let first = ScriptableHandler::new("first", HookHandlerVersionId::new(), &log)
            .with_prepares([Action::Return(Ok(PrepareNextTurnDecision::Inject {
                messages: vec![ChatMessage::user("note-1")],
            }))]);
        let third = ScriptableHandler::new("third", HookHandlerVersionId::new(), &log)
            .with_prepares([Action::Return(Ok(PrepareNextTurnDecision::Inject {
                messages: vec![ChatMessage::user("note-2"), ChatMessage::user("note-3")],
            }))]);
        let chain = build_chain(vec![
            Arc::new(first),
            handler("middle", HookHandlerVersionId::new(), &log),
            Arc::new(third),
        ]);
        let context = test_context();
        let snapshot = HookSnapshot::new(2, &context, None);

        let decision = chain
            .prepare_next_turn(PrepareNextTurnInput { snapshot }, test_control())
            .await
            .expect("chain should succeed");

        assert_eq!(
            decision,
            PrepareNextTurnDecision::Inject {
                messages: vec![
                    ChatMessage::user("note-1"),
                    ChatMessage::user("note-2"),
                    ChatMessage::user("note-3"),
                ],
            }
        );
    }

    #[tokio::test]
    async fn prepare_stop_discards_collected_injections_and_short_circuits() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let injector = ScriptableHandler::new("injector", HookHandlerVersionId::new(), &log)
            .with_prepares([Action::Return(Ok(PrepareNextTurnDecision::Inject {
                messages: vec![ChatMessage::user("discarded")],
            }))]);
        let stopper = ScriptableHandler::new("stopper", HookHandlerVersionId::new(), &log)
            .with_prepares([Action::Return(Ok(PrepareNextTurnDecision::Stop))]);
        let chain = build_chain(vec![
            Arc::new(injector),
            Arc::new(stopper),
            handler("never", HookHandlerVersionId::new(), &log),
        ]);
        let context = test_context();
        let snapshot = HookSnapshot::new(2, &context, None);

        let decision = chain
            .prepare_next_turn(PrepareNextTurnInput { snapshot }, test_control())
            .await
            .expect("chain should succeed");

        assert_eq!(decision, PrepareNextTurnDecision::Stop);
        let observed = calls(&log);
        assert_eq!(observed.len(), 2, "the handler after a stop must not run");
        assert_eq!(observed[0].0, "injector");
        assert_eq!(observed[1].0, "stopper");
    }

    #[tokio::test]
    async fn prepare_all_continue_and_invalid_injections_fail_closed() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let chain = build_chain(vec![
            handler("a", HookHandlerVersionId::new(), &log),
            handler("b", HookHandlerVersionId::new(), &log),
        ]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);

        let decision = chain
            .prepare_next_turn(PrepareNextTurnInput { snapshot }, test_control())
            .await
            .expect("all-continue chain should continue");
        assert_eq!(decision, PrepareNextTurnDecision::Continue);

        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let invalid = ScriptableHandler::new("invalid", HookHandlerVersionId::new(), &log)
            .with_prepares([Action::Return(Ok(PrepareNextTurnDecision::Inject {
                messages: vec![ChatMessage::assistant("forged")],
            }))]);
        let later = handler("later", HookHandlerVersionId::new(), &log);
        let chain = build_chain(vec![Arc::new(invalid), later]);

        let result = chain
            .prepare_next_turn(PrepareNextTurnInput { snapshot }, test_control())
            .await;
        assert_eq!(result, Err(HookFailure::InvalidOutput));
        assert_eq!(calls(&log).len(), 1, "the later handler must not run");
    }

    #[tokio::test]
    async fn a_handler_failure_fails_the_point_and_stops_the_chain() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let failing = ScriptableHandler::new("failing", HookHandlerVersionId::new(), &log)
            .with_decides([Action::Return(Err(HookFailure::HandlerFailed))]);
        let chain = build_chain(vec![
            handler("before", HookHandlerVersionId::new(), &log),
            Arc::new(failing),
            handler("after", HookHandlerVersionId::new(), &log),
        ]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: None,
            spec: &spec,
        };

        let result = chain
            .decide_tool_call(
                DecideToolCallInput {
                    snapshot,
                    tool_call: &tool_call,
                    tool: &target,
                },
                test_control(),
            )
            .await;

        assert_eq!(result, Err(HookFailure::HandlerFailed));
        let observed = calls(&log);
        assert_eq!(
            observed.len(),
            2,
            "the handler after a failure must not run"
        );
        assert_eq!(observed[1].0, "failing");
    }

    #[tokio::test]
    async fn every_handler_receives_the_kernel_control_unchanged() {
        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let canceller = ScriptableHandler::new("canceller", HookHandlerVersionId::new(), &log)
            .with_prepares([Action::CancelThenReturn(PrepareNextTurnDecision::Continue)]);
        let observer = handler("observer", HookHandlerVersionId::new(), &log);
        let observer_controls = Arc::clone(&observer.controls);
        let canceller_controls = Arc::clone(&canceller.controls);
        let chain = build_chain(vec![Arc::new(canceller), observer]);
        let context = test_context();
        let snapshot = HookSnapshot::new(0, &context, None);
        let deadline = Instant::now() + Duration::from_secs(30);
        let control = HookControl::new(CancellationToken::new(), Some(deadline));

        let decision = chain
            .prepare_next_turn(PrepareNextTurnInput { snapshot }, control)
            .await
            .expect("chain should succeed");

        assert_eq!(decision, PrepareNextTurnDecision::Continue);
        let canceller_controls = canceller_controls
            .lock()
            .expect("control lock should not be poisoned")
            .clone();
        let observer_controls = observer_controls
            .lock()
            .expect("control lock should not be poisoned")
            .clone();
        assert_eq!(
            canceller_controls,
            vec![ObservedControl {
                deadline: Some(deadline),
                token_cancelled: false,
            }]
        );
        // The observer sees the same token (already cancelled by the previous
        // handler) and the same absolute deadline.
        assert_eq!(
            observer_controls,
            vec![ObservedControl {
                deadline: Some(deadline),
                token_cancelled: true,
            }]
        );
    }

    #[tokio::test]
    async fn an_empty_chain_matches_the_noop_runtime_decisions() {
        let chain = ChainHookRuntime::new(Vec::new());
        assert!(chain.extension_set_version().is_some());
        assert!(NoopHookRuntime.extension_set_version().is_none());

        let context = test_context();
        let usage = TokenUsage {
            input_tokens: 3,
            output_tokens: 2,
            total_tokens: 5,
        };
        let snapshot = HookSnapshot::new(1, &context, Some(usage));
        let tool_call = test_tool_call();
        let spec = test_spec();
        let target = ToolHookTarget {
            authorization: Some((ToolKind::Read, DangerLevel::Low)),
            spec: &spec,
        };
        let result = ChatMessage::tool(tool_call.call_id.clone(), json!({"ok": true}));

        assert_eq!(
            chain
                .transform_context(TransformContextInput { snapshot }, test_control())
                .await,
            Ok(TransformContextDecision::Unchanged)
        );
        assert_eq!(
            chain
                .transform_tool_call(
                    TransformToolCallInput {
                        snapshot,
                        tool_call: &tool_call,
                        tool: &target,
                    },
                    test_control(),
                )
                .await,
            Ok(TransformToolCallDecision::Continue)
        );
        assert_eq!(
            chain
                .decide_tool_call(
                    DecideToolCallInput {
                        snapshot,
                        tool_call: &tool_call,
                        tool: &target,
                    },
                    test_control(),
                )
                .await,
            Ok(DecideToolCallDecision::Execute)
        );
        assert_eq!(
            chain
                .after_tool_call(
                    AfterToolCallInput {
                        snapshot,
                        tool_call: &tool_call,
                        tool: &target,
                        result: &result,
                    },
                    test_control(),
                )
                .await,
            Ok(AfterToolCallDecision::Keep)
        );
        assert_eq!(
            chain
                .prepare_next_turn(PrepareNextTurnInput { snapshot }, test_control())
                .await,
            Ok(PrepareNextTurnDecision::Continue)
        );
    }

    #[test]
    fn chain_version_is_fixed_by_order_membership_and_handler_versions() {
        let version_a = HookHandlerVersionId::new();
        let version_b = HookHandlerVersionId::new();
        let build = |first: HookHandlerVersionId, second: HookHandlerVersionId| {
            let log: CallLog = Arc::new(Mutex::new(Vec::new()));
            build_chain(vec![handler("a", first, &log), handler("b", second, &log)])
                .extension_set_version()
        };

        let pinned = build(version_a, version_b);
        assert_eq!(
            pinned,
            build(version_a, version_b),
            "same order same version"
        );
        assert_ne!(
            pinned,
            build(version_b, version_a),
            "a different order must change the version"
        );
        assert_ne!(
            pinned,
            build(version_a, HookHandlerVersionId::new()),
            "a handler version change must change the chain version"
        );

        let log: CallLog = Arc::new(Mutex::new(Vec::new()));
        let empty = ChainHookRuntime::new(Vec::new()).extension_set_version();
        assert_eq!(
            empty,
            ChainHookRuntime::new(Vec::new()).extension_set_version()
        );
        let single = build_chain(vec![handler("a", version_a, &log)]).extension_set_version();
        assert_ne!(empty, single);
    }
}
