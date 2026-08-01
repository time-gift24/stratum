use std::sync::Arc;

use serde_json::json;
use stratum_core::{
    ChatMessage, DangerLevel, DurableAgentEvent, ToolCall, ToolKind, ToolName, ToolSpec,
};
use stratum_infra::DurableEventSink;
use stratum_tools::{Tool, ToolError, ToolInput, ToolRegistry};
use tokio_util::sync::CancellationToken;

use super::ToolExecutorError;

/// Authorization metadata and the registered tool behind one tool name.
type HookLookup = (Option<(ToolKind, DangerLevel)>, Arc<dyn Tool>);

/// Sequential executor that durably gates external tool calls.
///
/// The executor is pure mechanism: lookup, deterministic validation, the
/// durable `ToolExecutionStarted` boundary, and the call itself. It has no
/// authorization concept; execution policy (argument transformation,
/// authorization overrides, approval, blocking) lives in the agent loop's
/// hook runtime.
pub struct ToolExecutor {
    registry: Arc<dyn ToolRegistry>,
    durable_events: Arc<dyn DurableEventSink>,
}

impl ToolExecutor {
    /// Creates an executor from its registry and durable sink.
    #[must_use]
    pub fn new(registry: Arc<dyn ToolRegistry>, durable_events: Arc<dyn DurableEventSink>) -> Self {
        Self {
            registry,
            durable_events,
        }
    }

    /// Returns provider-visible specifications from the registry.
    #[must_use]
    pub fn specs(&self) -> Vec<ToolSpec> {
        self.registry.specs()
    }

    pub(crate) fn durable_events(&self) -> Arc<dyn DurableEventSink> {
        Arc::clone(&self.durable_events)
    }

    /// Resolved hook target behind one tool name: authorization metadata and
    /// the registered tool.
    pub(crate) fn hook_lookup(&self, tool_name: &ToolName) -> Result<HookLookup, ToolError> {
        let authorization = self.registry.authorization(tool_name)?;
        let tool = self
            .registry
            .get(tool_name)
            .ok_or_else(|| ToolError::ToolNotFound {
                name: tool_name.clone(),
            })?;
        Ok((authorization, tool))
    }

    /// Validates one tool call's arguments without starting external work.
    pub(crate) fn validate_call(
        &self,
        tool_name: &ToolName,
        tool_call: &ToolCall,
    ) -> Result<(), ToolError> {
        let input = ToolInput::new(tool_call.call_id.clone(), tool_call.arguments.clone());
        self.registry.validate(tool_name, &input)
    }

    /// Executes one decide-approved tool call through its resolved tool handle.
    ///
    /// The caller (the agent loop) has already resolved the handle, validated
    /// the final arguments, and obtained an execute decision; this method only
    /// checks cancellation, records the durable `ToolExecutionStarted`
    /// boundary, and dispatches the call.
    ///
    /// # Errors
    ///
    /// Returns an error when a required durable event fails or cancellation
    /// prevents the execution boundary.
    ///
    /// # Cancellation safety
    ///
    /// Cancellation is cooperative through the supplied token. After
    /// `ToolExecutionStarted` is durably acknowledged, callers must await this method until the
    /// tool reports an outcome; racing or dropping the execution future can lose knowledge of an
    /// external side effect.
    pub(crate) async fn execute(
        &self,
        tool: &Arc<dyn Tool>,
        tool_call: &ToolCall,
        cancellation: &CancellationToken,
    ) -> Result<ChatMessage, ToolExecutorError> {
        let input = ToolInput::new(tool_call.call_id.clone(), tool_call.arguments.clone());
        ensure_not_cancelled(cancellation)?;

        self.durable_events
            .append(DurableAgentEvent::ToolExecutionStarted {
                call_id: tool_call.call_id.clone(),
                tool_name: ToolName::new(tool_call.name.clone()),
            })
            .await?;
        let result = tool.call(input, cancellation).await;
        let payload = match result {
            Ok(output) => output.result,
            Err(error) => json!({"error": error.to_string()}),
        };
        Ok(ChatMessage::tool(tool_call.call_id.clone(), payload))
    }
}

/// Builds the model-visible result for a lookup or validation failure.
pub(crate) fn tool_error_result(tool_call: &ToolCall, error: &ToolError) -> ChatMessage {
    ChatMessage::tool(
        tool_call.call_id.clone(),
        json!({"error": error.to_string()}),
    )
}

fn ensure_not_cancelled(cancellation: &CancellationToken) -> Result<(), ToolExecutorError> {
    if cancellation.is_cancelled() {
        Err(ToolExecutorError::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use serde_json::json;
    use stratum_core::{
        CallId, ChatMessage, DangerLevel, DurableAgentEvent, ToolCall, ToolKind, ToolName, ToolSpec,
    };
    use stratum_infra::{DurableEventSink, DurableEventSinkError};
    use stratum_tools::{Tool, ToolError, ToolInput, ToolOutput, ToolRegistry};
    use tokio_util::sync::CancellationToken;

    use crate::tool_executor::{ToolExecutor, ToolExecutorError};

    #[derive(Debug, Clone, PartialEq)]
    enum Operation {
        Durable(DurableAgentEvent),
        ToolCall {
            name: ToolName,
            input: ToolInput,
            cancelled: bool,
        },
    }

    #[derive(Debug, Clone)]
    enum ToolCallResult {
        Success(serde_json::Value),
        Failure,
        Cancelled,
    }

    struct RecordingTool {
        spec: ToolSpec,
        operations: Arc<Mutex<Vec<Operation>>>,
        call_result: ToolCallResult,
    }

    impl RecordingTool {
        fn new(
            name: &str,
            operations: &Arc<Mutex<Vec<Operation>>>,
            call_result: ToolCallResult,
        ) -> Self {
            Self {
                spec: ToolSpec::builder()
                    .name(name)
                    .description("records calls")
                    .input_schema(json!({"type": "object"}))
                    .build(),
                operations: Arc::clone(operations),
                call_result,
            }
        }
    }

    #[async_trait]
    impl Tool for RecordingTool {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        fn validate(&self, _input: &ToolInput) -> Result<(), ToolError> {
            Ok(())
        }

        async fn call(
            &self,
            input: ToolInput,
            cancellation: &CancellationToken,
        ) -> Result<ToolOutput, ToolError> {
            self.operations
                .lock()
                .expect("operation lock should not be poisoned")
                .push(Operation::ToolCall {
                    name: self.spec.name.clone(),
                    input,
                    cancelled: cancellation.is_cancelled(),
                });
            match &self.call_result {
                ToolCallResult::Success(result) => Ok(ToolOutput::new(result.clone())),
                ToolCallResult::Failure => Err(ToolError::InvalidArgument {
                    name: "value",
                    reason: "test failure",
                }),
                ToolCallResult::Cancelled => Err(ToolError::Cancelled),
            }
        }
    }

    /// Minimal registry: `execute` no longer goes through it, so only `specs`
    /// carries behavior here.
    struct StubRegistry {
        specs: Vec<ToolSpec>,
    }

    #[async_trait]
    impl ToolRegistry for StubRegistry {
        fn register(
            &mut self,
            _tool: Arc<dyn Tool>,
            _tool_kind: ToolKind,
            _danger_level: DangerLevel,
        ) -> Result<(), ToolError> {
            unreachable!("the executor never registers tools")
        }

        fn authorization(
            &self,
            _name: &ToolName,
        ) -> Result<Option<(ToolKind, DangerLevel)>, ToolError> {
            Ok(None)
        }

        fn validate(&self, _name: &ToolName, _input: &ToolInput) -> Result<(), ToolError> {
            Ok(())
        }

        fn get(&self, _name: &ToolName) -> Option<Arc<dyn Tool>> {
            None
        }

        fn specs(&self) -> Vec<ToolSpec> {
            self.specs.clone()
        }

        async fn call(
            &self,
            _name: &ToolName,
            _input: ToolInput,
            _cancellation: &CancellationToken,
        ) -> Result<ToolOutput, ToolError> {
            unreachable!("the executor dispatches through the resolved tool handle")
        }
    }

    struct RecordingDurableSink {
        operations: Arc<Mutex<Vec<Operation>>>,
    }

    #[async_trait]
    impl DurableEventSink for RecordingDurableSink {
        async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
            self.operations
                .lock()
                .expect("operation lock should not be poisoned")
                .push(Operation::Durable(event));
            Ok(())
        }
    }

    struct FailingDurableSink {
        operations: Arc<Mutex<Vec<Operation>>>,
        fail_at: usize,
        attempts: AtomicUsize,
    }

    #[async_trait]
    impl DurableEventSink for FailingDurableSink {
        async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
            self.operations
                .lock()
                .expect("operation lock should not be poisoned")
                .push(Operation::Durable(event));
            let attempt = self.attempts.fetch_add(1, Ordering::Relaxed);
            if attempt == self.fail_at {
                Err(DurableEventSinkError::UnsupportedEvent {
                    event_type: "test_failure",
                })
            } else {
                Ok(())
            }
        }
    }

    fn recording_executor(operations: &Arc<Mutex<Vec<Operation>>>) -> ToolExecutor {
        ToolExecutor::new(
            Arc::new(StubRegistry { specs: Vec::new() }),
            Arc::new(RecordingDurableSink {
                operations: Arc::clone(operations),
            }),
        )
    }

    fn tool_call(name: &str) -> ToolCall {
        ToolCall {
            call_id: CallId::new("call-1"),
            name: name.to_owned(),
            arguments: json!({"value": 1}),
        }
    }

    #[tokio::test]
    async fn call_is_started_durably_before_tool_invocation() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor = recording_executor(&operations);
        let tool: Arc<dyn Tool> = Arc::new(RecordingTool::new(
            "writer",
            &operations,
            ToolCallResult::Success(json!({"ok": true})),
        ));
        let call = tool_call("writer");

        let outcome = executor
            .execute(&tool, &call, &CancellationToken::new())
            .await
            .expect("the tool should execute");

        assert_eq!(
            outcome,
            ChatMessage::tool(call.call_id.clone(), json!({"ok": true}))
        );
        assert_eq!(
            *operations
                .lock()
                .expect("operation lock should not be poisoned"),
            vec![
                Operation::Durable(DurableAgentEvent::ToolExecutionStarted {
                    call_id: call.call_id.clone(),
                    tool_name: ToolName::new("writer"),
                }),
                Operation::ToolCall {
                    name: ToolName::new("writer"),
                    input: ToolInput::new(call.call_id.clone(), call.arguments.clone()),
                    cancelled: false,
                },
            ]
        );
    }

    #[tokio::test]
    async fn tool_failure_becomes_a_model_visible_error_result() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor = recording_executor(&operations);
        let tool: Arc<dyn Tool> = Arc::new(RecordingTool::new(
            "fallible",
            &operations,
            ToolCallResult::Failure,
        ));
        let call = tool_call("fallible");

        let outcome = executor
            .execute(&tool, &call, &CancellationToken::new())
            .await
            .expect("tool domain failures are recoverable results");

        assert_eq!(
            outcome,
            ChatMessage::tool(
                call.call_id.clone(),
                json!({"error": "invalid argument value: test failure"}),
            )
        );
        assert_eq!(
            *operations
                .lock()
                .expect("operation lock should not be poisoned"),
            vec![
                Operation::Durable(DurableAgentEvent::ToolExecutionStarted {
                    call_id: call.call_id.clone(),
                    tool_name: ToolName::new("fallible"),
                }),
                Operation::ToolCall {
                    name: ToolName::new("fallible"),
                    input: ToolInput::new(call.call_id.clone(), call.arguments.clone()),
                    cancelled: false,
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_failed_execution_start_ack_prevents_tool_invocation() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor = ToolExecutor::new(
            Arc::new(StubRegistry { specs: Vec::new() }),
            Arc::new(FailingDurableSink {
                operations: Arc::clone(&operations),
                fail_at: 0,
                attempts: AtomicUsize::new(0),
            }),
        );
        let tool: Arc<dyn Tool> = Arc::new(RecordingTool::new(
            "dangerous",
            &operations,
            ToolCallResult::Success(json!({"ok": true})),
        ));
        let call = tool_call("dangerous");

        let error = executor
            .execute(&tool, &call, &CancellationToken::new())
            .await
            .expect_err("a failed required ack must stop execution");

        assert!(matches!(
            error,
            ToolExecutorError::Durability {
                source: DurableEventSinkError::UnsupportedEvent {
                    event_type: "test_failure"
                }
            }
        ));
        assert_eq!(
            *operations
                .lock()
                .expect("operation lock should not be poisoned"),
            vec![Operation::Durable(
                DurableAgentEvent::ToolExecutionStarted {
                    call_id: call.call_id.clone(),
                    tool_name: ToolName::new("dangerous"),
                }
            )]
        );
    }

    #[tokio::test]
    async fn pre_cancellation_prevents_execution_start_and_dispatch() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor = recording_executor(&operations);
        let tool: Arc<dyn Tool> = Arc::new(RecordingTool::new(
            "cancellable",
            &operations,
            ToolCallResult::Cancelled,
        ));
        let call = tool_call("cancellable");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let error = executor
            .execute(&tool, &call, &cancellation)
            .await
            .expect_err("pre-cancellation should stop before execution starts");

        assert!(matches!(error, ToolExecutorError::Cancelled));
        assert!(
            operations
                .lock()
                .expect("operation lock should not be poisoned")
                .is_empty()
        );
    }

    #[test]
    fn specs_are_returned_unchanged_from_registry() {
        let specs = vec![
            ToolSpec::builder()
                .name("alpha")
                .description("first tool")
                .input_schema(json!({"type": "object"}))
                .build(),
            ToolSpec::builder()
                .name("beta")
                .description("second tool")
                .input_schema(json!({"type": "string"}))
                .build(),
        ];
        let executor = ToolExecutor::new(
            Arc::new(StubRegistry {
                specs: specs.clone(),
            }),
            Arc::new(RecordingDurableSink {
                operations: Arc::new(Mutex::new(Vec::new())),
            }),
        );

        assert_eq!(executor.specs(), specs);
    }
}
