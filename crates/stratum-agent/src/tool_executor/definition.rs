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
/// durable `ToolExecutionStarted` boundary, and the call itself. Execution
/// policy (argument transformation, approval, blocking) lives in the agent
/// loop's hook runtime.
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

    /// Processes one provider tool call.
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
    pub async fn execute(
        &self,
        tool_call: &ToolCall,
        cancellation: &CancellationToken,
    ) -> Result<ChatMessage, ToolExecutorError> {
        let tool_name = ToolName::new(tool_call.name.clone());
        let input = ToolInput::new(tool_call.call_id.clone(), tool_call.arguments.clone());
        if let Err(error) = self.registry.authorization(&tool_name) {
            return Ok(tool_error_result(tool_call, &error));
        }
        if let Err(error) = self.registry.validate(&tool_name, &input) {
            return Ok(tool_error_result(tool_call, &error));
        }
        ensure_not_cancelled(cancellation)?;

        self.durable_events
            .append(DurableAgentEvent::ToolExecutionStarted {
                call_id: tool_call.call_id.clone(),
                tool_name: tool_name.clone(),
            })
            .await?;
        let result = self.registry.call(&tool_name, input, cancellation).await;
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
    use stratum_tools::{
        BuiltinToolRegistry, EchoTool, Tool, ToolError, ToolInput, ToolOutput, ToolPermissionMode,
        ToolRegistry,
    };
    use tokio_util::sync::CancellationToken;

    use crate::tool_executor::{ToolExecutor, ToolExecutorError};

    #[derive(Debug, Clone, PartialEq)]
    enum Operation {
        Authorization(ToolName),
        Durable(DurableAgentEvent),
        ToolCall {
            name: ToolName,
            input: ToolInput,
            cancelled: bool,
        },
    }

    #[derive(Debug, Clone)]
    enum RegistryCallResult {
        Success(serde_json::Value),
        Failure,
        Cancelled,
    }

    struct RecordingRegistry {
        operations: Arc<Mutex<Vec<Operation>>>,
        missing: bool,
        approval: Option<(ToolKind, DangerLevel)>,
        specs: Vec<ToolSpec>,
        call_result: RegistryCallResult,
    }

    #[async_trait]
    impl ToolRegistry for RecordingRegistry {
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
            name: &ToolName,
        ) -> Result<Option<(ToolKind, DangerLevel)>, ToolError> {
            self.operations
                .lock()
                .expect("operation lock should not be poisoned")
                .push(Operation::Authorization(name.clone()));
            if self.missing {
                Err(ToolError::ToolNotFound { name: name.clone() })
            } else {
                Ok(self.approval)
            }
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
            name: &ToolName,
            input: ToolInput,
            cancellation: &CancellationToken,
        ) -> Result<ToolOutput, ToolError> {
            self.operations
                .lock()
                .expect("operation lock should not be poisoned")
                .push(Operation::ToolCall {
                    name: name.clone(),
                    input,
                    cancelled: cancellation.is_cancelled(),
                });
            match &self.call_result {
                RegistryCallResult::Success(result) => Ok(ToolOutput::new(result.clone())),
                RegistryCallResult::Failure => Err(ToolError::InvalidArgument {
                    name: "value",
                    reason: "test failure",
                }),
                RegistryCallResult::Cancelled => Err(ToolError::Cancelled),
            }
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

    fn recording_executor(
        operations: &Arc<Mutex<Vec<Operation>>>,
        missing: bool,
        call_result: RegistryCallResult,
    ) -> ToolExecutor {
        ToolExecutor::new(
            Arc::new(RecordingRegistry {
                operations: Arc::clone(operations),
                missing,
                approval: None,
                specs: Vec::new(),
                call_result,
            }),
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
    async fn missing_tool_returns_error_message_without_execution_start() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor =
            recording_executor(&operations, true, RegistryCallResult::Success(json!(null)));
        let call = tool_call("missing");

        let outcome = executor
            .execute(&call, &CancellationToken::new())
            .await
            .expect("missing tools are recoverable tool results");

        assert_eq!(
            outcome,
            ChatMessage::tool(
                call.call_id.clone(),
                json!({"error": "tool not found: missing"}),
            )
        );
        assert_eq!(
            *operations
                .lock()
                .expect("operation lock should not be poisoned"),
            vec![Operation::Authorization(ToolName::new("missing"))]
        );
    }

    #[tokio::test]
    async fn invalid_builtin_input_returns_error_before_execution_start() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let mut registry = BuiltinToolRegistry::new(ToolPermissionMode::RequireApproval);
        registry
            .register(Arc::new(EchoTool::new()), ToolKind::Read, DangerLevel::Low)
            .expect("echo tool should register");
        let executor = ToolExecutor::new(
            Arc::new(registry),
            Arc::new(RecordingDurableSink {
                operations: Arc::clone(&operations),
            }),
        );
        let call = ToolCall {
            call_id: CallId::new("call-invalid"),
            name: "echo".to_owned(),
            arguments: json!(42),
        };

        let outcome = executor
            .execute(&call, &CancellationToken::new())
            .await
            .expect("invalid arguments should remain a recoverable tool result");

        assert_eq!(
            outcome,
            ChatMessage::tool(
                call.call_id,
                json!({"error": "invalid argument arguments: must be an object"}),
            )
        );
        assert!(
            operations
                .lock()
                .expect("operation lock should not be poisoned")
                .is_empty(),
            "validation must precede the execution-start event"
        );
    }

    #[tokio::test]
    async fn call_is_started_durably_before_tool_invocation() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor = ToolExecutor::new(
            Arc::new(RecordingRegistry {
                operations: Arc::clone(&operations),
                missing: false,
                approval: Some((ToolKind::Write, DangerLevel::Medium)),
                specs: Vec::new(),
                call_result: RegistryCallResult::Success(json!({"ok": true})),
            }),
            Arc::new(RecordingDurableSink {
                operations: Arc::clone(&operations),
            }),
        );
        let call = tool_call("writer");

        let outcome = executor
            .execute(&call, &CancellationToken::new())
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
                Operation::Authorization(ToolName::new("writer")),
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
        let executor = recording_executor(&operations, false, RegistryCallResult::Failure);
        let call = tool_call("fallible");

        let outcome = executor
            .execute(&call, &CancellationToken::new())
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
                Operation::Authorization(ToolName::new("fallible")),
                Operation::Durable(DurableAgentEvent::ToolExecutionStarted {
                    call_id: call.call_id.clone(),
                    tool_name: ToolName::new("fallible"),
                }),
                Operation::ToolCall {
                    name: ToolName::new("fallible"),
                    input: ToolInput::new(call.call_id, call.arguments),
                    cancelled: false,
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_failed_execution_start_ack_prevents_tool_invocation() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor = ToolExecutor::new(
            Arc::new(RecordingRegistry {
                operations: Arc::clone(&operations),
                missing: false,
                approval: None,
                specs: Vec::new(),
                call_result: RegistryCallResult::Success(json!({"ok": true})),
            }),
            Arc::new(FailingDurableSink {
                operations: Arc::clone(&operations),
                fail_at: 0,
                attempts: AtomicUsize::new(0),
            }),
        );
        let call = tool_call("dangerous");

        let error = executor
            .execute(&call, &CancellationToken::new())
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
            vec![
                Operation::Authorization(ToolName::new("dangerous")),
                Operation::Durable(DurableAgentEvent::ToolExecutionStarted {
                    call_id: call.call_id.clone(),
                    tool_name: ToolName::new("dangerous"),
                }),
            ]
        );
    }

    #[tokio::test]
    async fn pre_cancellation_prevents_execution_start_and_dispatch() {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor = recording_executor(&operations, false, RegistryCallResult::Cancelled);
        let call = tool_call("cancellable");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let error = executor
            .execute(&call, &cancellation)
            .await
            .expect_err("pre-cancellation should stop before execution starts");

        assert!(matches!(error, ToolExecutorError::Cancelled));
        assert_eq!(
            *operations
                .lock()
                .expect("operation lock should not be poisoned"),
            vec![Operation::Authorization(ToolName::new("cancellable"))]
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
        let operations = Arc::new(Mutex::new(Vec::new()));
        let executor = ToolExecutor::new(
            Arc::new(RecordingRegistry {
                operations: Arc::clone(&operations),
                missing: false,
                approval: None,
                specs: specs.clone(),
                call_result: RegistryCallResult::Success(json!(null)),
            }),
            Arc::new(RecordingDurableSink { operations }),
        );

        assert_eq!(executor.specs(), specs);
    }
}
