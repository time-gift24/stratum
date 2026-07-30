use std::{
    collections::VecDeque,
    future::pending,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use futures_util::stream;
use serde_json::{Value, json};
use stratum_agent::{
    AfterToolCallDecision, AfterToolCallInput, AgentLoop, AgentLoopError, AllowAllToolApproval,
    BeforeToolCallDecision, BeforeToolCallInput, HookControl, HookRuntime, LoopCompletionReason,
    LoopContext, LoopLimits, PrepareNextTurnDecision, PrepareNextTurnInput, ToolApproval,
    ToolApprovalError, ToolApprovalRequest, ToolExecutor, TransformContextDecision,
    TransformContextInput,
};
use stratum_core::{
    AgentTelemetryEvent, ApprovalDecision, CallId, ChatContent, ChatMessage, ChatRole, DangerLevel,
    DurableAgentEvent, HookFailure, HookPoint, ModelId, ToolCall, ToolCallDelta, ToolKind,
    ToolName, ToolSpec,
};
use stratum_infra::{DurableEventSink, DurableEventSinkError, TelemetryEventSink};
use stratum_llm::{
    ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, FinishReason, LlmError, LlmProvider,
};
use stratum_tools::{
    BuiltinToolRegistry, Tool, ToolError, ToolInput, ToolOutput, ToolPermissionMode, ToolRegistry,
};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
enum Operation {
    Durable(DurableAgentEvent),
    ChatStream(ChatRequest),
    ToolCall { name: ToolName, input: ToolInput },
    Hook(HookCall),
}

#[derive(Debug, Clone, PartialEq)]
enum HookCall {
    TransformContext {
        iteration: u64,
        context: LoopContext,
    },
    BeforeToolCall {
        iteration: u64,
        tool_call: ToolCall,
    },
    AfterToolCall {
        iteration: u64,
        tool_call: ToolCall,
        result: ChatMessage,
    },
    PrepareNextTurn {
        iteration: u64,
        committed_messages: usize,
    },
}

#[derive(Debug, Clone)]
enum TransformBehavior {
    Unchanged,
    Replace(LoopContext),
    Fail(HookFailure),
    Pending,
    CancelThenPending,
}

#[derive(Debug, Clone)]
enum BeforeBehavior {
    Continue,
    Modify(Value),
    Block(String),
    Fail(HookFailure),
    Pending,
    CancelThenPending,
}

#[derive(Debug, Clone)]
enum AfterBehavior {
    Keep,
    Replace(Value),
    Fail(HookFailure),
    Pending,
    CancelThenPending,
}

#[derive(Debug, Clone)]
enum PrepareBehavior {
    Continue,
    Stop,
    Inject(Vec<ChatMessage>),
    Fail(HookFailure),
    Pending,
    CancelThenPending,
}

/// Control snapshot observed by one recorded hook invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObservedControl {
    point: HookPoint,
    token_cancelled: bool,
    deadline_in_future: bool,
}

struct RecordingHookRuntime {
    operations: Arc<Mutex<Vec<Operation>>>,
    controls: Arc<Mutex<Vec<ObservedControl>>>,
    transforms: Mutex<VecDeque<TransformBehavior>>,
    befores: Mutex<VecDeque<BeforeBehavior>>,
    afters: Mutex<VecDeque<AfterBehavior>>,
    prepares: Mutex<VecDeque<PrepareBehavior>>,
}

impl RecordingHookRuntime {
    fn new(operations: &Arc<Mutex<Vec<Operation>>>) -> Self {
        Self {
            operations: Arc::clone(operations),
            controls: Arc::new(Mutex::new(Vec::new())),
            transforms: Mutex::new(VecDeque::new()),
            befores: Mutex::new(VecDeque::new()),
            afters: Mutex::new(VecDeque::new()),
            prepares: Mutex::new(VecDeque::new()),
        }
    }

    fn with_transforms(self, behaviors: impl IntoIterator<Item = TransformBehavior>) -> Self {
        self.transforms
            .lock()
            .expect("behavior lock should not be poisoned")
            .extend(behaviors);
        self
    }

    fn with_befores(self, behaviors: impl IntoIterator<Item = BeforeBehavior>) -> Self {
        self.befores
            .lock()
            .expect("behavior lock should not be poisoned")
            .extend(behaviors);
        self
    }

    fn with_afters(self, behaviors: impl IntoIterator<Item = AfterBehavior>) -> Self {
        self.afters
            .lock()
            .expect("behavior lock should not be poisoned")
            .extend(behaviors);
        self
    }

    fn with_prepares(self, behaviors: impl IntoIterator<Item = PrepareBehavior>) -> Self {
        self.prepares
            .lock()
            .expect("behavior lock should not be poisoned")
            .extend(behaviors);
        self
    }

    fn record_control(&self, point: HookPoint, control: &HookControl) {
        let remaining = control
            .deadline()
            .saturating_duration_since(tokio::time::Instant::now());
        self.controls
            .lock()
            .expect("control lock should not be poisoned")
            .push(ObservedControl {
                point,
                token_cancelled: control.cancellation().is_cancelled(),
                deadline_in_future: remaining > Duration::ZERO,
            });
    }

    fn record(&self, call: HookCall) {
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::Hook(call));
    }
}

#[async_trait]
impl HookRuntime for RecordingHookRuntime {
    async fn transform_context<'a>(
        &self,
        input: TransformContextInput<'a>,
        control: HookControl,
    ) -> Result<TransformContextDecision, HookFailure> {
        self.record_control(HookPoint::TransformContext, &control);
        self.record(HookCall::TransformContext {
            iteration: input.iteration,
            context: input.context.clone(),
        });
        let behavior = self
            .transforms
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .unwrap_or(TransformBehavior::Unchanged);
        match behavior {
            TransformBehavior::Unchanged => Ok(TransformContextDecision::Unchanged),
            TransformBehavior::Replace(context) => {
                Ok(TransformContextDecision::Replace { context })
            }
            TransformBehavior::Fail(failure) => Err(failure),
            TransformBehavior::Pending => pending().await,
            TransformBehavior::CancelThenPending => {
                control.cancellation().cancel();
                pending().await
            }
        }
    }

    async fn before_tool_call<'a>(
        &self,
        input: BeforeToolCallInput<'a>,
        control: HookControl,
    ) -> Result<BeforeToolCallDecision, HookFailure> {
        self.record_control(HookPoint::BeforeToolCall, &control);
        self.record(HookCall::BeforeToolCall {
            iteration: input.iteration,
            tool_call: input.tool_call.clone(),
        });
        let behavior = self
            .befores
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .unwrap_or(BeforeBehavior::Continue);
        match behavior {
            BeforeBehavior::Continue => Ok(BeforeToolCallDecision::Continue),
            BeforeBehavior::Modify(arguments) => {
                Ok(BeforeToolCallDecision::ModifyArguments { arguments })
            }
            BeforeBehavior::Block(reason) => Ok(BeforeToolCallDecision::Block { reason }),
            BeforeBehavior::Fail(failure) => Err(failure),
            BeforeBehavior::Pending => pending().await,
            BeforeBehavior::CancelThenPending => {
                control.cancellation().cancel();
                pending().await
            }
        }
    }

    async fn after_tool_call<'a>(
        &self,
        input: AfterToolCallInput<'a>,
        control: HookControl,
    ) -> Result<AfterToolCallDecision, HookFailure> {
        self.record_control(HookPoint::AfterToolCall, &control);
        self.record(HookCall::AfterToolCall {
            iteration: input.iteration,
            tool_call: input.tool_call.clone(),
            result: input.result.clone(),
        });
        let behavior = self
            .afters
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .unwrap_or(AfterBehavior::Keep);
        match behavior {
            AfterBehavior::Keep => Ok(AfterToolCallDecision::Keep),
            AfterBehavior::Replace(result) => Ok(AfterToolCallDecision::ReplaceResult { result }),
            AfterBehavior::Fail(failure) => Err(failure),
            AfterBehavior::Pending => pending().await,
            AfterBehavior::CancelThenPending => {
                control.cancellation().cancel();
                pending().await
            }
        }
    }

    async fn prepare_next_turn<'a>(
        &self,
        input: PrepareNextTurnInput<'a>,
        control: HookControl,
    ) -> Result<PrepareNextTurnDecision, HookFailure> {
        self.record_control(HookPoint::PrepareNextTurn, &control);
        self.record(HookCall::PrepareNextTurn {
            iteration: input.iteration,
            committed_messages: input.context.messages.len(),
        });
        let behavior = self
            .prepares
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .unwrap_or(PrepareBehavior::Continue);
        match behavior {
            PrepareBehavior::Continue => Ok(PrepareNextTurnDecision::Continue),
            PrepareBehavior::Stop => Ok(PrepareNextTurnDecision::Stop),
            PrepareBehavior::Inject(messages) => Ok(PrepareNextTurnDecision::Inject { messages }),
            PrepareBehavior::Fail(failure) => Err(failure),
            PrepareBehavior::Pending => pending().await,
            PrepareBehavior::CancelThenPending => {
                control.cancellation().cancel();
                pending().await
            }
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

struct CancellingDurableSink {
    operations: Arc<Mutex<Vec<Operation>>>,
    cancellation: CancellationToken,
    trigger_on_tool_result: bool,
}

#[async_trait]
impl DurableEventSink for CancellingDurableSink {
    async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
        let should_cancel = match &event {
            DurableAgentEvent::MessageAppended { message } if self.trigger_on_tool_result => {
                message.role == ChatRole::Tool
            }
            DurableAgentEvent::MessageAppended { message } => message.role == ChatRole::Assistant,
            _ => false,
        };
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::Durable(event));
        if should_cancel {
            self.cancellation.cancel();
        }
        Ok(())
    }
}

struct NullTelemetrySink;

impl TelemetryEventSink for NullTelemetrySink {
    fn emit(&self, _event: AgentTelemetryEvent) {}
}

struct ScriptedProvider {
    operations: Arc<Mutex<Vec<Operation>>>,
    behaviors: Mutex<VecDeque<Vec<ChatStreamEvent>>>,
    model: ModelId,
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    fn model_id(&self) -> ModelId {
        self.model.clone()
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
        Err(LlmError::UnsupportedCapability("chat"))
    }

    async fn chat_stream(&self, request: ChatRequest) -> Result<ChatStream, LlmError> {
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::ChatStream(request));
        let events = self
            .behaviors
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .ok_or(LlmError::MockExhausted)?;
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

struct EchoRecordingTool {
    spec: ToolSpec,
    operations: Arc<Mutex<Vec<Operation>>>,
}

#[async_trait]
impl Tool for EchoRecordingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn validate(&self, _input: &ToolInput) -> Result<(), ToolError> {
        Ok(())
    }

    async fn call(
        &self,
        input: ToolInput,
        _cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::ToolCall {
                name: self.spec.name.clone(),
                input: input.clone(),
            });
        Ok(ToolOutput::new(json!({"echo": input.arguments})))
    }
}

struct FailingToolApproval;

#[async_trait]
impl ToolApproval for FailingToolApproval {
    async fn request(
        &self,
        _request: ToolApprovalRequest,
        _cancellation: &CancellationToken,
    ) -> Result<ApprovalDecision, ToolApprovalError> {
        Err(ToolApprovalError::interaction(std::io::Error::other(
            "scripted approval failure",
        )))
    }
}

fn tool_delta(index: usize, call_id: &str, name: &str, arguments: &Value) -> ChatStreamEvent {
    ChatStreamEvent::ToolCallDelta(ToolCallDelta {
        index,
        call_id: Some(CallId::from(call_id)),
        name: Some(name.to_owned()),
        arguments_delta: arguments.to_string(),
    })
}

fn tool_call_turn(call_id: &str, name: &str, arguments: Value) -> Vec<ChatStreamEvent> {
    vec![
        tool_delta(0, call_id, name, &arguments),
        ChatStreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
            usage: None,
        },
    ]
}

fn stop_turn(text: &str) -> Vec<ChatStreamEvent> {
    vec![
        ChatStreamEvent::TextDelta {
            delta: text.to_owned(),
        },
        ChatStreamEvent::Finished {
            finish_reason: FinishReason::Stop,
            usage: None,
        },
    ]
}

fn echo_registry(
    operations: &Arc<Mutex<Vec<Operation>>>,
    permission_mode: ToolPermissionMode,
) -> Arc<dyn ToolRegistry> {
    let mut registry = BuiltinToolRegistry::new(permission_mode);
    registry
        .register(
            Arc::new(EchoRecordingTool {
                spec: ToolSpec::builder()
                    .name("echo")
                    .description("records calls")
                    .input_schema(json!({"type": "object"}))
                    .build(),
                operations: Arc::clone(operations),
            }),
            ToolKind::Read,
            DangerLevel::Low,
        )
        .expect("echo tool should register");
    Arc::new(registry)
}

fn build_loop(
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
    limits: LoopLimits,
    registry: Arc<dyn ToolRegistry>,
    approval: Arc<dyn ToolApproval>,
    durable: Arc<dyn DurableEventSink>,
    hook_runtime: Option<Arc<dyn HookRuntime>>,
    operations: &Arc<Mutex<Vec<Operation>>>,
) -> AgentLoop {
    let provider: Arc<dyn LlmProvider> = Arc::new(ScriptedProvider {
        operations: Arc::clone(operations),
        behaviors: Mutex::new(behaviors),
        model: "scripted:test-model"
            .parse()
            .expect("static model id should parse"),
    });
    let builder = AgentLoop::builder()
        .llm_provider(provider)
        .tool_executor(ToolExecutor::new(registry, approval, durable))
        .telemetry(Arc::new(NullTelemetrySink))
        .limits(limits);
    let builder = match hook_runtime {
        Some(runtime) => builder.hook_runtime(runtime),
        None => builder,
    };
    builder
        .build()
        .expect("all agent loop fields should be present")
}

fn default_loop(
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
    operations: &Arc<Mutex<Vec<Operation>>>,
) -> AgentLoop {
    build_loop(
        behaviors,
        LoopLimits::new(8, 4),
        echo_registry(operations, ToolPermissionMode::Allow),
        Arc::new(AllowAllToolApproval),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(operations),
        }),
        None,
        operations,
    )
}

fn hooked_loop(
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
    runtime: RecordingHookRuntime,
    operations: &Arc<Mutex<Vec<Operation>>>,
) -> (AgentLoop, Arc<Mutex<Vec<ObservedControl>>>) {
    let controls = Arc::clone(&runtime.controls);
    let agent_loop = build_loop(
        behaviors,
        LoopLimits::new(8, 4),
        echo_registry(operations, ToolPermissionMode::Allow),
        Arc::new(AllowAllToolApproval),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(operations),
        }),
        Some(Arc::new(runtime)),
        operations,
    );
    (agent_loop, controls)
}

fn snapshot(operations: &Arc<Mutex<Vec<Operation>>>) -> Vec<Operation> {
    operations
        .lock()
        .expect("operation lock should not be poisoned")
        .clone()
}

fn hook_calls(operations: &[Operation]) -> Vec<HookCall> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::Hook(call) => Some(call.clone()),
            _ => None,
        })
        .collect()
}

fn durable_events(operations: &[Operation]) -> Vec<DurableAgentEvent> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::Durable(event) => Some(event.clone()),
            _ => None,
        })
        .collect()
}

fn chat_requests(operations: &[Operation]) -> Vec<ChatRequest> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::ChatStream(request) => Some(request.clone()),
            _ => None,
        })
        .collect()
}

async fn run_once(
    agent_loop: &AgentLoop,
    cancellation: CancellationToken,
) -> Result<stratum_agent::LoopOutcome, AgentLoopError> {
    agent_loop
        .run(
            LoopContext::new("be precise"),
            vec![ChatMessage::user("use echo")],
            cancellation,
        )
        .await
}

// 3.1: the default builder keeps the pre-hook kernel behavior unchanged.
#[tokio::test]
async fn default_noop_runtime_preserves_pre_hook_behavior() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let agent_loop = default_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    assert_eq!(
        outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );
    let call = ToolCall {
        call_id: CallId::from("call-1"),
        name: "echo".to_owned(),
        arguments: json!({"value": "one"}),
    };
    let assistant = ChatMessage::assistant("").with_tool_calls(vec![call.clone()]);
    let result = ChatMessage::tool(call.call_id.clone(), json!({"echo": call.arguments}));
    assert_eq!(
        outcome.new_messages,
        vec![
            ChatMessage::user("use echo"),
            assistant.clone(),
            result.clone(),
            ChatMessage::assistant("done"),
        ]
    );
    let recorded = snapshot(&operations);
    assert!(hook_calls(&recorded).is_empty());
    let requests = chat_requests(&recorded);
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].messages,
        vec![
            ChatMessage::system("be precise"),
            ChatMessage::user("use echo"),
        ]
    );
    assert_eq!(
        requests[1].messages,
        vec![
            ChatMessage::system("be precise"),
            ChatMessage::user("use echo"),
            assistant,
            result,
        ]
    );
    let durable = durable_events(&recorded);
    assert_eq!(
        durable
            .iter()
            .map(DurableAgentEvent::event_type)
            .collect::<Vec<_>>(),
        vec![
            "loop_started",
            "message_appended",
            "message_appended",
            "tool_execution_started",
            "message_appended",
            "iteration_completed",
            "message_appended",
            "iteration_completed",
            "loop_finished",
        ]
    );
    assert!(matches!(
        durable.last(),
        Some(DurableAgentEvent::LoopFinished { finish_reason, .. })
            if finish_reason == "stop"
    ));
}

// 3.1: an injected runtime is called at all four points in the fixed order.
#[tokio::test]
async fn custom_runtime_is_invoked_at_all_four_points_in_order() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations);
    let (agent_loop, controls) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    assert_eq!(
        outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );
    let recorded = snapshot(&operations);
    assert_eq!(
        hook_calls(&recorded),
        vec![
            HookCall::TransformContext {
                iteration: 0,
                context: LoopContext::new("be precise")
                    .with_messages(vec![ChatMessage::user("use echo")]),
            },
            HookCall::BeforeToolCall {
                iteration: 0,
                tool_call: ToolCall {
                    call_id: CallId::from("call-1"),
                    name: "echo".to_owned(),
                    arguments: json!({"value": "one"}),
                },
            },
            HookCall::AfterToolCall {
                iteration: 0,
                tool_call: ToolCall {
                    call_id: CallId::from("call-1"),
                    name: "echo".to_owned(),
                    arguments: json!({"value": "one"}),
                },
                result: ChatMessage::tool(
                    CallId::from("call-1"),
                    json!({"echo": {"value": "one"}}),
                ),
            },
            HookCall::PrepareNextTurn {
                iteration: 0,
                committed_messages: 3,
            },
            HookCall::TransformContext {
                iteration: 1,
                context: LoopContext::new("be precise").with_messages(vec![
                    ChatMessage::user("use echo"),
                    ChatMessage::assistant("").with_tool_calls(vec![ToolCall {
                        call_id: CallId::from("call-1"),
                        name: "echo".to_owned(),
                        arguments: json!({"value": "one"}),
                    }]),
                    ChatMessage::tool(CallId::from("call-1"), json!({"echo": {"value": "one"}})),
                ]),
            },
        ]
    );
    let controls = controls
        .lock()
        .expect("control lock should not be poisoned");
    assert_eq!(controls.len(), 5);
    for (index, observed) in controls.iter().enumerate() {
        let expected = [
            HookPoint::TransformContext,
            HookPoint::BeforeToolCall,
            HookPoint::AfterToolCall,
            HookPoint::PrepareNextTurn,
            HookPoint::TransformContext,
        ][index];
        assert_eq!(observed.point, expected);
        assert!(
            !observed.token_cancelled,
            "hook {index} should observe a live token"
        );
        assert!(
            observed.deadline_in_future,
            "hook {index} should get a future deadline"
        );
    }
}

// 3.2: a replaced context is used for the current request only.
#[tokio::test]
async fn transform_replace_is_request_scoped_and_never_committed() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let replacement = LoopContext::new("replaced system")
        .with_messages(vec![ChatMessage::user("replaced history")]);
    let runtime = RecordingHookRuntime::new(&operations)
        .with_transforms([TransformBehavior::Replace(replacement.clone())]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let recorded = snapshot(&operations);
    let requests = chat_requests(&recorded);
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].messages,
        vec![
            ChatMessage::system("replaced system"),
            ChatMessage::user("replaced history"),
        ]
    );
    assert_eq!(
        requests[1].messages.first(),
        Some(&ChatMessage::system("be precise"))
    );
    assert!(
        !requests[1]
            .messages
            .contains(&ChatMessage::user("replaced history")),
        "the replacement must not leak into the next committed view"
    );
    assert!(
        !outcome
            .new_messages
            .contains(&ChatMessage::user("replaced history"))
    );
    assert!(durable_events(&recorded).iter().all(|event| !matches!(
        event,
        DurableAgentEvent::MessageAppended { message }
            if message == &ChatMessage::user("replaced history")
    )));
}

// 3.2: modified arguments keep the call identity and reach the executor.
#[tokio::test]
async fn before_modify_arguments_preserves_call_identity() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_befores([BeforeBehavior::Modify(json!({"value": "modified"}))]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "original"})),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let recorded = snapshot(&operations);
    let tool_calls = recorded
        .iter()
        .filter_map(|operation| match operation {
            Operation::ToolCall { name, input } => Some((name.clone(), input.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tool_calls,
        vec![(
            ToolName::new("echo"),
            ToolInput::new(CallId::from("call-1"), json!({"value": "modified"})),
        )]
    );
    // The after hook observes the call as executed, with modified arguments.
    assert!(hook_calls(&recorded).contains(&HookCall::AfterToolCall {
        iteration: 0,
        tool_call: ToolCall {
            call_id: CallId::from("call-1"),
            name: "echo".to_owned(),
            arguments: json!({"value": "modified"}),
        },
        result: ChatMessage::tool(
            CallId::from("call-1"),
            json!({"echo": {"value": "modified"}}),
        ),
    }));
    assert_eq!(
        outcome.new_messages[2],
        ChatMessage::tool(
            CallId::from("call-1"),
            json!({"echo": {"value": "modified"}})
        )
    );
}

// 3.2 + 3.4: a block skips approval and execution, produces the fixed
// hook_blocked result, and still passes through the after hook.
#[tokio::test]
async fn before_block_skips_approval_and_execution_but_reaches_after() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_befores([BeforeBehavior::Block("policy denied".to_owned())]);
    let controls = Arc::clone(&runtime.controls);
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        LoopLimits::new(8, 4),
        echo_registry(&operations, ToolPermissionMode::RequireApproval),
        Arc::new(FailingToolApproval),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        Some(Arc::new(runtime)),
        &operations,
    );
    let _ = controls;

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("a blocked call should stay a model-visible result");

    let blocked = ChatMessage::tool(
        CallId::from("call-1"),
        json!({"error": {"code": "hook_blocked", "message": "policy denied"}}),
    );
    assert_eq!(outcome.new_messages[2], blocked);
    let recorded = snapshot(&operations);
    assert!(hook_calls(&recorded).contains(&HookCall::AfterToolCall {
        iteration: 0,
        tool_call: ToolCall {
            call_id: CallId::from("call-1"),
            name: "echo".to_owned(),
            arguments: json!({"value": "one"}),
        },
        result: blocked.clone(),
    }));
    assert!(
        !recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. })),
        "a blocked call must never dispatch"
    );
    let durable = durable_events(&recorded);
    assert!(!durable.iter().any(|event| matches!(
        event,
        DurableAgentEvent::ToolApprovalRequested { .. }
            | DurableAgentEvent::ToolApprovalResolved { .. }
            | DurableAgentEvent::ToolExecutionStarted { .. }
    )));
    let requests = chat_requests(&recorded);
    assert_eq!(requests[1].messages.last(), Some(&blocked));
}

// 3.2: a replaced result keeps the tool role and call identity.
#[tokio::test]
async fn after_replace_result_preserves_role_and_call_identity() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_afters([AfterBehavior::Replace(json!({"redacted": true}))]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let replaced = ChatMessage::tool(CallId::from("call-1"), json!({"redacted": true}));
    assert_eq!(outcome.new_messages[2], replaced);
    let recorded = snapshot(&operations);
    assert!(
        durable_events(&recorded).contains(&DurableAgentEvent::MessageAppended {
            message: replaced.clone(),
        })
    );
    assert!(!durable_events(&recorded).iter().any(|event| matches!(
        event,
        DurableAgentEvent::MessageAppended { message }
            if message.content == ChatContent::Json(json!({"echo": {"value": "one"}}))
    )));
    let requests = chat_requests(&recorded);
    assert_eq!(requests[1].messages.last(), Some(&replaced));
}

// 3.2: prepare stop commits the iteration and finishes as hook_stopped.
#[tokio::test]
async fn prepare_stop_commits_iteration_and_finishes_hook_stopped() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations).with_prepares([PrepareBehavior::Stop]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({"value": "one"}))]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("hook stop is a successful terminal");

    assert_eq!(outcome.completion, LoopCompletionReason::HookStopped);
    let recorded = snapshot(&operations);
    assert_eq!(chat_requests(&recorded).len(), 1);
    let durable = durable_events(&recorded);
    let iteration_position = durable
        .iter()
        .position(|event| {
            matches!(
                event,
                DurableAgentEvent::IterationCompleted { iteration: 0, .. }
            )
        })
        .expect("the iteration boundary must be committed");
    let finished_position = durable
        .iter()
        .position(|event| {
            matches!(
                event,
                DurableAgentEvent::LoopFinished { finish_reason, .. }
                    if finish_reason == "hook_stopped"
            )
        })
        .expect("loop must finish as hook_stopped");
    assert!(iteration_position < finished_position);
}

// 3.2 + 3.4: injected messages reach only the next request, exactly once.
#[tokio::test]
async fn prepare_inject_is_consumed_once_by_the_next_request_only() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let injected = ChatMessage::user("hook injected note");
    let runtime = RecordingHookRuntime::new(&operations)
        .with_prepares([PrepareBehavior::Inject(vec![injected.clone()])]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            tool_call_turn("call-2", "echo", json!({"value": "two"})),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let recorded = snapshot(&operations);
    let requests = chat_requests(&recorded);
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].messages.last(), Some(&injected));
    assert!(
        !requests[2].messages.contains(&injected),
        "an injected message is consumed by exactly one request"
    );
    assert!(!outcome.new_messages.contains(&injected));
    assert!(!durable_events(&recorded).iter().any(|event| matches!(
        event,
        DurableAgentEvent::MessageAppended { message } if message == &injected
    )));
    // The transform hook observes the inject inside the request view.
    let transform_views = hook_calls(&recorded)
        .into_iter()
        .filter_map(|call| match call {
            HookCall::TransformContext {
                iteration: 1,
                context,
            } => Some(context),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(transform_views.len(), 1);
    assert_eq!(transform_views[0].messages.last(), Some(&injected));
}

// 3.3: runtime failures fail closed at every hook point.
#[tokio::test]
async fn runtime_failure_fails_closed_at_every_hook_point() {
    // transform_context: no model request starts.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_transforms([TransformBehavior::Fail(HookFailure::HandlerFailed)]);
    let (agent_loop, _) = hooked_loop(VecDeque::from([stop_turn("done")]), runtime, &operations);
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("transform failure should stop the loop");
    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::TransformContext,
            failure: HookFailure::HandlerFailed,
        }
    ));
    let recorded = snapshot(&operations);
    assert!(chat_requests(&recorded).is_empty());

    // before_tool_call: the tool is neither approved nor executed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_befores([BeforeBehavior::Fail(HookFailure::HandlerFailed)]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("before failure should stop the loop");
    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::BeforeToolCall,
            failure: HookFailure::HandlerFailed,
        }
    ));
    let recorded = snapshot(&operations);
    assert!(
        !recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. }))
    );
    assert!(
        !durable_events(&recorded)
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );

    // after_tool_call: no result is committed, the durable start remains.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_afters([AfterBehavior::Fail(HookFailure::HandlerFailed)]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("after failure should stop the loop");
    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::AfterToolCall,
            failure: HookFailure::HandlerFailed,
        }
    ));
    let durable = durable_events(&snapshot(&operations));
    assert!(
        durable
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );
    assert!(!durable.iter().any(|event| matches!(
        event,
        DurableAgentEvent::MessageAppended { message } if message.role == ChatRole::Tool
    )));
    assert!(
        !durable
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::IterationCompleted { .. }))
    );

    // prepare_next_turn: the iteration boundary is not committed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_prepares([PrepareBehavior::Fail(HookFailure::HandlerFailed)]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("prepare failure should stop the loop");
    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::PrepareNextTurn,
            failure: HookFailure::HandlerFailed,
        }
    ));
    let durable = durable_events(&snapshot(&operations));
    assert!(durable.iter().any(|event| matches!(
        event,
        DurableAgentEvent::MessageAppended { message } if message.role == ChatRole::Tool
    )));
    assert!(
        !durable
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::IterationCompleted { .. }))
    );
}

// 3.3: a missed deadline maps to a typed timeout at every hook point.
#[tokio::test]
async fn hook_deadline_maps_to_typed_timeout_at_every_point() {
    let limits = LoopLimits::new(8, 4).with_hook_timeout(Duration::from_millis(50));
    type Configurer = Box<dyn FnOnce(RecordingHookRuntime) -> RecordingHookRuntime>;
    let cases: Vec<(HookPoint, Vec<Vec<ChatStreamEvent>>, Configurer)> = vec![
        (
            HookPoint::TransformContext,
            vec![stop_turn("done")],
            Box::new(|runtime| runtime.with_transforms([TransformBehavior::Pending])),
        ),
        (
            HookPoint::BeforeToolCall,
            vec![tool_call_turn("call-1", "echo", json!({}))],
            Box::new(|runtime| runtime.with_befores([BeforeBehavior::Pending])),
        ),
        (
            HookPoint::AfterToolCall,
            vec![tool_call_turn("call-1", "echo", json!({}))],
            Box::new(|runtime| runtime.with_afters([AfterBehavior::Pending])),
        ),
        (
            HookPoint::PrepareNextTurn,
            vec![tool_call_turn("call-1", "echo", json!({}))],
            Box::new(|runtime| runtime.with_prepares([PrepareBehavior::Pending])),
        ),
    ];
    for (point, behaviors, configure) in cases {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let runtime = configure(RecordingHookRuntime::new(&operations));
        let controls = Arc::clone(&runtime.controls);
        let agent_loop = build_loop(
            VecDeque::from(behaviors),
            limits,
            echo_registry(&operations, ToolPermissionMode::Allow),
            Arc::new(AllowAllToolApproval),
            Arc::new(RecordingDurableSink {
                operations: Arc::clone(&operations),
            }),
            Some(Arc::new(runtime)),
            &operations,
        );
        let _ = controls;

        let error = run_once(&agent_loop, CancellationToken::new())
            .await
            .expect_err("a pending hook should hit its deadline");

        assert!(
            matches!(
                error,
                AgentLoopError::Hook {
                    point: actual,
                    failure: HookFailure::TimedOut,
                } if actual == point
            ),
            "expected a typed timeout at {point:?}"
        );
        assert!(
            durable_events(&snapshot(&operations))
                .iter()
                .any(|event| matches!(event, DurableAgentEvent::LoopFailed { .. }))
        );
    }
}

// 3.3: cancellation before a hook never calls the runtime.
#[tokio::test]
async fn pre_cancelled_hooks_never_reach_the_runtime() {
    // Cancellation committed after the assistant message prevents
    // before_tool_call from being invoked.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let cancellation = CancellationToken::new();
    let runtime = RecordingHookRuntime::new(&operations);
    let agent_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        LoopLimits::new(8, 4),
        echo_registry(&operations, ToolPermissionMode::Allow),
        Arc::new(AllowAllToolApproval),
        Arc::new(CancellingDurableSink {
            operations: Arc::clone(&operations),
            cancellation: cancellation.clone(),
            trigger_on_tool_result: false,
        }),
        Some(Arc::new(runtime)),
        &operations,
    );

    let error = run_once(&agent_loop, cancellation)
        .await
        .expect_err("cancellation should stop the loop");

    assert!(matches!(error, AgentLoopError::Cancelled));
    let calls = hook_calls(&snapshot(&operations));
    assert_eq!(calls.len(), 1);
    assert!(matches!(
        calls[0],
        HookCall::TransformContext { iteration: 0, .. }
    ));

    // Cancellation committed after the tool result prevents prepare_next_turn
    // from being invoked, while the iteration boundary is still committed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let cancellation = CancellationToken::new();
    let runtime = RecordingHookRuntime::new(&operations);
    let agent_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        LoopLimits::new(8, 4),
        echo_registry(&operations, ToolPermissionMode::Allow),
        Arc::new(AllowAllToolApproval),
        Arc::new(CancellingDurableSink {
            operations: Arc::clone(&operations),
            cancellation: cancellation.clone(),
            trigger_on_tool_result: true,
        }),
        Some(Arc::new(runtime)),
        &operations,
    );

    let error = run_once(&agent_loop, cancellation)
        .await
        .expect_err("cancellation should stop the loop");

    assert!(matches!(error, AgentLoopError::Cancelled));
    let calls = hook_calls(&snapshot(&operations));
    assert!(
        !calls
            .iter()
            .any(|call| matches!(call, HookCall::PrepareNextTurn { .. })),
        "a pre-cancelled prepare hook must not run"
    );
    assert!(
        durable_events(&snapshot(&operations))
            .iter()
            .any(|event| matches!(
                event,
                DurableAgentEvent::IterationCompleted { iteration: 0, .. }
            ))
    );
}

// 3.3: cancellation during a gate hook (transform, before) cancels the loop
// without starting the affected action.
#[tokio::test]
async fn mid_hook_cancellation_at_gates_skips_the_affected_action() {
    // transform_context: no model request starts.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_transforms([TransformBehavior::CancelThenPending]);
    let (agent_loop, _) = hooked_loop(VecDeque::from([stop_turn("done")]), runtime, &operations);
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("mid-hook cancellation should stop the loop");
    assert!(matches!(error, AgentLoopError::Cancelled));
    assert!(chat_requests(&snapshot(&operations)).is_empty());

    // before_tool_call: the tool is not executed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime =
        RecordingHookRuntime::new(&operations).with_befores([BeforeBehavior::CancelThenPending]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("mid-hook cancellation should stop the loop");
    assert!(matches!(error, AgentLoopError::Cancelled));
    let recorded = snapshot(&operations);
    assert!(
        !recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. }))
    );
    assert!(
        !durable_events(&recorded)
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );
}

// 3.3: cancellation during a recording-path hook (after, prepare) degrades to
// the no-op decision so the started tool cycle finishes its durable records.
#[tokio::test]
async fn mid_hook_cancellation_on_recording_path_completes_durable_records() {
    // after_tool_call: the original result is still committed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime =
        RecordingHookRuntime::new(&operations).with_afters([AfterBehavior::CancelThenPending]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({"value": "one"}))]),
        runtime,
        &operations,
    );
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("cancellation should stop the loop after recording");
    assert!(matches!(error, AgentLoopError::Cancelled));
    let durable = durable_events(&snapshot(&operations));
    assert!(durable.iter().any(|event| matches!(
        event,
        DurableAgentEvent::MessageAppended { message }
            if message == &ChatMessage::tool(
                CallId::from("call-1"),
                json!({"echo": {"value": "one"}}),
            )
    )));
    assert!(durable.iter().any(|event| matches!(
        event,
        DurableAgentEvent::IterationCompleted { iteration: 0, .. }
    )));
    assert_eq!(
        durable
            .iter()
            .filter(|event| matches!(event, DurableAgentEvent::LoopCancelled { .. }))
            .count(),
        1
    );

    // prepare_next_turn: the iteration boundary is committed, no inject/stop.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime =
        RecordingHookRuntime::new(&operations).with_prepares([PrepareBehavior::CancelThenPending]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("cancellation should stop the loop after recording");
    assert!(matches!(error, AgentLoopError::Cancelled));
    let durable = durable_events(&snapshot(&operations));
    assert!(durable.iter().any(|event| matches!(
        event,
        DurableAgentEvent::IterationCompleted { iteration: 0, .. }
    )));
    assert!(
        !durable
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::LoopFinished { .. }))
    );
}

// 3.4: an empty block reason is rejected as invalid output.
#[tokio::test]
async fn empty_block_reason_is_invalid_output() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_befores([BeforeBehavior::Block("   ".to_owned())]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );

    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("an empty block reason should fail closed");

    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::BeforeToolCall,
            failure: HookFailure::InvalidOutput,
        }
    ));
    let recorded = snapshot(&operations);
    assert!(
        !recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. }))
    );
    assert!(!durable_events(&recorded).iter().any(|event| matches!(
        event,
        DurableAgentEvent::MessageAppended { message } if message.role == ChatRole::Tool
    )));
}

// 3.4: invalid inject payloads are rejected before the next model request.
#[tokio::test]
async fn invalid_inject_payloads_are_rejected_before_the_next_request() {
    let cases: Vec<Vec<ChatMessage>> = vec![
        vec![],
        vec![ChatMessage::assistant("forged assistant")],
        vec![ChatMessage::system("forged system")],
        vec![ChatMessage::user("note").with_reasoning_content("forged")],
        vec![ChatMessage::user("note").with_tool_calls(vec![ToolCall {
            call_id: CallId::from("call-x"),
            name: "echo".to_owned(),
            arguments: json!({}),
        }])],
        vec![{
            let mut message = ChatMessage::user("note");
            message.tool_call_id = Some(CallId::from("call-x"));
            message
        }],
    ];
    for messages in cases {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let runtime = RecordingHookRuntime::new(&operations)
            .with_prepares([PrepareBehavior::Inject(messages)]);
        let (agent_loop, _) = hooked_loop(
            VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
            runtime,
            &operations,
        );

        let error = run_once(&agent_loop, CancellationToken::new())
            .await
            .expect_err("an invalid inject should fail closed");

        assert!(matches!(
            error,
            AgentLoopError::Hook {
                point: HookPoint::PrepareNextTurn,
                failure: HookFailure::InvalidOutput,
            }
        ));
        assert_eq!(chat_requests(&snapshot(&operations)).len(), 1);
    }
}

// 3.4: tool calls with a non-tool_calls finish reason never enter tool hooks.
#[tokio::test]
async fn non_tool_calls_finish_reason_never_enters_tool_hooks() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            vec![
                tool_delta(0, "call-1", "echo", &json!({"value": 1})),
                ChatStreamEvent::Finished {
                    finish_reason: FinishReason::Length,
                    usage: None,
                },
            ],
            stop_turn("recovered"),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("unauthorized calls should be reported back to the model");

    assert_eq!(
        outcome.new_messages[2],
        ChatMessage::tool(
            CallId::from("call-1"),
            json!({
                "error": {
                    "code": "tool_call_truncated",
                    "message": "tool call was not executed because the model response reached its length limit"
                }
            }),
        )
    );
    let calls = hook_calls(&snapshot(&operations));
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            HookCall::BeforeToolCall { .. } | HookCall::AfterToolCall { .. }
        )),
        "unauthorized tool cycles must not enter tool hooks"
    );
}

// 3.4: hook errors never leak hook inputs or internal error text.
#[tokio::test]
async fn hook_failure_terminal_event_is_redacted() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_befores([BeforeBehavior::Fail(HookFailure::HandlerFailed)]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn(
            "call-1",
            "echo",
            json!({"secret": "payload"}),
        )]),
        runtime,
        &operations,
    );

    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("hook failure should stop the loop");

    let error_text = error.to_string();
    assert!(!error_text.contains("payload"));
    assert!(!error_text.contains("secret"));
    let durable = durable_events(&snapshot(&operations));
    let terminal = durable
        .iter()
        .find_map(|event| match event {
            DurableAgentEvent::LoopFailed { error_text, .. } => Some(error_text.clone()),
            _ => None,
        })
        .expect("loop failure should be durable");
    assert_eq!(
        terminal,
        "hook at before_tool_call failed: hook handler failed"
    );
}

// 3.5: one end-to-end flow through every hook decision.
#[tokio::test]
async fn end_to_end_hook_flow_transform_modify_replace_inject_stop() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let replacement = LoopContext::new("replaced system")
        .with_messages(vec![ChatMessage::user("replaced question")]);
    let injected = ChatMessage::user("one more constraint");
    let runtime = RecordingHookRuntime::new(&operations)
        .with_transforms([TransformBehavior::Replace(replacement)])
        .with_befores([
            BeforeBehavior::Modify(json!({"value": "modified"})),
            BeforeBehavior::Continue,
        ])
        .with_afters([
            AfterBehavior::Replace(json!({"redacted": true})),
            AfterBehavior::Keep,
        ])
        .with_prepares([
            PrepareBehavior::Inject(vec![injected.clone()]),
            PrepareBehavior::Stop,
        ]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "original"})),
            tool_call_turn("call-2", "echo", json!({"value": "second"})),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("the scripted hook flow should finish as hook stop");

    assert_eq!(outcome.completion, LoopCompletionReason::HookStopped);

    let modified_call = ToolCall {
        call_id: CallId::from("call-1"),
        name: "echo".to_owned(),
        arguments: json!({"value": "original"}),
    };
    let first_assistant = ChatMessage::assistant("").with_tool_calls(vec![modified_call.clone()]);
    let first_result = ChatMessage::tool(CallId::from("call-1"), json!({"redacted": true}));
    let second_assistant = ChatMessage::assistant("").with_tool_calls(vec![ToolCall {
        call_id: CallId::from("call-2"),
        name: "echo".to_owned(),
        arguments: json!({"value": "second"}),
    }]);
    let second_result =
        ChatMessage::tool(CallId::from("call-2"), json!({"echo": {"value": "second"}}));
    assert_eq!(
        outcome.new_messages,
        vec![
            ChatMessage::user("use echo"),
            first_assistant.clone(),
            first_result.clone(),
            second_assistant.clone(),
            second_result.clone(),
        ]
    );

    let recorded = snapshot(&operations);

    // The executor received the modified arguments under the original identity.
    let dispatched = recorded
        .iter()
        .filter_map(|operation| match operation {
            Operation::ToolCall { input, .. } => Some(input.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        dispatched,
        vec![
            ToolInput::new(CallId::from("call-1"), json!({"value": "modified"})),
            ToolInput::new(CallId::from("call-2"), json!({"value": "second"})),
        ]
    );

    // Model requests: the first uses the replaced context, the second uses the
    // committed context plus the replaced result and the one-shot inject.
    let requests = chat_requests(&recorded);
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[0].messages,
        vec![
            ChatMessage::system("replaced system"),
            ChatMessage::user("replaced question"),
        ]
    );
    assert_eq!(
        requests[1].messages,
        vec![
            ChatMessage::system("be precise"),
            ChatMessage::user("use echo"),
            first_assistant,
            first_result.clone(),
            injected.clone(),
        ]
    );

    // Hook order across the whole run.
    let hook_order = hook_calls(&recorded)
        .into_iter()
        .map(|call| match call {
            HookCall::TransformContext { iteration, .. } => ("transform", iteration),
            HookCall::BeforeToolCall { iteration, .. } => ("before", iteration),
            HookCall::AfterToolCall { iteration, .. } => ("after", iteration),
            HookCall::PrepareNextTurn { iteration, .. } => ("prepare", iteration),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        hook_order,
        vec![
            ("transform", 0),
            ("before", 0),
            ("after", 0),
            ("prepare", 0),
            ("transform", 1),
            ("before", 1),
            ("after", 1),
            ("prepare", 1),
        ]
    );

    // Durable truth: no replacement or inject messages, the tool result is the
    // replaced one, and the loop finishes as hook_stopped.
    let durable = durable_events(&recorded);
    assert!(!durable.iter().any(|event| matches!(
        event,
        DurableAgentEvent::MessageAppended { message }
            if message == &ChatMessage::user("replaced question") || message == &injected
    )));
    assert!(durable.contains(&DurableAgentEvent::MessageAppended {
        message: first_result,
    }));
    assert!(matches!(
        durable.last(),
        Some(DurableAgentEvent::LoopFinished { finish_reason, .. })
            if finish_reason == "hook_stopped"
    ));
}
