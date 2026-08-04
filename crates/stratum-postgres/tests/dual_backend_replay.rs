//! Dual-backend replay alignment (add-postgres-execution-storage task 5.2):
//! the same durable event sequence persisted through the filesystem and the
//! Postgres `DurableEventSink` backends must read back event-by-event equal
//! and resume through the kernel's public `AgentLoop::resume` entry to
//! identical rebuild results — committed context, iteration frontier, hook
//! journal reuse/retry/reproduce judgments, and terminal refusal behavior.
//!
//! The kernel's replay function itself is crate-private, so the comparison
//! runs at the public resume boundary: both backends' read-back streams are
//! handed to `AgentLoop::resume` with a fresh scripted runtime, and every
//! observable of the resumed run is compared. The filesystem side reads
//! through `read_events_from_checkpoint`, the production accelerated reader,
//! so the run also proves a checkpoint window replays identically to the
//! Postgres full stream. All tests are `#[ignore]` by default; run them
//! against the crate's compose stack via `make test-integration` (or
//! `cargo test -p stratum-postgres --test dual_backend_replay -- --ignored`).

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use async_trait::async_trait;
use futures_util::stream;
use serde_json::{Value, json};
use stratum_agent::{
    AfterToolCallDecision, AfterToolCallInput, AgentLoop, AgentLoopError, COMPACTION_MARKER_PREFIX,
    DecideToolCallDecision, DecideToolCallInput, HookControl, HookRuntime, LoopCompletionReason,
    LoopContext, LoopLimits, LoopOutcome, PrepareNextTurnDecision, PrepareNextTurnInput,
    ResumeError, ToolExecutor, TransformContextDecision, TransformContextInput,
    TransformToolCallDecision, TransformToolCallInput,
};
use stratum_core::{
    AgentId, AgentTelemetryEvent, CallId, ChatMessage, DangerLevel, DurableAgentEvent, HookFailure,
    HookInvocationId, HookPoint, ModelId, SessionId, ToolCallDelta, ToolKind, ToolName, ToolSpec,
    TurnId,
};
use stratum_infra::agent_event_sink::{read_events, read_events_from_checkpoint};
use stratum_infra::{
    DurableEventSink, DurableEventSinkError, FilesystemDurableEventSink, TelemetryEventSink,
};
use stratum_llm::{
    ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, FinishReason, LlmError, LlmProvider,
};
use stratum_postgres::{PostgresBackend, read_events as pg_read_events};
use stratum_tools::{
    BuiltinToolRegistry, Tool, ToolError, ToolInput, ToolOutput, ToolPermissionMode, ToolRegistry,
};
use tokio_util::sync::CancellationToken;

fn test_url() -> String {
    std::env::var("STRATUM_POSTGRES_TEST_URL").unwrap_or_else(|_| {
        "postgres://stratum:stratum@127.0.0.1:45432/stratum_test?sslmode=disable".to_owned()
    })
}

async fn backend() -> PostgresBackend {
    PostgresBackend::connect(&test_url())
        .await
        .expect("postgres backend connects and migrates")
}

static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

struct TestRunDir(PathBuf);

impl TestRunDir {
    fn new(test_name: &str) -> Self {
        let unique = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "stratum-postgres-dual-replay-{test_name}-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("test run directory should be creatable");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestRunDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

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

/// Hook runtime recording every invoked point and every `transform_context`
/// context view, with scripted `decide_tool_call` and `prepare_next_turn`
/// decisions.
struct ScriptedHookRuntime {
    operations: Arc<Mutex<Vec<Operation>>>,
    decides: Mutex<VecDeque<DecideBehavior>>,
    prepares: Mutex<VecDeque<PrepareNextTurnDecision>>,
    transform_views: Mutex<Vec<Vec<ChatMessage>>>,
}

impl ScriptedHookRuntime {
    fn new(operations: &Arc<Mutex<Vec<Operation>>>) -> Self {
        Self {
            operations: Arc::clone(operations),
            decides: Mutex::new(VecDeque::new()),
            prepares: Mutex::new(VecDeque::new()),
            transform_views: Mutex::new(Vec::new()),
        }
    }

    fn with_decides(mut self, decides: Vec<DecideBehavior>) -> Self {
        self.decides = Mutex::new(decides.into());
        self
    }

    fn with_prepares(mut self, prepares: Vec<PrepareNextTurnDecision>) -> Self {
        self.prepares = Mutex::new(prepares.into());
        self
    }

    fn record(&self, point: HookPoint) {
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::Hook(point));
    }

    fn transform_views(&self) -> Vec<Vec<ChatMessage>> {
        self.transform_views
            .lock()
            .expect("view lock should not be poisoned")
            .clone()
    }
}

#[async_trait]
impl HookRuntime for ScriptedHookRuntime {
    async fn transform_context<'a>(
        &self,
        input: TransformContextInput<'a>,
        _control: HookControl,
    ) -> Result<TransformContextDecision, HookFailure> {
        self.record(HookPoint::TransformContext);
        self.transform_views
            .lock()
            .expect("view lock should not be poisoned")
            .push(input.snapshot.context.messages.clone());
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
        Ok(self
            .prepares
            .lock()
            .expect("prepare lock should not be poisoned")
            .pop_front()
            .unwrap_or(PrepareNextTurnDecision::Continue))
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
    hook_runtime: Arc<ScriptedHookRuntime>,
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

fn hook_points(operations: &[Operation]) -> Vec<HookPoint> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::Hook(point) => Some(*point),
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

fn tool_executions(operations: &[Operation]) -> usize {
    operations
        .iter()
        .filter(|operation| matches!(operation, Operation::ToolCall { .. }))
        .count()
}

/// A fixed identity used to erase freshly generated invocation ids when
/// comparing the durable streams two independent resumed runs appended.
fn normalized_invocation_id() -> HookInvocationId {
    "00000000-0000-0000-0000-000000000000"
        .parse()
        .expect("the nil uuid is a valid invocation id")
}

/// Replaces every hook invocation id with a fixed identity: two independent
/// resumes journal fresh invocation identities for the hooks they invoke, so
/// the streams are comparable only after erasing them.
fn normalize_invocation_ids(events: &[DurableAgentEvent]) -> Vec<DurableAgentEvent> {
    events
        .iter()
        .cloned()
        .map(|event| match event {
            DurableAgentEvent::HookInvocationPending {
                point,
                iteration,
                call_id,
                input_digest,
                ..
            } => DurableAgentEvent::HookInvocationPending {
                invocation_id: normalized_invocation_id(),
                point,
                iteration,
                call_id,
                input_digest,
            },
            DurableAgentEvent::HookInvocationCompleted { decision, .. } => {
                DurableAgentEvent::HookInvocationCompleted {
                    invocation_id: normalized_invocation_id(),
                    decision,
                }
            }
            DurableAgentEvent::HookInvocationFailed { failure, .. } => {
                DurableAgentEvent::HookInvocationFailed {
                    invocation_id: normalized_invocation_id(),
                    failure,
                }
            }
            other => other,
        })
        .collect()
}

fn on_second_iteration_boundary(event: &DurableAgentEvent) -> bool {
    matches!(
        event,
        DurableAgentEvent::IterationCompleted { iteration: 1, .. }
    )
}

fn on_loop_failed(event: &DurableAgentEvent) -> bool {
    matches!(event, DurableAgentEvent::LoopFailed { .. })
}

/// Phase 1 of the alignment scenario: two tool-calling iterations. The first
/// prepare boundary commits a transcript compaction of the three prompts
/// (`upto == 3 == iteration_start`, retaining the clean
/// assistant/tool-result group), and the second iteration's prepare decision
/// is journaled but its boundary never lands — the crash hits
/// `IterationCompleted{1}`. The surviving stream covers all five hook points
/// (Pending + Completed each), `TranscriptCompacted`, one committed iteration
/// boundary, and no terminal event; resume must close iteration 1 by reusing
/// the journaled prepare decision, then continue from the frontier.
async fn alignment_crash_stream() -> Vec<DurableAgentEvent> {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let hook_runtime = Arc::new(ScriptedHookRuntime::new(&operations).with_prepares(vec![
        PrepareNextTurnDecision::Compact {
            upto: 3,
            summary: ChatMessage::system("earlier exchange summary"),
        },
    ]));
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", json!({"value": "one"})),
            tool_call_turn("call-2", json!({"value": "two"})),
        ]),
        hook_runtime,
        Arc::new(CrashDurableSink {
            operations: Arc::clone(&operations),
            crash_on: on_second_iteration_boundary,
        }),
        &operations,
    );
    agent_loop
        .run(
            LoopContext::new("be precise"),
            vec![
                ChatMessage::user("first question"),
                ChatMessage::user("second question"),
                ChatMessage::user("use echo"),
            ],
            CancellationToken::new(),
        )
        .await
        .expect_err("the simulated crash should stop phase 1");
    durable_events(&snapshot(&operations))
}

/// Everything one backend contributes to the comparison: the run directory or
/// turn the events were persisted under, plus the fresh-runtime resume run.
struct BackendSide {
    run: TestRunDir,
    session_id: SessionId,
    agent_id: AgentId,
    turn_id: TurnId,
}

impl BackendSide {
    fn new(test_name: &str) -> Self {
        Self {
            run: TestRunDir::new(test_name),
            session_id: SessionId::new(),
            agent_id: AgentId::new(),
            turn_id: TurnId::new(),
        }
    }
}

/// Persists the same event sequence through both backends and returns the
/// addressing needed to read each side back.
async fn persist_both(test_name: &str, events: &[DurableAgentEvent]) -> BackendSide {
    let side = BackendSide::new(test_name);

    let fs_sink = FilesystemDurableEventSink::new(side.run.path());
    for event in events {
        fs_sink
            .append(event.clone())
            .await
            .expect("filesystem append should succeed");
    }

    let backend = backend().await;
    let pg_sink = backend
        .event_sink(side.session_id, side.agent_id, side.turn_id)
        .await
        .expect("postgres sink opens");
    for event in events {
        pg_sink
            .append(event.clone())
            .await
            .expect("postgres append should succeed");
    }
    side
}

/// Reads both backends back and asserts the persisted streams are
/// event-by-event identical to each other and to the appended sequence, and
/// that the Postgres jsonb payload of every row is field-by-field equal to
/// the filesystem JSONL line of the same sequence.
async fn assert_persisted_streams_align(side: &BackendSide, events: &[DurableAgentEvent]) {
    let fs_events = read_events(side.run.path()).expect("filesystem events read back");
    let backend = backend().await;
    let pg_events = pg_read_events(backend.pool(), side.turn_id)
        .await
        .expect("postgres events read back");
    assert_eq!(fs_events, events, "filesystem stream round-trips");
    assert_eq!(pg_events, events, "postgres stream round-trips");
    assert_eq!(fs_events, pg_events, "both backends read back identically");

    // Payload parity: the jsonb payload column is field-by-field equal to the
    // JSONL line of the same event (jsonb normalizes key order/whitespace).
    let raw = std::fs::read_to_string(side.run.path().join("events.jsonl"))
        .expect("event log should be readable");
    let jsonl_lines: Vec<Value> = raw
        .lines()
        .map(|line| serde_json::from_str(line).expect("jsonl line parses"))
        .collect();
    let payloads: Vec<Value> = sqlx::query_scalar(
        "SELECT payload FROM durable_events WHERE turn_id = $1 ORDER BY seq ASC",
    )
    .bind(side.turn_id.as_uuid())
    .fetch_all(backend.pool())
    .await
    .expect("payloads query succeeds");
    assert_eq!(payloads.len(), jsonl_lines.len());
    for (index, (payload, line)) in payloads.iter().zip(jsonl_lines.iter()).enumerate() {
        assert_eq!(
            payload,
            line,
            "payload of seq {} must match the jsonl line field-by-field",
            index + 1
        );
    }
}

struct ResumedRun {
    outcome: Result<LoopOutcome, AgentLoopError>,
    operations: Vec<Operation>,
    transform_views: Vec<Vec<ChatMessage>>,
}

/// Resumes one backend's read-back stream through the kernel's public resume
/// entry with a fresh provider, hook runtime, and recording sink.
async fn resume_with_fresh_runtime(
    events: Vec<DurableAgentEvent>,
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
) -> ResumedRun {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let hook_runtime = Arc::new(ScriptedHookRuntime::new(&operations));
    let transform_views = Arc::clone(&hook_runtime);
    let agent_loop = build_loop(
        behaviors,
        hook_runtime,
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(&operations),
        }),
        &operations,
    );
    let outcome = agent_loop
        .resume("be precise", events, CancellationToken::new())
        .await;
    ResumedRun {
        outcome,
        transform_views: transform_views.transform_views(),
        operations: snapshot(&operations),
    }
}

// Spec scenario "双后端 replay 对齐": one stream covering all five hook
// points, compaction, and iteration boundaries is persisted through both
// backends; resume rebuilds must agree on the committed context, the
// iteration frontier, and every hook journal reuse judgment.
#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn dual_backend_replay_alignment() {
    let events = alignment_crash_stream().await;

    // The crash stream covers every hook point's Pending + Completed pair,
    // the compaction, and one committed iteration boundary, with no terminal
    // event; the second boundary is journaled but never lands.
    for point in [
        HookPoint::TransformContext,
        HookPoint::TransformToolCall,
        HookPoint::DecideToolCall,
        HookPoint::AfterToolCall,
        HookPoint::PrepareNextTurn,
    ] {
        assert!(
            events.iter().any(|event| matches!(
                event,
                DurableAgentEvent::HookInvocationPending { point: p, .. } if *p == point
            )),
            "crash stream should journal a pending {point:?}"
        );
    }
    assert!(
        events
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::TranscriptCompacted { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        DurableAgentEvent::IterationCompleted { iteration: 0, .. }
    )));
    assert!(
        !events.iter().any(|event| matches!(
            event,
            DurableAgentEvent::IterationCompleted { iteration: 1, .. }
        )),
        "the crash must hit before the second iteration boundary"
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        DurableAgentEvent::LoopFinished { .. }
            | DurableAgentEvent::LoopFailed { .. }
            | DurableAgentEvent::LoopCancelled { .. }
    )));

    let side = persist_both("alignment", &events).await;
    assert_persisted_streams_align(&side, &events).await;

    // Resume each side with an identical fresh runtime. The filesystem side
    // reads through the production checkpoint-accelerated reader — the
    // compaction's checkpoint exists because the same sink committed the
    // boundary — so this also proves a checkpoint window replays identically
    // to the Postgres full stream.
    let fs_window = read_events_from_checkpoint(side.run.path())
        .expect("filesystem checkpoint-window read succeeds");
    let backend = backend().await;
    let pg_full = pg_read_events(backend.pool(), side.turn_id)
        .await
        .expect("postgres events read back");
    assert!(
        fs_window.len() < pg_full.len(),
        "the checkpoint window should skip the compacted prefix"
    );

    let fs_run = resume_with_fresh_runtime(fs_window, VecDeque::from([stop_turn("final")])).await;
    let pg_run = resume_with_fresh_runtime(pg_full, VecDeque::from([stop_turn("final")])).await;

    // Both resumes finish the loop identically from the frontier.
    let fs_outcome = fs_run.outcome.as_ref().expect("filesystem resume finishes");
    let pg_outcome = pg_run.outcome.as_ref().expect("postgres resume finishes");
    assert_eq!(fs_outcome, pg_outcome, "resume outcomes must be identical");
    assert_eq!(
        fs_outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );

    // Committed context: the transform_context view of the resumed frontier
    // iteration is rebuilt identically from both backends, with the
    // compaction marker at index 0.
    assert_eq!(
        fs_run.transform_views.len(),
        1,
        "the resumed run transforms once at the frontier"
    );
    assert_eq!(
        fs_run.transform_views, pg_run.transform_views,
        "the committed context must rebuild identically from both backends"
    );
    assert_eq!(
        fs_run.transform_views[0][0],
        ChatMessage::system(format!(
            "{COMPACTION_MARKER_PREFIX}\nearlier exchange summary"
        )),
        "the leading message should be the compaction marker"
    );

    // Iteration frontier: the only model request of the resumed run is the
    // frontier iteration's, and it is identical on both sides.
    assert_eq!(
        chat_requests(&fs_run.operations),
        chat_requests(&pg_run.operations),
        "the frontier model request must be identical"
    );
    assert_eq!(chat_requests(&fs_run.operations).len(), 1);

    // Hook journal reuse: the crashed run's completed decisions are reused
    // without calling the runtime again. Iteration 1's boundary closes by
    // replaying the journaled prepare decision — no PrepareNextTurn call —
    // and the only fresh hook invocation is the frontier iteration's
    // transform_context (the loop's final no-tool-call turn runs no prepare
    // hook by kernel design).
    assert_eq!(
        hook_points(&fs_run.operations),
        vec![HookPoint::TransformContext]
    );
    assert_eq!(
        hook_points(&fs_run.operations),
        hook_points(&pg_run.operations)
    );
    assert!(
        durable_events(&fs_run.operations)
            .iter()
            .any(|event| matches!(
                event,
                DurableAgentEvent::IterationCompleted { iteration: 1, .. }
            )),
        "iteration 1's boundary must close through the journaled prepare decision"
    );
    assert_eq!(tool_executions(&fs_run.operations), 0);
    assert_eq!(tool_executions(&pg_run.operations), 0);

    // The durable streams appended by both resumes are identical once the
    // freshly generated invocation identities are erased.
    assert_eq!(
        normalize_invocation_ids(&durable_events(&fs_run.operations)),
        normalize_invocation_ids(&durable_events(&pg_run.operations)),
        "both resumes must journal the same new durable events"
    );
}

// A journaled hook failure persisted through either backend is reproduced by
// resume without calling the runtime again.
#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn dual_backend_resume_reproduces_journaled_hook_failure() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let hook_runtime = Arc::new(
        ScriptedHookRuntime::new(&operations)
            .with_decides(vec![DecideBehavior::Fail(HookFailure::HandlerFailed)]),
    );
    let agent_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1", json!({"value": "one"}))]),
        hook_runtime,
        Arc::new(CrashDurableSink {
            operations: Arc::clone(&operations),
            crash_on: on_loop_failed,
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
        .expect_err("the simulated crash should stop phase 1");
    let events = durable_events(&snapshot(&operations));
    assert!(
        events.iter().any(|event| matches!(
            event,
            DurableAgentEvent::HookInvocationFailed {
                failure: HookFailure::HandlerFailed,
                ..
            }
        )),
        "phase 1 journaled the typed failure"
    );

    let side = persist_both("failure", &events).await;
    assert_persisted_streams_align(&side, &events).await;

    let fs_events = read_events(side.run.path()).expect("filesystem events read back");
    let backend = backend().await;
    let pg_events = pg_read_events(backend.pool(), side.turn_id)
        .await
        .expect("postgres events read back");
    let fs_run = resume_with_fresh_runtime(fs_events, VecDeque::new()).await;
    let pg_run = resume_with_fresh_runtime(pg_events, VecDeque::new()).await;

    for (name, run) in [("filesystem", &fs_run), ("postgres", &pg_run)] {
        let error = run
            .outcome
            .as_ref()
            .expect_err("resume must reproduce the journaled failure");
        assert!(
            matches!(
                error,
                AgentLoopError::Hook {
                    point: HookPoint::DecideToolCall,
                    failure: HookFailure::HandlerFailed,
                }
            ),
            "unexpected {name} error: {error:?}"
        );
        assert!(
            hook_points(&run.operations).is_empty(),
            "the {name} resume must reproduce the failure without calling the runtime"
        );
    }
}

// A run carrying a terminal event refuses to resume, identically on both
// backends, and starts no model, tool, or hook action.
#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn dual_backend_resume_refuses_terminal_run() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let agent_loop = build_loop(
        VecDeque::from([stop_turn("done")]),
        Arc::new(ScriptedHookRuntime::new(&operations)),
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
        .expect("the reference run should finish");
    let events = durable_events(&snapshot(&operations));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::LoopFinished { .. }))
    );

    let side = persist_both("terminal", &events).await;
    assert_persisted_streams_align(&side, &events).await;

    let fs_events = read_events(side.run.path()).expect("filesystem events read back");
    let backend = backend().await;
    let pg_events = pg_read_events(backend.pool(), side.turn_id)
        .await
        .expect("postgres events read back");
    let fs_run = resume_with_fresh_runtime(fs_events, VecDeque::new()).await;
    let pg_run = resume_with_fresh_runtime(pg_events, VecDeque::new()).await;

    for (name, run) in [("filesystem", &fs_run), ("postgres", &pg_run)] {
        let error = run
            .outcome
            .as_ref()
            .expect_err("resume of a finished run must refuse");
        assert!(
            matches!(
                error,
                AgentLoopError::Resume {
                    reason: ResumeError::TerminalEvent,
                }
            ),
            "unexpected {name} error: {error:?}"
        );
        assert!(
            run.operations.is_empty(),
            "a refused {name} resume starts no model, tool, or hook action"
        );
    }
}
