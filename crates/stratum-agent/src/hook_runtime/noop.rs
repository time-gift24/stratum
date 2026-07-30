//! No-op hook runtime preserving the pre-hook kernel behavior.

use async_trait::async_trait;
use stratum_core::HookFailure;

use super::{
    AfterToolCallDecision, AfterToolCallInput, BeforeToolCallDecision, BeforeToolCallInput,
    HookControl, HookRuntime, PrepareNextTurnDecision, PrepareNextTurnInput,
    TransformContextDecision, TransformContextInput,
};

/// Hook runtime that changes nothing and borrows without copying.
///
/// This is the default runtime when [`crate::AgentLoopBuilder`] is not given a
/// custom one; with it the kernel produces exactly the requests, durable
/// events, tool calls, messages, and terminal outcomes it had before hooks.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHookRuntime;

#[async_trait]
impl HookRuntime for NoopHookRuntime {
    async fn transform_context<'a>(
        &self,
        _input: TransformContextInput<'a>,
        _control: HookControl,
    ) -> Result<TransformContextDecision, HookFailure> {
        Ok(TransformContextDecision::Unchanged)
    }

    async fn before_tool_call<'a>(
        &self,
        _input: BeforeToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<BeforeToolCallDecision, HookFailure> {
        Ok(BeforeToolCallDecision::Continue)
    }

    async fn after_tool_call<'a>(
        &self,
        _input: AfterToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<AfterToolCallDecision, HookFailure> {
        Ok(AfterToolCallDecision::Keep)
    }

    async fn prepare_next_turn<'a>(
        &self,
        _input: PrepareNextTurnInput<'a>,
        _control: HookControl,
    ) -> Result<PrepareNextTurnDecision, HookFailure> {
        Ok(PrepareNextTurnDecision::Continue)
    }
}

#[cfg(test)]
mod tests {
    use stratum_core::{CallId, ChatMessage, ToolCall};
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::LoopContext;

    fn control() -> HookControl {
        HookControl::new(CancellationToken::new(), tokio::time::Instant::now())
    }

    #[tokio::test]
    async fn noop_runtime_keeps_every_boundary_unchanged() {
        let runtime = NoopHookRuntime;
        let context = LoopContext::new("be precise");
        let tool_call = ToolCall {
            call_id: CallId::from("call-1"),
            name: "echo".to_owned(),
            arguments: serde_json::json!({}),
        };
        let result = ChatMessage::tool(tool_call.call_id.clone(), serde_json::json!({"ok": true}));

        assert_eq!(
            runtime
                .transform_context(
                    TransformContextInput {
                        iteration: 0,
                        context: &context,
                    },
                    control(),
                )
                .await,
            Ok(TransformContextDecision::Unchanged)
        );
        assert_eq!(
            runtime
                .before_tool_call(
                    BeforeToolCallInput {
                        iteration: 0,
                        tool_call: &tool_call,
                    },
                    control(),
                )
                .await,
            Ok(BeforeToolCallDecision::Continue)
        );
        assert_eq!(
            runtime
                .after_tool_call(
                    AfterToolCallInput {
                        iteration: 0,
                        tool_call: &tool_call,
                        result: &result,
                    },
                    control(),
                )
                .await,
            Ok(AfterToolCallDecision::Keep)
        );
        assert_eq!(
            runtime
                .prepare_next_turn(
                    PrepareNextTurnInput {
                        iteration: 0,
                        context: &context,
                    },
                    control(),
                )
                .await,
            Ok(PrepareNextTurnDecision::Continue)
        );
    }
}
