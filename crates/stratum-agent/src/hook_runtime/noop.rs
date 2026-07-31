//! No-op hook runtime preserving the pre-hook kernel behavior.

use async_trait::async_trait;
use stratum_core::HookFailure;

use super::{
    AfterToolCallDecision, AfterToolCallInput, DecideToolCallDecision, DecideToolCallInput,
    HookControl, HookRuntime, PrepareNextTurnDecision, PrepareNextTurnInput,
    TransformContextDecision, TransformContextInput, TransformToolCallDecision,
    TransformToolCallInput,
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

    async fn transform_tool_call<'a>(
        &self,
        _input: TransformToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<TransformToolCallDecision, HookFailure> {
        Ok(TransformToolCallDecision::Continue)
    }

    async fn decide_tool_call<'a>(
        &self,
        _input: DecideToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<DecideToolCallDecision, HookFailure> {
        Ok(DecideToolCallDecision::Execute)
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
    use stratum_core::{CallId, ChatMessage, DangerLevel, ToolCall, ToolKind, ToolSpec};
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::{LoopContext, ToolHookTarget};

    fn control() -> HookControl {
        HookControl::new(CancellationToken::new(), None)
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
        let spec = ToolSpec::builder()
            .name("echo")
            .description("records calls")
            .input_schema(serde_json::json!({"type": "object"}))
            .build();
        let target = ToolHookTarget {
            authorization: Some((ToolKind::Read, DangerLevel::Low)),
            spec: &spec,
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
                .transform_tool_call(
                    TransformToolCallInput {
                        iteration: 0,
                        tool_call: &tool_call,
                        tool: &target,
                    },
                    control(),
                )
                .await,
            Ok(TransformToolCallDecision::Continue)
        );
        assert_eq!(
            runtime
                .decide_tool_call(
                    DecideToolCallInput {
                        iteration: 0,
                        tool_call: &tool_call,
                        tool: &target,
                    },
                    control(),
                )
                .await,
            Ok(DecideToolCallDecision::Execute)
        );
        assert_eq!(
            runtime
                .after_tool_call(
                    AfterToolCallInput {
                        iteration: 0,
                        tool_call: &tool_call,
                        tool: &target,
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
