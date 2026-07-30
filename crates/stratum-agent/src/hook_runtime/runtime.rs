//! Hook runtime contract executed at the agent loop's decision boundaries.
//!
//! Typed failures reuse [`HookFailure`] from `stratum-core`; the loop-facing
//! mapping lives in [`crate::AgentLoopError`].

use async_trait::async_trait;
use serde_json::Value;
use stratum_core::{ChatMessage, ChatRole, HookFailure, ToolCall};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::LoopContext;

/// Control information the agent loop enforces for every hook invocation.
///
/// The loop stops waiting for a hook when the cancellation token fires or the
/// absolute deadline passes, regardless of what the runtime itself does.
#[derive(Debug, Clone)]
pub struct HookControl {
    cancellation: CancellationToken,
    deadline: Instant,
}

impl HookControl {
    /// Creates control information from the turn token and an absolute deadline.
    #[must_use]
    pub fn new(cancellation: CancellationToken, deadline: Instant) -> Self {
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

    /// Returns the absolute deadline after which the loop stops waiting.
    #[must_use]
    pub const fn deadline(&self) -> Instant {
        self.deadline
    }
}

/// Borrowed input to [`HookRuntime::transform_context`].
#[derive(Debug)]
pub struct TransformContextInput<'a> {
    /// Zero-based model iteration the request belongs to.
    pub iteration: u64,
    /// Committed context plus any one-shot injected messages for this request.
    pub context: &'a LoopContext,
}

/// Decision returned by [`HookRuntime::transform_context`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TransformContextDecision {
    /// Use the supplied context view unchanged.
    Unchanged,
    /// Replace the context for the current model request only. The replacement
    /// is never committed back to the transcript or the loop outcome.
    Replace {
        /// Request-scoped replacement context.
        context: LoopContext,
    },
}

/// Borrowed input to [`HookRuntime::before_tool_call`].
#[derive(Debug)]
pub struct BeforeToolCallInput<'a> {
    /// Zero-based model iteration that produced the tool call.
    pub iteration: u64,
    /// Provider tool call authorized by a `tool_calls` finish reason.
    pub tool_call: &'a ToolCall,
}

/// Decision returned by [`HookRuntime::before_tool_call`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum BeforeToolCallDecision {
    /// Hand the original tool call to the tool executor.
    Continue,
    /// Execute the same call identity and tool name with new arguments.
    ModifyArguments {
        /// Replacement JSON arguments.
        arguments: Value,
    },
    /// Skip approval and execution, answering with a structured `hook_blocked`
    /// tool result. The reason must be non-empty and safe to show the model.
    Block {
        /// Model-visible block reason.
        reason: String,
    },
}

impl BeforeToolCallDecision {
    /// Enforces the decision contract before the loop applies it.
    pub(crate) fn validate(self) -> Result<Self, HookFailure> {
        match &self {
            Self::Block { reason } if reason.trim().is_empty() => Err(HookFailure::InvalidOutput),
            _ => Ok(self),
        }
    }
}

/// Borrowed input to [`HookRuntime::after_tool_call`].
#[derive(Debug)]
pub struct AfterToolCallInput<'a> {
    /// Zero-based model iteration that produced the tool call.
    pub iteration: u64,
    /// Tool call as it was executed, including hook-modified arguments.
    pub tool_call: &'a ToolCall,
    /// Model-visible tool result produced by execution or a before-tool block.
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
    /// Zero-based model iteration whose tool cycle just committed.
    pub iteration: u64,
    /// Committed context including this iteration's assistant and tool results.
    pub context: &'a LoopContext,
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
    pub(crate) fn validate(self) -> Result<Self, HookFailure> {
        match &self {
            Self::Inject { messages }
                if messages.is_empty() || !messages.iter().all(is_plain_user_message) =>
            {
                Err(HookFailure::InvalidOutput)
            }
            _ => Ok(self),
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
/// [`HookControl`] cancellation token and absolute deadline around every call;
/// hook futures must therefore be cancellation-safe and must not start external
/// side effects that cannot be abandoned safely before returning a decision.
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

    /// Decides whether and how one authorized tool call executes.
    ///
    /// # Errors
    ///
    /// Returns a safe [`HookFailure`] classification; the affected tool call is
    /// then neither approved nor executed and the loop fails closed.
    async fn before_tool_call<'a>(
        &self,
        input: BeforeToolCallInput<'a>,
        control: HookControl,
    ) -> Result<BeforeToolCallDecision, HookFailure>;

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
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stratum_core::CallId;

    use super::*;

    #[test]
    fn block_decisions_require_a_non_empty_reason() {
        for reason in ["", "   "] {
            let decision = BeforeToolCallDecision::Block {
                reason: reason.to_owned(),
            };
            assert_eq!(decision.validate(), Err(HookFailure::InvalidOutput));
        }
        let decision = BeforeToolCallDecision::Block {
            reason: "policy denied".to_owned(),
        };
        assert_eq!(
            decision.validate(),
            Ok(BeforeToolCallDecision::Block {
                reason: "policy denied".to_owned(),
            })
        );
        let decision = BeforeToolCallDecision::ModifyArguments {
            arguments: json!({"value": 1}),
        };
        assert!(decision.validate().is_ok());
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
            assert_eq!(decision.validate().is_ok(), expected);
        }
        let decision = PrepareNextTurnDecision::Continue;
        assert!(decision.validate().is_ok());
        let decision = PrepareNextTurnDecision::Stop;
        assert!(decision.validate().is_ok());
    }

    #[test]
    fn hook_control_exposes_the_shared_token_and_deadline() {
        let cancellation = CancellationToken::new();
        let deadline = Instant::now();
        let control = HookControl::new(cancellation.clone(), deadline);

        assert!(!control.cancellation().is_cancelled());
        assert_eq!(control.deadline(), deadline);
        cancellation.cancel();
        assert!(control.cancellation().is_cancelled());
    }
}
