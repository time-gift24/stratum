//! `prepare_resume` seam (task 7.5): the pure prepare path validates the
//! replay window without any side effect, the prepared value resumes
//! equivalently to `AgentLoop::resume`, and it stays bound to the exact loop
//! that prepared it.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures_util::stream;
use stratum_agent::{
    AfterToolCallDecision, AfterToolCallInput, AgentLoop, DecideToolCallDecision,
    DecideToolCallInput, HookControl, HookRuntime, LoopLimits, NoopHookRuntime,
    PrepareNextTurnDecision, PrepareNextTurnInput, PreparedResume, ResumeError, ToolExecutor,
    TransformContextDecision, TransformContextInput, TransformToolCallDecision,
    TransformToolCallInput,
};
use stratum_core::{
    AgentTelemetryEvent, ChatMessage, ChatRole, DurableAgentEvent, ExtensionSetVersionId,
    HookFailure, ModelId,
};
use stratum_infra::{DurableEventSink, DurableEventSinkError, TelemetryEventSink};
use stratum_llm::{
    ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, FinishReason, LlmError, LlmProvider,
};
use stratum_tools::{BuiltinToolRegistry, ToolPermissionMode, ToolRegistry};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<DurableAgentEvent>>,
}

impl RecordingSink {
    fn events(&self) -> Vec<DurableAgentEvent> {
        self.events
            .lock()
            .expect("event lock should not be poisoned")
            .clone()
    }
}

#[async_trait]
impl DurableEventSink for RecordingSink {
    async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
        self.events
            .lock()
            .expect("event lock should not be poisoned")
            .push(event);
        Ok(())
    }
}

struct ScriptedProvider {
    requests: Mutex<usize>,
    behaviors: Mutex<VecDeque<Vec<ChatStreamEvent>>>,
    model: ModelId,
}

impl ScriptedProvider {
    fn requests(&self) -> usize {
        *self
            .requests
            .lock()
            .expect("request lock should not be poisoned")
    }
}

#[async_trait]
impl LlmProvider for ScriptedProvider {
    fn model_id(&self) -> ModelId {
        self.model.clone()
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
        Err(LlmError::UnsupportedCapability("chat"))
    }

    async fn chat_stream(&self, _request: ChatRequest) -> Result<ChatStream, LlmError> {
        *self
            .requests
            .lock()
            .expect("request lock should not be poisoned") += 1;
        let events = self
            .behaviors
            .lock()
            .expect("behavior lock should not be poisoned")
            .pop_front()
            .ok_or(LlmError::MockExhausted)?;
        Ok(Box::pin(stream::iter(events.into_iter().map(Ok))))
    }
}

struct NullTelemetrySink;

impl TelemetryEventSink for NullTelemetrySink {
    fn emit(&self, _event: AgentTelemetryEvent) {}
}

/// Hook runtime with no-op decisions that pins an extension set version.
struct VersionedHookRuntime {
    version: ExtensionSetVersionId,
}

#[async_trait]
impl HookRuntime for VersionedHookRuntime {
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

    fn extension_set_version(&self) -> Option<ExtensionSetVersionId> {
        Some(self.version)
    }
}

struct Fixture {
    agent_loop: Arc<AgentLoop>,
    sink: Arc<RecordingSink>,
    provider: Arc<ScriptedProvider>,
}

fn fixture(
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
    hook_runtime: Arc<dyn HookRuntime>,
) -> Fixture {
    let provider = Arc::new(ScriptedProvider {
        requests: Mutex::new(0),
        behaviors: Mutex::new(behaviors),
        model: "scripted:test-model"
            .parse()
            .expect("static model id should parse"),
    });
    let sink = Arc::new(RecordingSink::default());
    let registry: Arc<dyn ToolRegistry> =
        Arc::new(BuiltinToolRegistry::new(ToolPermissionMode::Allow));
    let agent_loop = Arc::new(
        AgentLoop::builder()
            .llm_provider(provider.clone())
            .tool_executor(ToolExecutor::new(registry, sink.clone()))
            .hook_runtime(hook_runtime)
            .telemetry(Arc::new(NullTelemetrySink))
            .limits(LoopLimits::new(8, 4))
            .build()
            .expect("all agent loop fields should be present"),
    );
    Fixture {
        agent_loop,
        sink,
        provider,
    }
}

fn noop_fixture(behaviors: VecDeque<Vec<ChatStreamEvent>>) -> Fixture {
    fixture(behaviors, Arc::new(NoopHookRuntime))
}

/// Hand-built resumable stream: a started loop with one committed user prompt
/// and no iteration boundary.
fn started_events() -> Vec<DurableAgentEvent> {
    vec![
        DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        },
        DurableAgentEvent::MessageAppended {
            message: ChatMessage::user("hello"),
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

#[tokio::test]
async fn prepared_resume_runs_equivalently_to_direct_resume() {
    let direct = noop_fixture(VecDeque::from([stop_turn("done")]));
    let direct_outcome = direct
        .agent_loop
        .resume("be precise", started_events(), CancellationToken::new())
        .await
        .expect("direct resume should finish");

    let prepared = noop_fixture(VecDeque::from([stop_turn("done")]));
    let prepared_value = prepared
        .agent_loop
        .prepare_resume("be precise", started_events())
        .expect("prepare should succeed");
    let prepared_outcome = prepared_value
        .run(CancellationToken::new())
        .await
        .expect("prepared run should finish");

    assert_eq!(prepared_outcome, direct_outcome);
    assert_eq!(prepared.provider.requests(), direct.provider.requests());

    // Same continuation events on both paths; hook invocation identities are
    // fresh per run, so journal events compare by address, digest, and
    // decision rather than by invocation id.
    let prepared_events = prepared.sink.events();
    let direct_events = direct.sink.events();
    assert_eq!(prepared_events.len(), direct_events.len());
    for (prepared_event, direct_event) in prepared_events.iter().zip(&direct_events) {
        match (prepared_event, direct_event) {
            (
                DurableAgentEvent::HookInvocationPending {
                    point,
                    iteration,
                    call_id,
                    input_digest,
                    ..
                },
                DurableAgentEvent::HookInvocationPending {
                    point: direct_point,
                    iteration: direct_iteration,
                    call_id: direct_call_id,
                    input_digest: direct_digest,
                    ..
                },
            ) => {
                assert_eq!(
                    (point, iteration, call_id, input_digest),
                    (
                        direct_point,
                        direct_iteration,
                        direct_call_id,
                        direct_digest
                    )
                );
            }
            (
                DurableAgentEvent::HookInvocationCompleted { decision, .. },
                DurableAgentEvent::HookInvocationCompleted {
                    decision: direct_decision,
                    ..
                },
            ) => {
                assert_eq!(decision, direct_decision);
            }
            _ => assert_eq!(prepared_event, direct_event),
        }
    }
}

#[tokio::test]
async fn prepare_resume_does_no_io_and_run_appends_continuation_to_the_same_sink() {
    let prepared = noop_fixture(VecDeque::from([stop_turn("done")]));
    let prepared_value = prepared
        .agent_loop
        .prepare_resume("be precise", started_events())
        .expect("prepare should succeed");

    // Prepare is pure: no durable append and no model request.
    assert!(prepared.sink.events().is_empty());
    assert_eq!(prepared.provider.requests(), 0);

    let outcome = prepared_value
        .run(CancellationToken::new())
        .await
        .expect("prepared run should finish");
    assert_eq!(outcome.new_messages.len(), 1);

    // The continuation appends to the same sink and never records a second
    // `LoopStarted`.
    let events = prepared.sink.events();
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::LoopStarted { .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        DurableAgentEvent::MessageAppended { message } if message.role == ChatRole::Assistant
    )));
    assert!(matches!(
        events.last(),
        Some(DurableAgentEvent::LoopFinished { .. })
    ));
}

#[tokio::test]
async fn prepare_resume_fails_closed_without_side_effects_on_a_missing_loop_started() {
    let prepared = noop_fixture(VecDeque::from([stop_turn("unreachable")]));
    let error = prepared
        .agent_loop
        .prepare_resume(
            "be precise",
            vec![DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("hello"),
            }],
        )
        .err()
        .expect("a stream without loop_started must refuse closed");

    assert!(matches!(error, ResumeError::MissingLoopStarted));
    assert!(prepared.sink.events().is_empty());
    assert_eq!(prepared.provider.requests(), 0);
}

#[tokio::test]
async fn prepare_resume_fails_closed_without_side_effects_on_a_terminal_event() {
    let prepared = noop_fixture(VecDeque::from([stop_turn("unreachable")]));
    let mut events = started_events();
    events.push(DurableAgentEvent::LoopFinished {
        finish_reason: "stop".to_owned(),
        usage: Default::default(),
    });
    let error = prepared
        .agent_loop
        .prepare_resume("be precise", events)
        .err()
        .expect("a terminal stream must refuse closed");

    assert!(matches!(error, ResumeError::TerminalEvent));
    assert!(prepared.sink.events().is_empty());
    assert_eq!(prepared.provider.requests(), 0);
}

#[tokio::test]
async fn prepare_resume_fails_closed_without_side_effects_on_a_version_mismatch() {
    let recorded = ExtensionSetVersionId::new();
    let mut events = started_events();
    events[0] = DurableAgentEvent::LoopStarted {
        extension_set_version_id: Some(recorded),
    };
    let current = ExtensionSetVersionId::new();
    let prepared = fixture(
        VecDeque::from([stop_turn("unreachable")]),
        Arc::new(VersionedHookRuntime { version: current }),
    );
    let error = prepared
        .agent_loop
        .prepare_resume("be precise", events)
        .err()
        .expect("a version mismatch must refuse closed");

    assert!(matches!(
        error,
        ResumeError::ExtensionSetVersionMismatch { recorded: refused, current: reported }
        if refused == recorded && reported == current
    ));
    assert!(prepared.sink.events().is_empty());
    assert_eq!(prepared.provider.requests(), 0);
}

/// Compile-time guard: `PreparedResume` must never become `Clone` (the seam
/// contract is a single-use prepared value bound to one runtime). The method
/// call below only resolves when the blanket impl is the single candidate; a
/// `Clone` impl would make the resolution ambiguous and fail the build.
#[test]
fn prepared_resume_is_not_clone() {
    trait AmbiguousIfImpl<A> {
        fn method(&self) {}
    }
    impl<T: ?Sized> AmbiguousIfImpl<u8> for T {}
    // `Wrapper`'s field is never read: the type exists only as a method-
    // resolution probe for the compile-time `Clone` guard below.
    #[allow(dead_code)]
    struct Wrapper<T>(std::marker::PhantomData<T>);
    impl<T: Clone> AmbiguousIfImpl<u16> for Wrapper<T> {}

    Wrapper::<PreparedResume>(std::marker::PhantomData).method();
}
