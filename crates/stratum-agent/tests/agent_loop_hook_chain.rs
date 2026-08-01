//! Integration coverage for `ChainHookRuntime` injected into the agent loop
//! kernel: chain version pinning on `LoopStarted`, resume-time version
//! verification, kernel re-validation of chained transforms, and reuse of the
//! existing journal/deadline/cancellation contract.

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
    AfterToolCallDecision, AfterToolCallInput, AgentLoop, AgentLoopError, ChainHookRuntime,
    DecideToolCallDecision, DecideToolCallInput, HookControl, HookHandler, HookHandlerDescriptor,
    HookRuntime, HookTimeouts, LoopCompletionReason, LoopContext, LoopLimits,
    PrepareNextTurnDecision, PrepareNextTurnInput, ResumeError, ToolExecutor,
    TransformContextDecision, TransformContextInput, TransformToolCallDecision,
    TransformToolCallInput, TransformToolCallModification,
};
use stratum_core::{
    AgentTelemetryEvent, CallId, ChatMessage, ChatRole, DangerLevel, DurableAgentEvent,
    ExtensionSetVersionId, HookFailure, HookHandlerVersionId, HookPoint, ModelId, ToolCall,
    ToolCallDelta, ToolKind, ToolName, ToolSpec,
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
}

/// Hook point one chain handler was invoked at, recorded in call order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChainPoint {
    TransformContext,
    TransformToolCall,
    DecideToolCall,
    AfterToolCall,
    PrepareNextTurn,
}

/// Shared ordered log of `(handler name, hook point)` invocations.
type ChainLog = Arc<Mutex<Vec<(&'static str, ChainPoint)>>>;

#[derive(Debug, Clone)]
enum ContextAction {
    Unchanged,
    Pending,
    CancelThenPending,
}

#[derive(Debug, Clone)]
enum ToolTransformAction {
    Continue,
    ModifyArguments(Value),
}

#[derive(Debug, Clone)]
enum DecideAction {
    Execute,
    Block(String),
}

/// Programmable chain handler recording its invocations in call order.
struct ChainScriptableHandler {
    name: &'static str,
    version_id: HookHandlerVersionId,
    log: ChainLog,
    transforms: Mutex<VecDeque<ContextAction>>,
    tool_transforms: Mutex<VecDeque<ToolTransformAction>>,
    decides: Mutex<VecDeque<DecideAction>>,
}

impl ChainScriptableHandler {
    fn new(name: &'static str, version_id: HookHandlerVersionId, log: &ChainLog) -> Self {
        Self {
            name,
            version_id,
            log: Arc::clone(log),
            transforms: Mutex::new(VecDeque::new()),
            tool_transforms: Mutex::new(VecDeque::new()),
            decides: Mutex::new(VecDeque::new()),
        }
    }

    fn with_transforms(mut self, actions: impl IntoIterator<Item = ContextAction>) -> Self {
        self.transforms
            .get_mut()
            .expect("action lock should not be poisoned")
            .extend(actions);
        self
    }

    fn with_tool_transforms(
        mut self,
        actions: impl IntoIterator<Item = ToolTransformAction>,
    ) -> Self {
        self.tool_transforms
            .get_mut()
            .expect("action lock should not be poisoned")
            .extend(actions);
        self
    }

    fn with_decides(mut self, actions: impl IntoIterator<Item = DecideAction>) -> Self {
        self.decides
            .get_mut()
            .expect("action lock should not be poisoned")
            .extend(actions);
        self
    }

    fn record(&self, point: ChainPoint) {
        self.log
            .lock()
            .expect("chain log lock should not be poisoned")
            .push((self.name, point));
    }
}

#[async_trait]
impl HookHandler for ChainScriptableHandler {
    fn descriptor(&self) -> HookHandlerDescriptor {
        HookHandlerDescriptor::new(self.version_id)
    }

    async fn transform_context<'a>(
        &self,
        _input: TransformContextInput<'a>,
        control: HookControl,
    ) -> Result<TransformContextDecision, HookFailure> {
        self.record(ChainPoint::TransformContext);
        let action = self
            .transforms
            .lock()
            .expect("action lock should not be poisoned")
            .pop_front()
            .unwrap_or(ContextAction::Unchanged);
        match action {
            ContextAction::Unchanged => Ok(TransformContextDecision::Unchanged),
            ContextAction::Pending => pending().await,
            ContextAction::CancelThenPending => {
                control.cancellation().cancel();
                pending().await
            }
        }
    }

    async fn transform_tool_call<'a>(
        &self,
        _input: TransformToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<TransformToolCallDecision, HookFailure> {
        self.record(ChainPoint::TransformToolCall);
        let action = self
            .tool_transforms
            .lock()
            .expect("action lock should not be poisoned")
            .pop_front()
            .unwrap_or(ToolTransformAction::Continue);
        match action {
            ToolTransformAction::Continue => Ok(TransformToolCallDecision::Continue),
            ToolTransformAction::ModifyArguments(arguments) => {
                Ok(TransformToolCallDecision::Modify(
                    TransformToolCallModification::new(Some(arguments), None),
                ))
            }
        }
    }

    async fn decide_tool_call<'a>(
        &self,
        _input: DecideToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<DecideToolCallDecision, HookFailure> {
        self.record(ChainPoint::DecideToolCall);
        let action = self
            .decides
            .lock()
            .expect("action lock should not be poisoned")
            .pop_front()
            .unwrap_or(DecideAction::Execute);
        match action {
            DecideAction::Execute => Ok(DecideToolCallDecision::Execute),
            DecideAction::Block(reason) => Ok(DecideToolCallDecision::Block { reason }),
        }
    }

    async fn after_tool_call<'a>(
        &self,
        _input: AfterToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<AfterToolCallDecision, HookFailure> {
        self.record(ChainPoint::AfterToolCall);
        Ok(AfterToolCallDecision::Keep)
    }

    async fn prepare_next_turn<'a>(
        &self,
        _input: PrepareNextTurnInput<'a>,
        _control: HookControl,
    ) -> Result<PrepareNextTurnDecision, HookFailure> {
        self.record(ChainPoint::PrepareNextTurn);
        Ok(PrepareNextTurnDecision::Continue)
    }
}

fn noop_handler(name: &'static str, log: &ChainLog) -> Arc<ChainScriptableHandler> {
    Arc::new(ChainScriptableHandler::new(
        name,
        HookHandlerVersionId::new(),
        log,
    ))
}

fn chain_of(handlers: Vec<Arc<ChainScriptableHandler>>) -> Arc<ChainHookRuntime> {
    Arc::new(ChainHookRuntime::new(
        handlers
            .into_iter()
            .map(|handler| handler as Arc<dyn HookHandler>)
            .collect(),
    ))
}

fn chain_calls(log: &ChainLog) -> Vec<(&'static str, ChainPoint)> {
    log.lock()
        .expect("chain log lock should not be poisoned")
        .clone()
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
                reason: "must be an object".into(),
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

fn tool_call_turn(call_id: &str, name: &str, arguments: Value) -> Vec<ChatStreamEvent> {
    vec![
        ChatStreamEvent::ToolCallDelta(ToolCallDelta {
            index: 0,
            call_id: Some(CallId::from(call_id)),
            name: Some(name.to_owned()),
            arguments_delta: arguments.to_string(),
        }),
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
    echo_registry_with(operations, false)
}

fn echo_registry_with(
    operations: &Arc<Mutex<Vec<Operation>>>,
    strict_validation: bool,
) -> Arc<dyn ToolRegistry> {
    let mut registry = BuiltinToolRegistry::new(ToolPermissionMode::Allow);
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
        .tool_executor(ToolExecutor::new(
            registry,
            Arc::new(RecordingDurableSink {
                operations: Arc::clone(operations),
            }),
        ))
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

fn chained_loop(
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
    chain: Arc<ChainHookRuntime>,
    operations: &Arc<Mutex<Vec<Operation>>>,
) -> AgentLoop {
    build_loop(
        behaviors,
        LoopLimits::new(8, 4),
        echo_registry(operations),
        Some(chain),
        operations,
    )
}

fn snapshot(operations: &Arc<Mutex<Vec<Operation>>>) -> Vec<Operation> {
    operations
        .lock()
        .expect("operation lock should not be poisoned")
        .clone()
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

/// Hand-built resumable stream: a started loop with one committed user prompt
/// and no iteration boundary.
fn started_stream(version: Option<ExtensionSetVersionId>) -> Vec<DurableAgentEvent> {
    vec![
        DurableAgentEvent::LoopStarted {
            extension_set_version_id: version,
        },
        DurableAgentEvent::MessageAppended {
            message: ChatMessage::user("use echo"),
        },
    ]
}

// 4.2/4.3: a chain reports its pinned extension set version, and the kernel
// durably commits it with `LoopStarted`.
#[tokio::test]
async fn chain_version_is_committed_with_loop_started() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let chain = chain_of(vec![noop_handler("a", &log), noop_handler("b", &log)]);
    let pinned = chain.extension_set_version();
    let agent_loop = chained_loop(VecDeque::from([stop_turn("done")]), chain, &operations);

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    assert_eq!(
        outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );
    let durable = durable_events(&snapshot(&operations));
    assert_eq!(
        durable.first(),
        Some(&DurableAgentEvent::LoopStarted {
            extension_set_version_id: pinned,
        })
    );
    assert!(pinned.is_some(), "a chain always pins its version");
}

// 4.3: a chain of no-op handlers preserves the single-runtime message flow,
// journal write order, and terminal outcome; both handlers observe every hook
// point in declaration order.
#[tokio::test]
async fn noop_chain_preserves_the_single_runtime_event_flow() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let chain = chain_of(vec![noop_handler("a", &log), noop_handler("b", &log)]);
    let agent_loop = chained_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        chain,
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("loop should finish");

    let call = ToolCall {
        call_id: CallId::from("call-1"),
        name: "echo".to_owned(),
        arguments: json!({"value": "one"}),
    };
    assert_eq!(
        outcome.new_messages,
        vec![
            ChatMessage::user("use echo"),
            ChatMessage::assistant("").with_tool_calls(vec![call.clone()]),
            ChatMessage::tool(call.call_id.clone(), json!({"echo": call.arguments})),
            ChatMessage::assistant("done"),
        ]
    );
    // Every hook point reached both handlers in declaration order.
    assert_eq!(
        chain_calls(&log),
        vec![
            ("a", ChainPoint::TransformContext),
            ("b", ChainPoint::TransformContext),
            ("a", ChainPoint::TransformToolCall),
            ("b", ChainPoint::TransformToolCall),
            ("a", ChainPoint::DecideToolCall),
            ("b", ChainPoint::DecideToolCall),
            ("a", ChainPoint::AfterToolCall),
            ("b", ChainPoint::AfterToolCall),
            ("a", ChainPoint::PrepareNextTurn),
            ("b", ChainPoint::PrepareNextTurn),
            ("a", ChainPoint::TransformContext),
            ("b", ChainPoint::TransformContext),
        ]
    );
    // The durable stream is byte-equivalent in shape to a single-runtime run:
    // journal records stay per hook point, and no chain internals leak in.
    let durable = durable_events(&snapshot(&operations));
    assert_eq!(
        durable
            .iter()
            .map(DurableAgentEvent::event_type)
            .collect::<Vec<_>>(),
        vec![
            "loop_started",
            "message_appended",
            "hook_invocation_pending",
            "hook_invocation_completed",
            "message_appended",
            "hook_invocation_pending",
            "hook_invocation_completed",
            "hook_invocation_pending",
            "hook_invocation_completed",
            "tool_execution_started",
            "hook_invocation_pending",
            "hook_invocation_completed",
            "message_appended",
            "hook_invocation_pending",
            "hook_invocation_completed",
            "iteration_completed",
            "hook_invocation_pending",
            "hook_invocation_completed",
            "message_appended",
            "iteration_completed",
            "loop_finished",
        ]
    );
}

// 4.2: rebuilding the same handlers in the same order reproduces the pinned
// version and the resume continues.
#[tokio::test]
async fn resume_with_a_matching_chain_version_continues() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let version_a = HookHandlerVersionId::new();
    let version_b = HookHandlerVersionId::new();
    let chain = chain_of(vec![
        Arc::new(ChainScriptableHandler::new("a", version_a, &log)),
        Arc::new(ChainScriptableHandler::new("b", version_b, &log)),
    ]);
    let pinned = chain
        .extension_set_version()
        .expect("a chain pins its version");
    let agent_loop = chained_loop(VecDeque::from([stop_turn("done")]), chain, &operations);

    let outcome = agent_loop
        .resume(
            "be precise",
            started_stream(Some(pinned)),
            CancellationToken::new(),
        )
        .await
        .expect("a matching chain version must resume");

    assert_eq!(
        outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );
    assert_eq!(outcome.new_messages, vec![ChatMessage::assistant("done")]);
}

// 4.2: a chain with a different version (here: reordered handlers) fails the
// resume closed before any model, tool, or hook action starts.
#[tokio::test]
async fn resume_with_a_mismatched_chain_version_fails_closed() {
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let version_a = HookHandlerVersionId::new();
    let version_b = HookHandlerVersionId::new();
    let recorded = chain_of(vec![
        Arc::new(ChainScriptableHandler::new("a", version_a, &log)),
        Arc::new(ChainScriptableHandler::new("b", version_b, &log)),
    ])
    .extension_set_version()
    .expect("a chain pins its version");

    let operations = Arc::new(Mutex::new(Vec::new()));
    let reordered = chain_of(vec![
        Arc::new(ChainScriptableHandler::new("b", version_b, &log)),
        Arc::new(ChainScriptableHandler::new("a", version_a, &log)),
    ]);
    let current = reordered
        .extension_set_version()
        .expect("a chain pins its version");
    assert_ne!(recorded, current);
    let agent_loop = chained_loop(VecDeque::from([stop_turn("done")]), reordered, &operations);

    let error = agent_loop
        .resume(
            "be precise",
            started_stream(Some(recorded)),
            CancellationToken::new(),
        )
        .await
        .expect_err("a mismatched chain version must refuse the resume");

    assert!(
        matches!(
            error,
            AgentLoopError::Resume {
                reason: ResumeError::ExtensionSetVersionMismatch {
                    recorded: actual_recorded,
                    current: actual_current,
                },
            } if actual_recorded == recorded && actual_current == current
        ),
        "expected a typed extension set version mismatch, got {error:?}"
    );
    assert!(
        snapshot(&operations).is_empty(),
        "no model, tool, hook, or durable action may start after a version mismatch"
    );
    assert!(
        chain_calls(&log).is_empty(),
        "no handler may run after a version mismatch"
    );
}

// 4.2: the version check is skipped when the stream recorded no version or
// the injected runtime reports none.
#[tokio::test]
async fn resume_skips_the_version_check_when_either_side_is_unpinned() {
    // An unpinned stream resumes under a pinned chain.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let chain = chain_of(vec![noop_handler("a", &log)]);
    let agent_loop = chained_loop(VecDeque::from([stop_turn("done")]), chain, &operations);
    let outcome = agent_loop
        .resume("be precise", started_stream(None), CancellationToken::new())
        .await
        .expect("an unpinned stream skips the version check");
    assert_eq!(
        outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );

    // A pinned stream resumes under a runtime that reports no version.
    let operations = Arc::new(Mutex::new(Vec::new()));
    let agent_loop = build_loop(
        VecDeque::from([stop_turn("done")]),
        LoopLimits::new(8, 4),
        echo_registry(&operations),
        None,
        &operations,
    );
    let outcome = agent_loop
        .resume(
            "be precise",
            started_stream(Some(ExtensionSetVersionId::new())),
            CancellationToken::new(),
        )
        .await
        .expect("a runtime without a pinned version skips the version check");
    assert_eq!(
        outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );
}

// 3.4/4.3: arguments modified by the transform chain are re-validated by the
// kernel before the decide phase; a chain producing invalid arguments never
// reaches decide, after, or the tool itself.
#[tokio::test]
async fn chain_modified_arguments_are_revalidated_before_decide() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let invalid = Arc::new(
        ChainScriptableHandler::new("invalid", HookHandlerVersionId::new(), &log)
            .with_tool_transforms([ToolTransformAction::ModifyArguments(json!(42))]),
    );
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "original"})),
            stop_turn("recovered"),
        ]),
        LoopLimits::new(8, 4),
        echo_registry_with(&operations, true),
        Some(chain_of(vec![invalid])),
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("re-validation failures stay model-visible results");

    let result = &outcome.new_messages[2];
    assert_eq!(result.role, ChatRole::Tool);
    assert_eq!(result.tool_call_id, Some(CallId::from("call-1")));
    let error = serde_json::to_value(&result.content).expect("content serializes");
    assert!(
        error.to_string().contains("error"),
        "the committed result must be a structured error, got {error}"
    );
    let observed = chain_calls(&log);
    assert!(
        observed
            .iter()
            .any(|(_, point)| *point == ChainPoint::TransformToolCall),
        "the transform chain must run before re-validation"
    );
    assert!(
        !observed.iter().any(|(_, point)| matches!(
            point,
            ChainPoint::DecideToolCall | ChainPoint::AfterToolCall
        )),
        "re-validation failures must not reach decide or after"
    );
    let recorded = snapshot(&operations);
    assert!(
        !recorded
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. })),
        "the tool must not execute"
    );
    assert!(
        !durable_events(&recorded)
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::ToolExecutionStarted { .. }))
    );
}

// 4.1/4.3: a decide block short-circuits the chain inside the kernel — the
// handler after the blocker never runs, the call produces the fixed
// hook_blocked result, and the after chain still observes it.
#[tokio::test]
async fn decide_block_short_circuits_the_chain_inside_the_kernel() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let blocker = Arc::new(
        ChainScriptableHandler::new("blocker", HookHandlerVersionId::new(), &log)
            .with_decides([DecideAction::Block("policy denied".to_owned())]),
    );
    let agent_loop = chained_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("recovered"),
        ]),
        chain_of(vec![
            noop_handler("auditor", &log),
            blocker,
            noop_handler("never", &log),
        ]),
        &operations,
    );

    let outcome = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect("a blocked call stays a model-visible result");

    assert_eq!(
        outcome.new_messages[2],
        ChatMessage::tool(
            CallId::from("call-1"),
            json!({"error": {"code": "hook_blocked", "message": "policy denied"}}),
        )
    );
    let observed = chain_calls(&log);
    let decide_calls: Vec<_> = observed
        .iter()
        .filter(|(_, point)| *point == ChainPoint::DecideToolCall)
        .collect();
    assert_eq!(
        decide_calls,
        vec![
            &("auditor", ChainPoint::DecideToolCall),
            &("blocker", ChainPoint::DecideToolCall)
        ],
        "the handler after the blocker must not run"
    );
    assert!(
        observed
            .iter()
            .any(|(_, point)| *point == ChainPoint::AfterToolCall),
        "the blocked result still passes through the after chain"
    );
    assert!(
        !snapshot(&operations)
            .iter()
            .any(|operation| matches!(operation, Operation::ToolCall { .. })),
        "a blocked call must not execute"
    );
}

// 4.3: the whole chain is one kernel hook invocation — the per-point deadline
// bounds the entire chain and maps to the typed timeout.
#[tokio::test]
async fn chain_invocation_is_bounded_by_the_point_deadline() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let slow = Arc::new(
        ChainScriptableHandler::new("slow", HookHandlerVersionId::new(), &log)
            .with_transforms([ContextAction::Pending]),
    );
    let limits = LoopLimits::new(8, 4).with_hook_timeouts(
        HookTimeouts::new().with_transform_context(Some(Duration::from_millis(50))),
    );
    let agent_loop = build_loop(
        VecDeque::from([stop_turn("done")]),
        limits,
        echo_registry(&operations),
        Some(chain_of(vec![noop_handler("fast", &log), slow])),
        &operations,
    );

    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("a pending chain handler should hit the point deadline");

    assert!(
        matches!(
            error,
            AgentLoopError::Hook {
                point: HookPoint::TransformContext,
                failure: HookFailure::TimedOut,
            }
        ),
        "expected a typed timeout, got {error:?}"
    );
    assert!(
        durable_events(&snapshot(&operations))
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::LoopFailed { .. }))
    );
}

// 4.3: cancellation inside a chain handler cancels the loop through the
// shared turn token, exactly like a single runtime.
#[tokio::test]
async fn cancellation_inside_a_chain_handler_cancels_the_loop() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let log: ChainLog = Arc::new(Mutex::new(Vec::new()));
    let cancelling = Arc::new(
        ChainScriptableHandler::new("cancelling", HookHandlerVersionId::new(), &log)
            .with_transforms([ContextAction::CancelThenPending]),
    );
    let agent_loop = chained_loop(
        VecDeque::from([stop_turn("done")]),
        chain_of(vec![cancelling]),
        &operations,
    );

    let error = run_once(&agent_loop, CancellationToken::new())
        .await
        .expect_err("a self-cancelled chain should cancel the loop");

    assert!(
        matches!(error, AgentLoopError::Cancelled),
        "expected loop cancellation, got {error:?}"
    );
    assert!(
        durable_events(&snapshot(&operations))
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::LoopCancelled { .. }))
    );
}
