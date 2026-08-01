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
    AfterToolCallDecision, AfterToolCallInput, AgentLoop, AgentLoopError, AuthorizationOverride,
    DecideToolCallDecision, DecideToolCallInput, HookControl, HookRuntime, HookSnapshot,
    HookTimeouts, LoopCompletionReason, LoopContext, LoopLimits, PrepareNextTurnDecision,
    PrepareNextTurnInput, ToolExecutor, ToolHookTarget, TransformContextDecision,
    TransformContextInput, TransformToolCallDecision, TransformToolCallInput,
    TransformToolCallModification,
};
use stratum_core::{
    AgentTelemetryEvent, CallId, ChatContent, ChatMessage, ChatRole, DangerLevel,
    DurableAgentEvent, HookFailure, HookPoint, ModelId, TokenUsage, ToolCall, ToolCallDelta,
    ToolKind, ToolName, ToolSpec,
};
use stratum_infra::{DurableEventSink, DurableEventSinkError, TelemetryEventSink};
use stratum_llm::{
    ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, FinishReason, LlmError, LlmProvider,
};
use stratum_tools::{
    BuiltinToolRegistry, Tool, ToolError, ToolInput, ToolOutput, ToolPermissionMode, ToolRegistry,
};
use tokio::sync::{Notify, mpsc};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, PartialEq)]
enum Operation {
    Durable(DurableAgentEvent),
    ChatStream(ChatRequest),
    ToolCall { name: ToolName, input: ToolInput },
    Hook(Box<HookCall>),
}

/// Owned snapshot of the borrowed [`ToolHookTarget`] one hook received.
#[derive(Debug, Clone, PartialEq)]
struct RecordedTarget {
    authorization: Option<(ToolKind, DangerLevel)>,
    spec: ToolSpec,
}

impl RecordedTarget {
    fn of(target: &ToolHookTarget<'_>) -> Self {
        Self {
            authorization: target.authorization,
            spec: target.spec.clone(),
        }
    }
}

/// Owned snapshot of the borrowed [`HookSnapshot`] one hook received.
#[derive(Debug, Clone, PartialEq)]
struct RecordedSnapshot {
    iteration: u64,
    context: LoopContext,
    usage: Option<TokenUsage>,
}

impl RecordedSnapshot {
    fn of(snapshot: HookSnapshot<'_>) -> Self {
        Self {
            iteration: snapshot.iteration,
            context: snapshot.context.clone(),
            usage: snapshot.usage,
        }
    }
}

/// Builds an expected [`RecordedSnapshot`] for the standard `run_once` prompt.
fn recorded_snapshot(
    iteration: u64,
    messages: Vec<ChatMessage>,
    usage: Option<TokenUsage>,
) -> RecordedSnapshot {
    RecordedSnapshot {
        iteration,
        context: LoopContext::new("be precise").with_messages(messages),
        usage,
    }
}

#[derive(Debug, Clone, PartialEq)]
enum HookCall {
    TransformContext {
        snapshot: RecordedSnapshot,
    },
    TransformToolCall {
        snapshot: RecordedSnapshot,
        tool_call: ToolCall,
        target: RecordedTarget,
    },
    DecideToolCall {
        snapshot: RecordedSnapshot,
        tool_call: ToolCall,
        target: RecordedTarget,
    },
    AfterToolCall {
        snapshot: RecordedSnapshot,
        tool_call: ToolCall,
        target: RecordedTarget,
        result: ChatMessage,
    },
    PrepareNextTurn {
        snapshot: RecordedSnapshot,
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
enum ToolTransformBehavior {
    Continue,
    Modify(TransformToolCallModification),
    Fail(HookFailure),
    Pending,
    CancelThenPending,
}

/// Builds an arguments-only tool transform behavior.
fn modify_arguments(arguments: Value) -> ToolTransformBehavior {
    ToolTransformBehavior::Modify(TransformToolCallModification::new(Some(arguments), None))
}

#[derive(Debug, Clone)]
enum DecideBehavior {
    Execute,
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
    has_deadline: bool,
    deadline_in_future: bool,
}

struct RecordingHookRuntime {
    operations: Arc<Mutex<Vec<Operation>>>,
    controls: Arc<Mutex<Vec<ObservedControl>>>,
    transforms: Mutex<VecDeque<TransformBehavior>>,
    tool_transforms: Mutex<VecDeque<ToolTransformBehavior>>,
    decides: Mutex<VecDeque<DecideBehavior>>,
    afters: Mutex<VecDeque<AfterBehavior>>,
    prepares: Mutex<VecDeque<PrepareBehavior>>,
}

impl RecordingHookRuntime {
    fn new(operations: &Arc<Mutex<Vec<Operation>>>) -> Self {
        Self {
            operations: Arc::clone(operations),
            controls: Arc::new(Mutex::new(Vec::new())),
            transforms: Mutex::new(VecDeque::new()),
            tool_transforms: Mutex::new(VecDeque::new()),
            decides: Mutex::new(VecDeque::new()),
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

    fn with_tool_transforms(
        self,
        behaviors: impl IntoIterator<Item = ToolTransformBehavior>,
    ) -> Self {
        self.tool_transforms
            .lock()
            .expect("behavior lock should not be poisoned")
            .extend(behaviors);
        self
    }

    fn with_decides(self, behaviors: impl IntoIterator<Item = DecideBehavior>) -> Self {
        self.decides
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
        let deadline = control.deadline();
        self.controls
            .lock()
            .expect("control lock should not be poisoned")
            .push(ObservedControl {
                point,
                token_cancelled: control.cancellation().is_cancelled(),
                has_deadline: deadline.is_some(),
                deadline_in_future: deadline.is_some_and(|deadline| {
                    deadline.saturating_duration_since(tokio::time::Instant::now()) > Duration::ZERO
                }),
            });
    }

    fn record(&self, call: HookCall) {
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::Hook(Box::new(call)));
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
            snapshot: RecordedSnapshot::of(input.snapshot),
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

    async fn transform_tool_call<'a>(
        &self,
        input: TransformToolCallInput<'a>,
        control: HookControl,
    ) -> Result<TransformToolCallDecision, HookFailure> {
        self.record_control(HookPoint::TransformToolCall, &control);
        self.record(HookCall::TransformToolCall {
            snapshot: RecordedSnapshot::of(input.snapshot),
            tool_call: input.tool_call.clone(),
            target: RecordedTarget::of(input.tool),
        });
        let behavior = self
            .tool_transforms
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .unwrap_or(ToolTransformBehavior::Continue);
        match behavior {
            ToolTransformBehavior::Continue => Ok(TransformToolCallDecision::Continue),
            ToolTransformBehavior::Modify(modification) => {
                Ok(TransformToolCallDecision::Modify(modification))
            }
            ToolTransformBehavior::Fail(failure) => Err(failure),
            ToolTransformBehavior::Pending => pending().await,
            ToolTransformBehavior::CancelThenPending => {
                control.cancellation().cancel();
                pending().await
            }
        }
    }

    async fn decide_tool_call<'a>(
        &self,
        input: DecideToolCallInput<'a>,
        control: HookControl,
    ) -> Result<DecideToolCallDecision, HookFailure> {
        self.record_control(HookPoint::DecideToolCall, &control);
        self.record(HookCall::DecideToolCall {
            snapshot: RecordedSnapshot::of(input.snapshot),
            tool_call: input.tool_call.clone(),
            target: RecordedTarget::of(input.tool),
        });
        let behavior = self
            .decides
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .unwrap_or(DecideBehavior::Execute);
        match behavior {
            DecideBehavior::Execute => Ok(DecideToolCallDecision::Execute),
            DecideBehavior::Block(reason) => Ok(DecideToolCallDecision::Block { reason }),
            DecideBehavior::Fail(failure) => Err(failure),
            DecideBehavior::Pending => pending().await,
            DecideBehavior::CancelThenPending => {
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
            snapshot: RecordedSnapshot::of(input.snapshot),
            tool_call: input.tool_call.clone(),
            target: RecordedTarget::of(input.tool),
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
            snapshot: RecordedSnapshot::of(input.snapshot),
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
    strict_validation: bool,
}

#[async_trait]
impl Tool for EchoRecordingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn validate(&self, input: &ToolInput) -> Result<(), ToolError> {
        if self.strict_validation && !input.arguments.is_object() {
            return Err(ToolError::InvalidArgument {
                name: "arguments",
                reason: "must be an object",
            });
        }
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

fn echo_spec() -> ToolSpec {
    ToolSpec::builder()
        .name("echo")
        .description("records calls")
        .input_schema(json!({"type": "object"}))
        .build()
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

fn echo_registry(operations: &Arc<Mutex<Vec<Operation>>>) -> Arc<dyn ToolRegistry> {
    echo_registry_with(operations, ToolPermissionMode::Allow, false)
}

fn echo_registry_with(
    operations: &Arc<Mutex<Vec<Operation>>>,
    permission_mode: ToolPermissionMode,
    strict_validation: bool,
) -> Arc<dyn ToolRegistry> {
    let mut registry = BuiltinToolRegistry::new(permission_mode);
    registry
        .register(
            Arc::new(EchoRecordingTool {
                spec: echo_spec(),
                operations: Arc::clone(operations),
                strict_validation,
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
        .tool_executor(ToolExecutor::new(registry, durable))
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
        echo_registry(operations),
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
        echo_registry(operations),
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
            Operation::Hook(call) => Some(call.as_ref().clone()),
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

fn has_approval_events(operations: &[Operation]) -> bool {
    durable_events(operations).iter().any(|event| {
        matches!(
            event,
            DurableAgentEvent::ToolApprovalRequested { .. }
                | DurableAgentEvent::ToolApprovalResolved { .. }
        )
    })
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

// 4.1: the default builder keeps the pre-hook kernel behavior unchanged.
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

// 4.1: an injected runtime is called at all five points in the fixed order,
// and only the decide point has no deadline by default.
#[tokio::test]
async fn custom_runtime_is_invoked_at_all_five_points_in_order() {
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
    let expected_call = ToolCall {
        call_id: CallId::from("call-1"),
        name: "echo".to_owned(),
        arguments: json!({"value": "one"}),
    };
    let expected_target = RecordedTarget {
        authorization: None,
        spec: echo_spec(),
    };
    let assistant = ChatMessage::assistant("").with_tool_calls(vec![expected_call.clone()]);
    let result = ChatMessage::tool(CallId::from("call-1"), json!({"echo": {"value": "one"}}));
    let tool_boundary = recorded_snapshot(
        0,
        vec![ChatMessage::user("use echo"), assistant.clone()],
        None,
    );
    assert_eq!(
        hook_calls(&recorded),
        vec![
            HookCall::TransformContext {
                snapshot: recorded_snapshot(0, vec![ChatMessage::user("use echo")], None),
            },
            HookCall::TransformToolCall {
                snapshot: tool_boundary.clone(),
                tool_call: expected_call.clone(),
                target: expected_target.clone(),
            },
            HookCall::DecideToolCall {
                snapshot: tool_boundary.clone(),
                tool_call: expected_call.clone(),
                target: expected_target.clone(),
            },
            HookCall::AfterToolCall {
                snapshot: tool_boundary,
                tool_call: expected_call.clone(),
                target: expected_target,
                result: result.clone(),
            },
            HookCall::PrepareNextTurn {
                snapshot: recorded_snapshot(
                    0,
                    vec![
                        ChatMessage::user("use echo"),
                        assistant.clone(),
                        result.clone(),
                    ],
                    None,
                ),
            },
            HookCall::TransformContext {
                snapshot: recorded_snapshot(
                    1,
                    vec![ChatMessage::user("use echo"), assistant, result],
                    None,
                ),
            },
        ]
    );
    let controls = controls
        .lock()
        .expect("control lock should not be poisoned");
    let expected = [
        (HookPoint::TransformContext, true),
        (HookPoint::TransformToolCall, true),
        (HookPoint::DecideToolCall, false),
        (HookPoint::AfterToolCall, true),
        (HookPoint::PrepareNextTurn, true),
        (HookPoint::TransformContext, true),
    ];
    assert_eq!(controls.len(), expected.len());
    for (index, (observed, (point, has_deadline))) in
        controls.iter().zip(expected.iter()).enumerate()
    {
        assert_eq!(observed.point, *point);
        assert_eq!(
            observed.has_deadline, *has_deadline,
            "hook {index} deadline presence"
        );
        assert!(
            !observed.token_cancelled,
            "hook {index} should observe a live token"
        );
        assert_eq!(
            observed.deadline_in_future, *has_deadline,
            "hook {index} deadline should be in the future when present"
        );
    }
}

// 3.2 (H1): a replaced context is used for the current request only.
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

// 4.2: modified arguments keep the call identity and reach the executor.
#[tokio::test]
async fn transform_modify_arguments_preserves_call_identity() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_tool_transforms([modify_arguments(json!({"value": "modified"}))]);
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
    assert!(hook_calls(&recorded).iter().any(|call| matches!(
        call,
        HookCall::AfterToolCall { tool_call, result, .. }
            if tool_call.arguments == json!({"value": "modified"})
                && result == &ChatMessage::tool(
                    CallId::from("call-1"),
                    json!({"echo": {"value": "modified"}}),
                )
    )));
    assert_eq!(
        outcome.new_messages[2],
        ChatMessage::tool(
            CallId::from("call-1"),
            json!({"echo": {"value": "modified"}})
        )
    );
}

// 4.2: a transformed argument payload that fails the final re-validation
// produces the validation error result without reaching decide or execution.
#[tokio::test]
async fn transform_modify_failing_revalidation_never_reaches_decide() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime =
        RecordingHookRuntime::new(&operations).with_tool_transforms([modify_arguments(json!(42))]);
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "original"})),
            stop_turn("recovered"),
        ]),
        LoopLimits::new(8, 4),
        echo_registry_with(&operations, ToolPermissionMode::Allow, true),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        Some(Arc::new(runtime)),
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("re-validation failures stay model-visible results");

    let expected_result = ChatMessage::tool(
        CallId::from("call-1"),
        json!({"error": "invalid argument arguments: must be an object"}),
    );
    assert_eq!(outcome.new_messages[2], expected_result);
    let recorded = snapshot(&operations);
    let calls = hook_calls(&recorded);
    assert!(
        calls
            .iter()
            .any(|call| matches!(call, HookCall::TransformToolCall { .. })),
        "the transform hook must run before re-validation"
    );
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            HookCall::DecideToolCall { .. } | HookCall::AfterToolCall { .. }
        )),
        "re-validation failures must not reach decide or after"
    );
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
    let requests = chat_requests(&recorded);
    assert_eq!(requests[1].messages.last(), Some(&expected_result));
}

// 4.3: the decide hook receives the re-validated final arguments produced by
// the transform phase.
#[tokio::test]
async fn decide_sees_the_revalidated_final_arguments() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_tool_transforms([modify_arguments(json!({"value": "modified"}))]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "original"})),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let calls = hook_calls(&snapshot(&operations));
    let decide_calls = calls
        .iter()
        .filter_map(|call| match call {
            HookCall::DecideToolCall { tool_call, .. } => Some(tool_call.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        decide_calls,
        vec![ToolCall {
            call_id: CallId::from("call-1"),
            name: "echo".to_owned(),
            arguments: json!({"value": "modified"}),
        }]
    );
}

// 4.2: a Set authorization override becomes the effective authorization the
// decide and after phases observe, while the transform phase still sees the
// registry-declared default.
#[tokio::test]
async fn transform_set_authorization_override_reaches_decide_and_after() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations).with_tool_transforms([
        ToolTransformBehavior::Modify(TransformToolCallModification::new(
            None,
            Some(AuthorizationOverride::Set {
                kind: ToolKind::Write,
                danger: DangerLevel::High,
            }),
        )),
    ]);
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        LoopLimits::new(8, 4),
        echo_registry_with(&operations, ToolPermissionMode::RequireApproval, false),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        Some(Arc::new(runtime)),
        &operations,
    );

    run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let recorded = snapshot(&operations);
    let calls = hook_calls(&recorded);
    let effective_target = RecordedTarget {
        authorization: Some((ToolKind::Write, DangerLevel::High)),
        spec: echo_spec(),
    };
    for call in &calls {
        match call {
            HookCall::TransformToolCall { target, .. } => assert_eq!(
                target.authorization,
                Some((ToolKind::Read, DangerLevel::Low)),
                "transform must observe the registry-declared default"
            ),
            HookCall::DecideToolCall { target, .. } | HookCall::AfterToolCall { target, .. } => {
                assert_eq!(
                    target, &effective_target,
                    "decide and after must observe the effective authorization"
                );
            }
            _ => {}
        }
    }
    // The kernel transports the override without branching on it: the call
    // still dispatches and commits its durable start.
    assert!(
        recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. }))
    );
    assert!(
        durable_events(&recorded)
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );
}

// 4.2: a PreAuthorize override erases the declared authorization, so the
// decide and after phases observe a pre-authorized target.
#[tokio::test]
async fn transform_pre_authorize_override_erases_authorization_at_decide() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations).with_tool_transforms([
        ToolTransformBehavior::Modify(TransformToolCallModification::new(
            None,
            Some(AuthorizationOverride::PreAuthorize),
        )),
    ]);
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        LoopLimits::new(8, 4),
        echo_registry_with(&operations, ToolPermissionMode::RequireApproval, false),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        Some(Arc::new(runtime)),
        &operations,
    );

    run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let calls = hook_calls(&snapshot(&operations));
    for call in &calls {
        match call {
            HookCall::TransformToolCall { target, .. } => assert_eq!(
                target.authorization,
                Some((ToolKind::Read, DangerLevel::Low)),
                "transform must observe the registry-declared default"
            ),
            HookCall::DecideToolCall { target, .. } | HookCall::AfterToolCall { target, .. } => {
                assert_eq!(
                    target.authorization, None,
                    "PreAuthorize must erase the declared authorization at decide and after"
                );
            }
            _ => {}
        }
    }
}

// 3.4: a Modify decision with every field left unchanged is invalid output
// and fails closed before decide and execution.
#[tokio::test]
async fn transform_modify_without_changes_is_invalid_output() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations).with_tool_transforms([
        ToolTransformBehavior::Modify(TransformToolCallModification::new(None, None)),
    ]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );

    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("a no-op Modify should fail closed");

    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::TransformToolCall,
            failure: HookFailure::InvalidOutput,
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
}

// 4.2 + 4.3: a transform that modifies arguments and authorization together
// lets the decide phase see both the final arguments and the effective
// authorization, and the call executes with the final arguments.
#[tokio::test]
async fn transform_modify_arguments_and_authorization_together() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations).with_tool_transforms([
        ToolTransformBehavior::Modify(TransformToolCallModification::new(
            Some(json!({"value": "modified"})),
            Some(AuthorizationOverride::Set {
                kind: ToolKind::Write,
                danger: DangerLevel::Medium,
            }),
        )),
    ]);
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
    let decide_calls = hook_calls(&recorded)
        .into_iter()
        .filter_map(|call| match call {
            HookCall::DecideToolCall {
                tool_call, target, ..
            } => Some((tool_call, target)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        decide_calls,
        vec![(
            ToolCall {
                call_id: CallId::from("call-1"),
                name: "echo".to_owned(),
                arguments: json!({"value": "modified"}),
            },
            RecordedTarget {
                authorization: Some((ToolKind::Write, DangerLevel::Medium)),
                spec: echo_spec(),
            },
        )]
    );
    assert_eq!(
        outcome.new_messages[2],
        ChatMessage::tool(
            CallId::from("call-1"),
            json!({"echo": {"value": "modified"}})
        )
    );
}

// 4.3 + 2.2: a decide block skips the durable start and execution, produces
// the fixed hook_blocked result, and still passes through the after hook.
#[tokio::test]
async fn decide_block_produces_hook_blocked_result_and_reaches_after() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_decides([DecideBehavior::Block("policy denied".to_owned())]);
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        LoopLimits::new(8, 4),
        echo_registry_with(&operations, ToolPermissionMode::RequireApproval, false),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        Some(Arc::new(runtime)),
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("a blocked call should stay a model-visible result");

    let blocked = ChatMessage::tool(
        CallId::from("call-1"),
        json!({"error": {"code": "hook_blocked", "message": "policy denied"}}),
    );
    assert_eq!(outcome.new_messages[2], blocked);
    let recorded = snapshot(&operations);
    assert!(hook_calls(&recorded).iter().any(|call| matches!(
        call,
        HookCall::AfterToolCall { tool_call, result, .. }
            if tool_call.arguments == json!({"value": "one"}) && result == &blocked
    )));
    assert!(
        !recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. })),
        "a blocked call must never dispatch"
    );
    let durable = durable_events(&recorded);
    assert!(
        !durable
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );
    assert!(!has_approval_events(&recorded));
    let requests = chat_requests(&recorded);
    assert_eq!(requests[1].messages.last(), Some(&blocked));
}

// 4.3: an empty block reason is rejected as invalid output.
#[tokio::test]
async fn empty_block_reason_is_invalid_output() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_decides([DecideBehavior::Block("   ".to_owned())]);
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
            point: HookPoint::DecideToolCall,
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

// 3.2 (H1): a replaced result keeps the tool role and call identity.
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

// 3.2 (H1): prepare stop commits the iteration and finishes as hook_stopped.
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

// 3.2 (H1): injected messages reach only the next request, exactly once.
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
            HookCall::TransformContext { snapshot } if snapshot.iteration == 1 => {
                Some(snapshot.context)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(transform_views.len(), 1);
    assert_eq!(transform_views[0].messages.last(), Some(&injected));
    // The transform snapshot context is the full request view basis: committed
    // context plus the pending one-shot inject.
    assert_eq!(
        transform_views[0],
        LoopContext::new("be precise").with_messages(vec![
            ChatMessage::user("use echo"),
            ChatMessage::assistant("").with_tool_calls(vec![ToolCall {
                call_id: CallId::from("call-1"),
                name: "echo".to_owned(),
                arguments: json!({"value": "one"}),
            }]),
            ChatMessage::tool(CallId::from("call-1"), json!({"echo": {"value": "one"}})),
            injected.clone(),
        ])
    );
}

// 4.1 + 4.5: runtime failures fail closed at every hook point.
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

    // transform_tool_call: the tool is neither decided nor executed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_tool_transforms([ToolTransformBehavior::Fail(HookFailure::HandlerFailed)]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("tool transform failure should stop the loop");
    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::TransformToolCall,
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

    // decide_tool_call: the tool is not executed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_decides([DecideBehavior::Fail(HookFailure::HandlerFailed)]);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );
    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("decide failure should stop the loop");
    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::DecideToolCall,
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

// 4.5: a missed configured deadline maps to a typed timeout at every point,
// including decide when one is configured explicitly.
#[tokio::test]
async fn hook_deadline_maps_to_typed_timeout_at_every_point() {
    let timeout = Duration::from_millis(50);
    let limits = LoopLimits::new(8, 4).with_hook_timeouts(
        HookTimeouts::new()
            .with_transform_context(Some(timeout))
            .with_transform_tool_call(Some(timeout))
            .with_decide_tool_call(Some(timeout))
            .with_after_tool_call(Some(timeout))
            .with_prepare_next_turn(Some(timeout)),
    );
    type Configurer = Box<dyn FnOnce(RecordingHookRuntime) -> RecordingHookRuntime>;
    let cases: Vec<(HookPoint, Vec<Vec<ChatStreamEvent>>, Configurer)> = vec![
        (
            HookPoint::TransformContext,
            vec![stop_turn("done")],
            Box::new(|runtime| runtime.with_transforms([TransformBehavior::Pending])),
        ),
        (
            HookPoint::TransformToolCall,
            vec![tool_call_turn("call-1", "echo", json!({}))],
            Box::new(|runtime| runtime.with_tool_transforms([ToolTransformBehavior::Pending])),
        ),
        (
            HookPoint::DecideToolCall,
            vec![tool_call_turn("call-1", "echo", json!({}))],
            Box::new(|runtime| runtime.with_decides([DecideBehavior::Pending])),
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
        let agent_loop = build_loop(
            VecDeque::from(behaviors),
            limits,
            echo_registry(&operations),
            Arc::new(RecordingDurableSink {
                operations: Arc::clone(&operations),
            }),
            Some(Arc::new(runtime)),
            &operations,
        );

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

// 4.5: with the default configuration decide has no deadline, so a long wait
// only ends when the turn cancellation token fires.
#[tokio::test(start_paused = true)]
async fn decide_tool_call_without_deadline_waits_until_cancellation() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations).with_decides([DecideBehavior::Pending]);
    let controls = Arc::clone(&runtime.controls);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        runtime,
        &operations,
    );
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move { run_once(&agent_loop, task_cancellation).await });

    for _ in 0..100 {
        if hook_calls(&snapshot(&operations))
            .iter()
            .any(|call| matches!(call, HookCall::DecideToolCall { .. }))
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(
        hook_calls(&snapshot(&operations))
            .iter()
            .any(|call| matches!(call, HookCall::DecideToolCall { .. })),
        "the decide hook should be pending"
    );

    // An hour passes: no deadline fires for the decide point.
    tokio::time::advance(Duration::from_secs(3600)).await;
    tokio::task::yield_now().await;
    assert!(
        !task.is_finished(),
        "decide without a deadline must keep waiting"
    );

    cancellation.cancel();
    let error = task
        .await
        .expect("loop task should not panic")
        .expect_err("cancellation should stop the loop");
    assert!(matches!(error, AgentLoopError::Cancelled));
    let decide_control = controls
        .lock()
        .expect("control lock should not be poisoned")
        .iter()
        .find(|control| control.point == HookPoint::DecideToolCall)
        .copied()
        .expect("decide control should be recorded");
    assert!(!decide_control.has_deadline);
    let durable = durable_events(&snapshot(&operations));
    assert!(
        !durable
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );
    assert_eq!(
        durable
            .iter()
            .filter(|event| matches!(event, DurableAgentEvent::LoopCancelled { .. }))
            .count(),
        1
    );
}

// 3.3 (H1): cancellation before a hook never calls the runtime.
#[tokio::test]
async fn pre_cancelled_hooks_never_reach_the_runtime() {
    // Cancellation committed after the assistant message prevents
    // transform_tool_call from being invoked.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let cancellation = CancellationToken::new();
    let runtime = RecordingHookRuntime::new(&operations);
    let agent_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        LoopLimits::new(8, 4),
        echo_registry(&operations),
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
        &calls[0],
        HookCall::TransformContext { snapshot } if snapshot.iteration == 0
    ));

    // Cancellation committed after the tool result prevents prepare_next_turn
    // from being invoked, while the iteration boundary is still committed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let cancellation = CancellationToken::new();
    let runtime = RecordingHookRuntime::new(&operations);
    let agent_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        LoopLimits::new(8, 4),
        echo_registry(&operations),
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

// 3.3 (H1): cancellation during a gate hook (context transform, tool
// transform, decide) cancels the loop without starting the affected action.
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

    // transform_tool_call: the tool is neither decided nor executed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_tool_transforms([ToolTransformBehavior::CancelThenPending]);
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

    // decide_tool_call: the tool is not executed.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime =
        RecordingHookRuntime::new(&operations).with_decides([DecideBehavior::CancelThenPending]);
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

// 3.3 (H1): cancellation during a recording-path hook (after, prepare)
// degrades to the no-op decision so the started tool cycle finishes its
// durable records.
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

// 3.4 (H1): invalid inject payloads are rejected before the next model request.
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

// 2.3: tool calls with a non-tool_calls finish reason never enter tool hooks.
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
            HookCall::TransformToolCall { .. }
                | HookCall::DecideToolCall { .. }
                | HookCall::AfterToolCall { .. }
        )),
        "unauthorized tool cycles must not enter tool hooks"
    );
}

// 2.3 + 4.4: a missing tool produces the lookup error result without entering
// any tool hook.
#[tokio::test]
async fn missing_tool_never_enters_tool_hooks() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "ghost", json!({"value": 1})),
            stop_turn("recovered"),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("a missing tool stays a model-visible result");

    assert_eq!(
        outcome.new_messages[2],
        ChatMessage::tool(
            CallId::from("call-1"),
            json!({"error": "tool not found: ghost"}),
        )
    );
    let calls = hook_calls(&snapshot(&operations));
    assert!(
        !calls.iter().any(|call| matches!(
            call,
            HookCall::TransformToolCall { .. }
                | HookCall::DecideToolCall { .. }
                | HookCall::AfterToolCall { .. }
        )),
        "a missing tool must not enter tool hooks"
    );
}

// 4.4: all three tool hooks receive the authorization metadata and the tool
// spec exactly as registered.
#[tokio::test]
async fn tool_hooks_receive_authorization_metadata_and_spec() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations);
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        LoopLimits::new(8, 4),
        echo_registry_with(&operations, ToolPermissionMode::RequireApproval, false),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        Some(Arc::new(runtime)),
        &operations,
    );

    run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let expected_target = RecordedTarget {
        authorization: Some((ToolKind::Read, DangerLevel::Low)),
        spec: echo_spec(),
    };
    let targets = hook_calls(&snapshot(&operations))
        .into_iter()
        .filter_map(|call| match call {
            HookCall::TransformToolCall { target, .. }
            | HookCall::DecideToolCall { target, .. }
            | HookCall::AfterToolCall { target, .. } => Some(target),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 3);
    assert!(
        targets.iter().all(|target| target == &expected_target),
        "every tool hook must observe the registry authorization metadata and spec"
    );
}

// 3.4 (H1): hook errors never leak hook inputs or internal error text.
#[tokio::test]
async fn hook_failure_terminal_event_is_redacted() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations)
        .with_decides([DecideBehavior::Fail(HookFailure::HandlerFailed)]);
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
        "hook at decide_tool_call failed: hook handler failed"
    );
}

// 4.1: one end-to-end flow through every hook decision.
#[tokio::test]
async fn end_to_end_hook_flow_transform_modify_replace_inject_stop() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let replacement = LoopContext::new("replaced system")
        .with_messages(vec![ChatMessage::user("replaced question")]);
    let injected = ChatMessage::user("one more constraint");
    let runtime = RecordingHookRuntime::new(&operations)
        .with_transforms([TransformBehavior::Replace(replacement)])
        .with_tool_transforms([
            modify_arguments(json!({"value": "modified"})),
            ToolTransformBehavior::Continue,
        ])
        .with_decides([DecideBehavior::Execute, DecideBehavior::Execute])
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
            HookCall::TransformContext { snapshot } => ("transform", snapshot.iteration),
            HookCall::TransformToolCall { snapshot, .. } => ("tool_transform", snapshot.iteration),
            HookCall::DecideToolCall { snapshot, .. } => ("decide", snapshot.iteration),
            HookCall::AfterToolCall { snapshot, .. } => ("after", snapshot.iteration),
            HookCall::PrepareNextTurn { snapshot } => ("prepare", snapshot.iteration),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        hook_order,
        vec![
            ("transform", 0),
            ("tool_transform", 0),
            ("decide", 0),
            ("after", 0),
            ("prepare", 0),
            ("transform", 1),
            ("tool_transform", 1),
            ("decide", 1),
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
    assert!(!has_approval_events(&recorded));
}

// 4.6: a simulated approval handler implemented as an ordinary decide-phase
// hook with a private ask-the-human channel.
#[derive(Debug, Clone, Copy)]
enum ApprovalOutcome {
    Approve,
    Reject,
}

struct SimulatedApprovalRuntime {
    entered: Arc<Notify>,
    outcomes: tokio::sync::Mutex<mpsc::Receiver<ApprovalOutcome>>,
}

impl SimulatedApprovalRuntime {
    fn new() -> (Self, mpsc::Sender<ApprovalOutcome>) {
        let (sender, receiver) = mpsc::channel(1);
        (
            Self {
                entered: Arc::new(Notify::new()),
                outcomes: tokio::sync::Mutex::new(receiver),
            },
            sender,
        )
    }
}

#[async_trait]
impl HookRuntime for SimulatedApprovalRuntime {
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
        self.entered.notify_one();
        match self.outcomes.lock().await.recv().await {
            Some(ApprovalOutcome::Approve) => Ok(DecideToolCallDecision::Execute),
            Some(ApprovalOutcome::Reject) => Ok(DecideToolCallDecision::Block {
                reason: "user rejected the tool call".to_owned(),
            }),
            // The ask channel is gone; wait for the turn cancellation token.
            None => pending().await,
        }
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

fn approval_loop(
    runtime: SimulatedApprovalRuntime,
    operations: &Arc<Mutex<Vec<Operation>>>,
) -> (AgentLoop, Arc<Notify>) {
    let entered = Arc::clone(&runtime.entered);
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        LoopLimits::new(8, 4),
        echo_registry_with(operations, ToolPermissionMode::RequireApproval, false),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(operations),
        }),
        Some(Arc::new(runtime)),
        operations,
    );
    (agent_loop, entered)
}

// 4.6: an approved call executes through the ordinary Execute path, without
// any approval durable events.
#[tokio::test]
async fn approval_handler_approval_executes_the_tool() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (runtime, outcomes) = SimulatedApprovalRuntime::new();
    let (agent_loop, entered) = approval_loop(runtime, &operations);
    let task = tokio::spawn(async move { run_once(&agent_loop, CancellationToken::new()).await });
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .expect("the approval handler should begin waiting");

    outcomes
        .send(ApprovalOutcome::Approve)
        .await
        .expect("approval channel should be open");
    let outcome = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("the loop should finish after approval")
        .expect("loop task should not panic")
        .expect("an approved call should execute");

    assert_eq!(
        outcome.new_messages[2],
        ChatMessage::tool(CallId::from("call-1"), json!({"echo": {"value": "one"}}))
    );
    let recorded = snapshot(&operations);
    assert!(
        durable_events(&recorded)
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );
    assert!(
        recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. }))
    );
    assert!(!has_approval_events(&recorded));
}

// 4.6: a rejected call becomes the hook_blocked result without execution and
// without approval durable events.
#[tokio::test]
async fn approval_handler_rejection_blocks_with_hook_blocked() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (runtime, outcomes) = SimulatedApprovalRuntime::new();
    let (agent_loop, entered) = approval_loop(runtime, &operations);
    let task = tokio::spawn(async move { run_once(&agent_loop, CancellationToken::new()).await });
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .expect("the approval handler should begin waiting");

    outcomes
        .send(ApprovalOutcome::Reject)
        .await
        .expect("approval channel should be open");
    let outcome = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("the loop should finish after rejection")
        .expect("loop task should not panic")
        .expect("a rejected call should stay a model-visible result");

    assert_eq!(
        outcome.new_messages[2],
        ChatMessage::tool(
            CallId::from("call-1"),
            json!({"error": {"code": "hook_blocked", "message": "user rejected the tool call"}}),
        )
    );
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
    assert!(!has_approval_events(&recorded));
}

// 4.6: cancellation while the approval handler waits ends in loop
// cancellation without an execution start.
#[tokio::test]
async fn approval_handler_pending_cancel_ends_in_loop_cancellation() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let (runtime, _outcomes) = SimulatedApprovalRuntime::new();
    let (agent_loop, entered) = approval_loop(runtime, &operations);
    let cancellation = CancellationToken::new();
    let task_cancellation = cancellation.clone();
    let task = tokio::spawn(async move { run_once(&agent_loop, task_cancellation).await });
    tokio::time::timeout(Duration::from_secs(1), entered.notified())
        .await
        .expect("the approval handler should begin waiting");

    cancellation.cancel();
    let error = tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("the loop should stop after cancellation")
        .expect("loop task should not panic")
        .expect_err("cancellation during approval should fail the run");

    assert!(matches!(error, AgentLoopError::Cancelled));
    let recorded = snapshot(&operations);
    assert!(
        !recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. }))
    );
    let durable = durable_events(&recorded);
    assert!(
        !durable
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );
    assert_eq!(
        durable
            .iter()
            .filter(|event| matches!(event, DurableAgentEvent::LoopCancelled { .. }))
            .count(),
        1
    );
    assert!(!has_approval_events(&recorded));
}

// ---- HookSnapshot boundary semantics (add-hook-input-envelope tasks 3.2/3.3) ----

/// Returns the recorded snapshot of any hook call variant.
fn hook_snapshot(call: &HookCall) -> &RecordedSnapshot {
    match call {
        HookCall::TransformContext { snapshot }
        | HookCall::TransformToolCall { snapshot, .. }
        | HookCall::DecideToolCall { snapshot, .. }
        | HookCall::AfterToolCall { snapshot, .. }
        | HookCall::PrepareNextTurn { snapshot } => snapshot,
    }
}

fn two_tool_call_turn(
    first_call_id: &str,
    first_arguments: Value,
    second_call_id: &str,
    second_arguments: Value,
) -> Vec<ChatStreamEvent> {
    vec![
        tool_delta(0, first_call_id, "echo", &first_arguments),
        tool_delta(1, second_call_id, "echo", &second_arguments),
        ChatStreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
            usage: None,
        },
    ]
}

fn tool_call_turn_with_usage(
    call_id: &str,
    name: &str,
    arguments: Value,
    usage: TokenUsage,
) -> Vec<ChatStreamEvent> {
    vec![
        tool_delta(0, call_id, name, &arguments),
        ChatStreamEvent::Finished {
            finish_reason: FinishReason::ToolCalls,
            usage: Some(usage),
        },
    ]
}

// 3.2: each hook boundary observes exactly the committed context defined for
// its point; `after_tool_call` never sees the current uncommitted result.
#[tokio::test]
async fn snapshot_context_matches_each_hook_boundary() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            two_tool_call_turn(
                "call-1",
                json!({"value": "one"}),
                "call-2",
                json!({"value": "two"}),
            ),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let first_call = ToolCall {
        call_id: CallId::from("call-1"),
        name: "echo".to_owned(),
        arguments: json!({"value": "one"}),
    };
    let second_call = ToolCall {
        call_id: CallId::from("call-2"),
        name: "echo".to_owned(),
        arguments: json!({"value": "two"}),
    };
    let assistant =
        ChatMessage::assistant("").with_tool_calls(vec![first_call.clone(), second_call.clone()]);
    let first_result = ChatMessage::tool(CallId::from("call-1"), json!({"echo": {"value": "one"}}));
    let second_result =
        ChatMessage::tool(CallId::from("call-2"), json!({"echo": {"value": "two"}}));
    let calls = hook_calls(&snapshot(&operations));

    // transform_context sees the request view basis before the assistant
    // message is committed.
    let transform = calls
        .iter()
        .find_map(|call| match call {
            HookCall::TransformContext { snapshot } if snapshot.iteration == 0 => Some(snapshot),
            _ => None,
        })
        .expect("the iteration-0 transform hook should run");
    assert_eq!(
        transform,
        &recorded_snapshot(0, vec![ChatMessage::user("use echo")], None)
    );

    let first_boundary = recorded_snapshot(
        0,
        vec![ChatMessage::user("use echo"), assistant.clone()],
        None,
    );
    let second_boundary = recorded_snapshot(
        0,
        vec![
            ChatMessage::user("use echo"),
            assistant.clone(),
            first_result.clone(),
        ],
        None,
    );
    for call in &calls {
        let (tool_call, snapshot) = match call {
            HookCall::TransformToolCall {
                tool_call,
                snapshot,
                ..
            }
            | HookCall::DecideToolCall {
                tool_call,
                snapshot,
                ..
            }
            | HookCall::AfterToolCall {
                tool_call,
                snapshot,
                ..
            } => (tool_call, snapshot),
            _ => continue,
        };
        let expected = if tool_call.call_id == CallId::from("call-1") {
            &first_boundary
        } else {
            &second_boundary
        };
        assert_eq!(
            snapshot, expected,
            "tool hook boundary for {tool_call:?} should be the committed context"
        );
    }

    // after_tool_call excludes the current uncommitted result; the result only
    // appears in the point-specific payload.
    let afters = calls
        .iter()
        .filter_map(|call| match call {
            HookCall::AfterToolCall {
                snapshot, result, ..
            } => Some((snapshot, result)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(afters.len(), 2);
    for (snapshot, result) in afters {
        assert!(
            !snapshot.context.messages.contains(result),
            "the after snapshot must not contain the uncommitted result {result:?}"
        );
    }

    // prepare_next_turn sees every committed result of the cycle.
    let prepare = calls
        .iter()
        .find_map(|call| match call {
            HookCall::PrepareNextTurn { snapshot } => Some(snapshot),
            _ => None,
        })
        .expect("the prepare hook should run");
    assert_eq!(
        prepare,
        &recorded_snapshot(
            0,
            vec![
                ChatMessage::user("use echo"),
                assistant,
                first_result,
                second_result,
            ],
            None,
        )
    );
}

// 3.3: snapshot usage is the run-level accumulation of provider reports up to
// each hook boundary.
#[tokio::test]
async fn snapshot_usage_accumulates_provider_reports() {
    let first = TokenUsage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
    };
    let second = TokenUsage {
        input_tokens: 20,
        output_tokens: 10,
        total_tokens: 30,
    };
    let accumulated = TokenUsage {
        input_tokens: 30,
        output_tokens: 15,
        total_tokens: 45,
    };
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn_with_usage("call-1", "echo", json!({}), first),
            tool_call_turn_with_usage("call-2", "echo", json!({}), second),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    assert_eq!(outcome.usage, accumulated);
    for call in hook_calls(&snapshot(&operations)) {
        let snapshot = hook_snapshot(&call);
        let expected = match &call {
            // Nothing has been reported before the first model response.
            HookCall::TransformContext { .. } if snapshot.iteration == 0 => None,
            // Later transform hooks see everything reported so far; the final
            // stop response reported no usage, so the total stays unchanged.
            HookCall::TransformContext { .. } if snapshot.iteration == 1 => Some(first),
            HookCall::TransformContext { .. } => Some(accumulated),
            _ if snapshot.iteration == 0 => Some(first),
            _ => Some(accumulated),
        };
        assert_eq!(
            snapshot.usage, expected,
            "usage at {call:?} should be the accumulation up to that boundary"
        );
    }
}

// 3.3: snapshot usage stays `None` when no provider response reports usage.
#[tokio::test]
async fn snapshot_usage_is_none_when_providers_never_report() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations);
    let (agent_loop, _) = hooked_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        runtime,
        &operations,
    );

    run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let calls = hook_calls(&snapshot(&operations));
    assert!(!calls.is_empty());
    assert!(
        calls.iter().all(|call| hook_snapshot(call).usage.is_none()),
        "no provider reported usage, so every snapshot must carry None"
    );
}
