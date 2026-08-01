//! Resume crash matrix (task 5.4) and journal digest semantics (task 5.5).
//!
//! Every phase-1 run crashes at one durable boundary: the crash sink records
//! the surviving prefix and refuses the first event past it. Phase 2 resumes
//! from that prefix with a fresh provider script, hook runtime, and sink.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures_util::stream;
use serde_json::{Value, json};
use stratum_agent::{
    AfterToolCallDecision, AfterToolCallInput, AgentLoop, AgentLoopError, DecideToolCallDecision,
    DecideToolCallInput, HookControl, HookRuntime, LoopCompletionReason, LoopContext, LoopLimits,
    PrepareNextTurnDecision, PrepareNextTurnInput, ResumeError, ToolExecutor,
    TransformContextDecision, TransformContextInput, TransformToolCallDecision,
    TransformToolCallInput,
};
use stratum_core::{
    AgentTelemetryEvent, CallId, ChatMessage, ChatRole, DangerLevel, DurableAgentEvent,
    HookDecisionRecord, HookFailure, HookPoint, ModelId, ToolCallDelta, ToolKind, ToolName,
    ToolSpec,
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
    Hook(HookPoint),
}

#[derive(Debug, Clone)]
enum DecideBehavior {
    Execute,
    Fail(HookFailure),
}

struct RecordingHookRuntime {
    operations: Arc<Mutex<Vec<Operation>>>,
    decides: Mutex<VecDeque<DecideBehavior>>,
}

impl RecordingHookRuntime {
    fn new(operations: &Arc<Mutex<Vec<Operation>>>, decides: Vec<DecideBehavior>) -> Self {
        Self {
            operations: Arc::clone(operations),
            decides: Mutex::new(decides.into()),
        }
    }

    fn record(&self, point: HookPoint) {
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::Hook(point));
    }
}

#[async_trait]
impl HookRuntime for RecordingHookRuntime {
    async fn transform_context<'a>(
        &self,
        _input: TransformContextInput<'a>,
        _control: HookControl,
    ) -> Result<TransformContextDecision, HookFailure> {
        self.record(HookPoint::TransformContext);
        Ok(TransformContextDecision::Unchanged)
    }

    async fn transform_tool_call<'a>(
        &self,
        _input: TransformToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<TransformToolCallDecision, HookFailure> {
        self.record(HookPoint::TransformToolCall);
        Ok(TransformToolCallDecision::Continue)
    }

    async fn decide_tool_call<'a>(
        &self,
        _input: DecideToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<DecideToolCallDecision, HookFailure> {
        self.record(HookPoint::DecideToolCall);
        let behavior = self
            .decides
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .unwrap_or(DecideBehavior::Execute);
        match behavior {
            DecideBehavior::Execute => Ok(DecideToolCallDecision::Execute),
            DecideBehavior::Fail(failure) => Err(failure),
        }
    }

    async fn after_tool_call<'a>(
        &self,
        _input: AfterToolCallInput<'a>,
        _control: HookControl,
    ) -> Result<AfterToolCallDecision, HookFailure> {
        self.record(HookPoint::AfterToolCall);
        Ok(AfterToolCallDecision::Keep)
    }

    async fn prepare_next_turn<'a>(
        &self,
        _input: PrepareNextTurnInput<'a>,
        _control: HookControl,
    ) -> Result<PrepareNextTurnDecision, HookFailure> {
        self.record(HookPoint::PrepareNextTurn);
        Ok(PrepareNextTurnDecision::Continue)
    }
}

/// Records the surviving crash prefix and refuses the first event past it.
struct CrashDurableSink {
    operations: Arc<Mutex<Vec<Operation>>>,
    crash_on: fn(&DurableAgentEvent) -> bool,
}

#[async_trait]
impl DurableEventSink for CrashDurableSink {
    async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
        if (self.crash_on)(&event) {
            return Err(DurableEventSinkError::UnsupportedEvent {
                event_type: "simulated_crash",
            });
        }
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::Durable(event));
        Ok(())
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

fn echo_registry(operations: &Arc<Mutex<Vec<Operation>>>) -> Arc<dyn ToolRegistry> {
    let mut registry = BuiltinToolRegistry::new(ToolPermissionMode::Allow);
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

fn tool_call_turn(call_id: &str, arguments: Value) -> Vec<ChatStreamEvent> {
    vec![
        ChatStreamEvent::ToolCallDelta(ToolCallDelta {
            index: 0,
            call_id: Some(CallId::from(call_id)),
            name: Some("echo".to_owned()),
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

fn build_loop(
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
    hook_runtime: Arc<RecordingHookRuntime>,
    sink: Arc<dyn DurableEventSink>,
    operations: &Arc<Mutex<Vec<Operation>>>,
) -> AgentLoop {
    let provider: Arc<dyn LlmProvider> = Arc::new(ScriptedProvider {
        operations: Arc::clone(operations),
        behaviors: Mutex::new(behaviors),
        model: "scripted:test-model"
            .parse()
            .expect("static model id should parse"),
    });
    AgentLoop::builder()
        .llm_provider(provider)
        .tool_executor(ToolExecutor::new(echo_registry(operations), sink))
        .hook_runtime(hook_runtime)
        .telemetry(Arc::new(NullTelemetrySink))
        .limits(LoopLimits::new(8, 4))
        .build()
        .expect("all agent loop fields should be present")
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

fn transcript(events: &[DurableAgentEvent]) -> Vec<ChatMessage> {
    events
        .iter()
        .filter_map(|event| match event {
            DurableAgentEvent::MessageAppended { message } => Some(message.clone()),
            _ => None,
        })
        .collect()
}

fn hook_points(operations: &[Operation]) -> Vec<HookPoint> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::Hook(point) => Some(*point),
            _ => None,
        })
        .collect()
}

fn tool_executions(operations: &[Operation]) -> usize {
    operations
        .iter()
        .filter(|operation| matches!(operation, Operation::ToolCall { .. }))
        .count()
}

// Crash triggers: the sink refuses the first event matching one of these, so
// the recorded prefix is exactly the crash-surviving stream.
fn on_decide_completed(event: &DurableAgentEvent) -> bool {
    matches!(
        event,
        DurableAgentEvent::HookInvocationCompleted {
            decision: HookDecisionRecord::DecideToolCall(_),
            ..
        }
    )
}

fn on_tool_started(event: &DurableAgentEvent) -> bool {
    matches!(event, DurableAgentEvent::ToolExecutionStarted { .. })
}

fn on_tool_result(event: &DurableAgentEvent) -> bool {
    matches!(
        event,
        DurableAgentEvent::MessageAppended { message } if message.role == ChatRole::Tool
    )
}

fn on_prepare_pending(event: &DurableAgentEvent) -> bool {
    matches!(
        event,
        DurableAgentEvent::HookInvocationPending {
            point: HookPoint::PrepareNextTurn,
            ..
        }
    )
}

fn on_iteration_completed(event: &DurableAgentEvent) -> bool {
    matches!(event, DurableAgentEvent::IterationCompleted { .. })
}

fn on_second_transform_pending(event: &DurableAgentEvent) -> bool {
    matches!(
        event,
        DurableAgentEvent::HookInvocationPending {
            point: HookPoint::TransformContext,
            iteration: 1,
            ..
        }
    )
}

fn on_loop_failed(event: &DurableAgentEvent) -> bool {
    matches!(event, DurableAgentEvent::LoopFailed { .. })
}

struct CrashResume {
    events_before: Vec<DurableAgentEvent>,
    phase1: Vec<Operation>,
    phase2: Vec<Operation>,
    outcome: Result<stratum_agent::LoopOutcome, AgentLoopError>,
}

/// Runs phase 1 up to `crash_on`, then resumes from the surviving events with
/// a fresh runtime, provider script, and sink.
async fn crash_and_resume(
    crash_on: fn(&DurableAgentEvent) -> bool,
    phase1_decides: Vec<DecideBehavior>,
    phase2_decides: Vec<DecideBehavior>,
) -> CrashResume {
    let phase1_operations = Arc::new(Mutex::new(Vec::new()));
    let phase1_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", json!({"value": "one"})),
            stop_turn("unreachable"),
        ]),
        Arc::new(RecordingHookRuntime::new(
            &phase1_operations,
            phase1_decides,
        )),
        Arc::new(CrashDurableSink {
            operations: Arc::clone(&phase1_operations),
            crash_on,
        }),
        &phase1_operations,
    );
    phase1_loop
        .run(
            LoopContext::new("be precise"),
            vec![ChatMessage::user("use echo")],
            CancellationToken::new(),
        )
        .await
        .expect_err("the simulated crash should stop phase 1");
    let phase1 = snapshot(&phase1_operations);
    let events_before = durable_events(&phase1);

    let phase2_operations = Arc::new(Mutex::new(Vec::new()));
    let phase2_loop = build_loop(
        VecDeque::from([stop_turn("done")]),
        Arc::new(RecordingHookRuntime::new(
            &phase2_operations,
            phase2_decides,
        )),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&phase2_operations),
        }),
        &phase2_operations,
    );
    let outcome = phase2_loop
        .resume(
            "be precise",
            events_before.clone(),
            CancellationToken::new(),
        )
        .await;
    CrashResume {
        events_before,
        phase1,
        phase2: snapshot(&phase2_operations),
        outcome,
    }
}

/// Transcript of the same scenario without any crash.
async fn reference_transcript() -> Vec<ChatMessage> {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        Arc::new(RecordingHookRuntime::new(&operations, Vec::new())),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        &operations,
    );
    agent_loop
        .run(
            LoopContext::new("be precise"),
            vec![ChatMessage::user("use echo")],
            CancellationToken::new(),
        )
        .await
        .expect("reference run should finish");
    transcript(&durable_events(&snapshot(&operations)))
}

fn combined_transcript(report: &CrashResume) -> Vec<ChatMessage> {
    let mut messages = transcript(&report.events_before);
    messages.extend(transcript(&durable_events(&report.phase2)));
    messages
}

fn expect_finished(report: &CrashResume) -> &stratum_agent::LoopOutcome {
    let outcome = report
        .outcome
        .as_ref()
        .expect("the resumed run should finish");
    assert_eq!(
        outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );
    outcome
}

// 5.4: crash between the decide pending and its completion; the decide hook
// is retried and the run finishes identically.
#[tokio::test]
async fn resume_after_crash_following_decide_pending_retries_the_hook() {
    let report = crash_and_resume(
        on_decide_completed,
        Vec::new(),
        vec![DecideBehavior::Execute],
    )
    .await;

    expect_finished(&report);
    assert_eq!(
        hook_points(&report.phase2),
        vec![
            HookPoint::DecideToolCall,
            HookPoint::AfterToolCall,
            HookPoint::PrepareNextTurn,
            HookPoint::TransformContext,
        ],
        "the pending decide is retried; later hooks run normally"
    );
    assert_eq!(tool_executions(&report.phase2), 1);
    assert_eq!(combined_transcript(&report), reference_transcript().await);
}

// 5.4 + 5.5: crash after the decide (approval) completed but before
// tool_execution_started; resume reuses the decision and never calls the
// approval handler again.
#[tokio::test]
async fn resume_after_completed_approval_never_asks_again() {
    let report = crash_and_resume(on_tool_started, vec![DecideBehavior::Execute], Vec::new()).await;

    expect_finished(&report);
    assert_eq!(tool_executions(&report.phase1), 0);
    assert_eq!(tool_executions(&report.phase2), 1);
    assert!(
        !hook_points(&report.phase2).contains(&HookPoint::DecideToolCall),
        "a completed approval decision must be reused without calling the runtime"
    );
    assert_eq!(combined_transcript(&report), reference_transcript().await);
}

// 5.4: crash after tool_execution_started without a committed result; the
// call re-executes under the at-least-once stance.
#[tokio::test]
async fn resume_after_started_without_result_reexecutes_the_tool() {
    let report = crash_and_resume(on_tool_result, vec![DecideBehavior::Execute], Vec::new()).await;

    expect_finished(&report);
    assert_eq!(tool_executions(&report.phase1), 1);
    assert_eq!(
        tool_executions(&report.phase2),
        1,
        "a started call with an unknown outcome re-executes"
    );
    assert!(
        !hook_points(&report.phase2).contains(&HookPoint::DecideToolCall),
        "the completed decide decision is still reused"
    );
    assert_eq!(combined_transcript(&report), reference_transcript().await);
}

// 5.4: crash after the result commit but before the prepare invocation; the
// iteration boundary closes through a fresh prepare call.
#[tokio::test]
async fn resume_after_result_commit_closes_the_iteration() {
    let report = crash_and_resume(on_prepare_pending, Vec::new(), Vec::new()).await;

    expect_finished(&report);
    assert_eq!(tool_executions(&report.phase2), 0);
    assert_eq!(
        hook_points(&report.phase2),
        vec![HookPoint::PrepareNextTurn, HookPoint::TransformContext],
        "only the missing boundary work runs"
    );
    assert_eq!(combined_transcript(&report), reference_transcript().await);
}

// 5.4: crash after the prepare decision completed but before the iteration
// boundary; the boundary closes by replaying the journaled decision.
#[tokio::test]
async fn resume_after_prepare_completed_replays_the_boundary_decision() {
    let report = crash_and_resume(on_iteration_completed, Vec::new(), Vec::new()).await;

    expect_finished(&report);
    assert_eq!(tool_executions(&report.phase2), 0);
    assert!(
        !hook_points(&report.phase2).contains(&HookPoint::PrepareNextTurn),
        "the completed prepare decision must be reused"
    );
    assert_eq!(combined_transcript(&report), reference_transcript().await);
}

// 5.4: crash exactly at the iteration boundary; the next iteration starts a
// fresh model request from the frontier.
#[tokio::test]
async fn resume_at_iteration_boundary_continues_with_the_next_model_request() {
    let report = crash_and_resume(on_second_transform_pending, Vec::new(), Vec::new()).await;

    expect_finished(&report);
    assert_eq!(tool_executions(&report.phase2), 0);
    assert_eq!(
        hook_points(&report.phase2),
        vec![HookPoint::TransformContext],
        "iteration 1 starts fresh from the frontier"
    );
    assert_eq!(combined_transcript(&report), reference_transcript().await);
}

// 5.4: a run carrying a terminal event refuses to resume.
#[tokio::test]
async fn resume_refuses_a_finished_run() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let agent_loop = build_loop(
        VecDeque::from([stop_turn("done")]),
        Arc::new(RecordingHookRuntime::new(&operations, Vec::new())),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        &operations,
    );
    agent_loop
        .run(
            LoopContext::new("be precise"),
            vec![ChatMessage::user("question")],
            CancellationToken::new(),
        )
        .await
        .expect("reference run should finish");
    let events = durable_events(&snapshot(&operations));

    let phase2_operations = Arc::new(Mutex::new(Vec::new()));
    let phase2_loop = build_loop(
        VecDeque::new(),
        Arc::new(RecordingHookRuntime::new(&phase2_operations, Vec::new())),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&phase2_operations),
        }),
        &phase2_operations,
    );
    let error = phase2_loop
        .resume("be precise", events, CancellationToken::new())
        .await
        .expect_err("a finished run must refuse resume");

    assert!(matches!(
        error,
        AgentLoopError::Resume {
            reason: ResumeError::TerminalEvent,
        }
    ));
    assert!(
        snapshot(&phase2_operations).is_empty(),
        "a refused resume starts no model, tool, or hook action"
    );
}

// 5.5: a journaled address whose payload changed across the crash boundary
// fails closed without calling the runtime.
#[tokio::test]
async fn resume_fails_closed_on_a_digest_mismatch() {
    let mut report =
        crash_and_resume(on_tool_started, vec![DecideBehavior::Execute], Vec::new()).await;
    // Tamper with the committed tool call: the rebuilt invocation no longer
    // matches the journaled digest.
    for event in &mut report.events_before {
        if let DurableAgentEvent::MessageAppended { message } = event
            && !message.tool_calls.is_empty()
        {
            message.tool_calls[0].arguments = json!({"value": "tampered"});
        }
    }

    let phase2_operations = Arc::new(Mutex::new(Vec::new()));
    let phase2_loop = build_loop(
        VecDeque::from([stop_turn("done")]),
        Arc::new(RecordingHookRuntime::new(&phase2_operations, Vec::new())),
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&phase2_operations),
        }),
        &phase2_operations,
    );
    let error = phase2_loop
        .resume("be precise", report.events_before, CancellationToken::new())
        .await
        .expect_err("a digest mismatch must fail closed");

    assert!(matches!(
        error,
        AgentLoopError::Resume {
            reason: ResumeError::HookDigestMismatch {
                point: HookPoint::TransformToolCall,
            },
        }
    ));
    let phase2 = snapshot(&phase2_operations);
    assert!(
        hook_points(&phase2).is_empty(),
        "the runtime is never called"
    );
    assert!(
        !phase2
            .iter()
            .any(|operation| matches!(operation, Operation::ChatStream(_)))
    );
}

// 5.5: a pending-only invocation retries under its original identity instead
// of creating a second logical invocation.
#[tokio::test]
async fn resume_retries_a_pending_invocation_under_its_original_identity() {
    let report = crash_and_resume(
        on_decide_completed,
        Vec::new(),
        vec![DecideBehavior::Execute],
    )
    .await;
    expect_finished(&report);

    let pending_id = report
        .events_before
        .iter()
        .find_map(|event| match event {
            DurableAgentEvent::HookInvocationPending {
                invocation_id,
                point: HookPoint::DecideToolCall,
                ..
            } => Some(*invocation_id),
            _ => None,
        })
        .expect("phase 1 journaled the decide pending");
    let phase2_events = durable_events(&report.phase2);
    let completed_id = phase2_events
        .iter()
        .find_map(|event| match event {
            DurableAgentEvent::HookInvocationCompleted {
                invocation_id,
                decision: HookDecisionRecord::DecideToolCall(_),
            } => Some(*invocation_id),
            _ => None,
        })
        .expect("phase 2 journaled the retried decision");
    assert_eq!(
        pending_id, completed_id,
        "the retry reuses the original invocation identity"
    );
    assert!(
        !phase2_events.iter().any(|event| matches!(
            event,
            DurableAgentEvent::HookInvocationPending {
                point: HookPoint::DecideToolCall,
                ..
            }
        )),
        "no second pending record is created for the retried invocation"
    );
}

// 5.5: a journaled failure is reproduced without calling the runtime.
#[tokio::test]
async fn resume_reproduces_a_journaled_failure() {
    let report = crash_and_resume(
        on_loop_failed,
        vec![DecideBehavior::Fail(HookFailure::HandlerFailed)],
        Vec::new(),
    )
    .await;
    assert!(
        report.events_before.iter().any(|event| matches!(
            event,
            DurableAgentEvent::HookInvocationFailed {
                failure: HookFailure::HandlerFailed,
                ..
            }
        )),
        "phase 1 journaled the typed failure"
    );

    let error = report
        .outcome
        .expect_err("the journaled failure must be reproduced");
    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::DecideToolCall,
            failure: HookFailure::HandlerFailed,
        }
    ));
    assert!(
        !hook_points(&report.phase2).contains(&HookPoint::DecideToolCall),
        "the failed hook is never called again"
    );
}
