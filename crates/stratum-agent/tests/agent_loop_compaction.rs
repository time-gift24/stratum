//! Durable transcript compaction (add-context-compaction tasks 4.1, 4.2, 4.4):
//! the cut-validation matrix, kernel execution ordering and rebased views,
//! and the resume replay of compaction events plus the crash windows.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures_util::stream;
use serde_json::json;
use stratum_agent::{
    AfterToolCallDecision, AfterToolCallInput, AgentLoop, AgentLoopError, COMPACTION_MARKER_PREFIX,
    DecideToolCallDecision, DecideToolCallInput, HookControl, HookRuntime, LoopContext, LoopLimits,
    PrepareNextTurnDecision, PrepareNextTurnInput, ToolExecutor, TransformContextDecision,
    TransformContextInput, TransformToolCallDecision, TransformToolCallInput,
};
use stratum_core::{
    AgentTelemetryEvent, CallId, ChatMessage, DangerLevel, DurableAgentEvent, HookDecisionRecord,
    HookFailure, HookPoint, ModelId, ToolCall, ToolCallDelta, ToolKind, ToolSpec,
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
    Hook(HookPoint),
}

/// Hook runtime with scripted `prepare_next_turn` decisions that records the
/// committed context of every `transform_context` snapshot.
struct CompactionRuntime {
    operations: Arc<Mutex<Vec<Operation>>>,
    prepares: Mutex<VecDeque<PrepareNextTurnDecision>>,
    transform_views: Mutex<Vec<Vec<ChatMessage>>>,
}

impl CompactionRuntime {
    fn new(
        operations: &Arc<Mutex<Vec<Operation>>>,
        prepares: Vec<PrepareNextTurnDecision>,
    ) -> Self {
        Self {
            operations: Arc::clone(operations),
            prepares: Mutex::new(prepares.into()),
            transform_views: Mutex::new(Vec::new()),
        }
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
impl HookRuntime for CompactionRuntime {
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
        Ok(DecideToolCallDecision::Execute)
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

struct EchoTool {
    spec: ToolSpec,
}

#[async_trait]
impl Tool for EchoTool {
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
        Ok(ToolOutput::new(json!({"echo": input.arguments})))
    }
}

fn echo_registry() -> Arc<dyn ToolRegistry> {
    let mut registry = BuiltinToolRegistry::new(ToolPermissionMode::Allow);
    registry
        .register(
            Arc::new(EchoTool {
                spec: ToolSpec::builder()
                    .name("echo")
                    .description("records calls")
                    .input_schema(json!({"type": "object"}))
                    .build(),
            }),
            ToolKind::Read,
            DangerLevel::Low,
        )
        .expect("echo tool should register");
    Arc::new(registry)
}

fn tool_call_turn(call_id: &str) -> Vec<ChatStreamEvent> {
    vec![
        ChatStreamEvent::ToolCallDelta(ToolCallDelta {
            index: 0,
            call_id: Some(CallId::from(call_id)),
            name: Some("echo".to_owned()),
            arguments_delta: "{}".to_owned(),
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
    hook_runtime: Arc<CompactionRuntime>,
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
        .tool_executor(ToolExecutor::new(echo_registry(), sink))
        .hook_runtime(hook_runtime)
        .telemetry(Arc::new(NullTelemetrySink))
        .limits(LoopLimits::new(8, 4))
        .build()
        .expect("all agent loop fields should be present")
}

fn recording_loop(
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
    hook_runtime: Arc<CompactionRuntime>,
    operations: &Arc<Mutex<Vec<Operation>>>,
) -> AgentLoop {
    build_loop(
        behaviors,
        hook_runtime,
        Arc::new(RecordingDurableSink {
            operations: Arc::clone(operations),
        }),
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

fn requests(operations: &[Operation]) -> Vec<ChatRequest> {
    operations
        .iter()
        .filter_map(|operation| match operation {
            Operation::ChatStream(request) => Some(request.clone()),
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

/// The kernel-owned marker message for one handler summary body.
fn marker(body: &str) -> ChatMessage {
    ChatMessage::system(format!("{COMPACTION_MARKER_PREFIX}\n{body}"))
}

fn assistant_with_call(call_id: &str) -> ChatMessage {
    ChatMessage::assistant("").with_tool_calls(vec![ToolCall {
        call_id: CallId::from(call_id),
        name: "echo".to_owned(),
        arguments: json!({}),
    }])
}

// 4.1: every invalid compact decision fails closed as InvalidOutput and
// commits neither the compaction nor the iteration boundary.
#[tokio::test]
async fn invalid_compactions_fail_closed_without_committing_anything() {
    let paired_history = vec![
        ChatMessage::user("old question"),
        assistant_with_call("call-historical"),
        ChatMessage::tool(CallId::from("call-historical"), json!({"ok": true})),
    ];
    // Committed at the prepare boundary: history + prompt + assistant + tool
    // result; the current iteration starts after history + prompt.
    let cases: Vec<(Vec<ChatMessage>, PrepareNextTurnDecision)> = vec![
        // A zero cut is a no-op compaction.
        (
            Vec::new(),
            PrepareNextTurnDecision::Compact {
                upto: 0,
                summary: ChatMessage::system("summary"),
            },
        ),
        // Out of bounds.
        (
            Vec::new(),
            PrepareNextTurnDecision::Compact {
                upto: 99,
                summary: ChatMessage::system("summary"),
            },
        ),
        // The cut drops the assistant message but not its tool result.
        (
            paired_history.clone(),
            PrepareNextTurnDecision::Compact {
                // history [user, assistant+ calls, tool] + prompt + current
                // cycle: upto 2 drops the assistant at index 1 while its
                // result at index 2 survives.
                upto: 2,
                summary: ChatMessage::system("summary"),
            },
        ),
        // The cut reaches into the current iteration's committed messages.
        (
            paired_history.clone(),
            PrepareNextTurnDecision::Compact {
                // history (3) + prompt (1) = iteration start 4; upto 5 cuts
                // the current assistant message.
                upto: 5,
                summary: ChatMessage::system("summary"),
            },
        ),
        // The summary must not forge another role.
        (
            Vec::new(),
            PrepareNextTurnDecision::Compact {
                upto: 1,
                summary: ChatMessage::assistant("forged"),
            },
        ),
        (
            Vec::new(),
            PrepareNextTurnDecision::Compact {
                upto: 1,
                summary: ChatMessage::user("forged"),
            },
        ),
        // The summary must not carry tool identity or reasoning.
        (
            Vec::new(),
            PrepareNextTurnDecision::Compact {
                upto: 1,
                summary: ChatMessage::system("summary").with_tool_calls(vec![ToolCall {
                    call_id: CallId::from("call-forged"),
                    name: "echo".to_owned(),
                    arguments: json!({}),
                }]),
            },
        ),
        (
            Vec::new(),
            PrepareNextTurnDecision::Compact {
                upto: 1,
                summary: ChatMessage::system("summary").with_reasoning_content("forged"),
            },
        ),
        (
            Vec::new(),
            PrepareNextTurnDecision::Compact {
                upto: 1,
                summary: ChatMessage::tool(CallId::from("call-forged"), json!({})),
            },
        ),
    ];
    for (history, decision) in cases {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let runtime = Arc::new(CompactionRuntime::new(&operations, vec![decision.clone()]));
        let agent_loop = recording_loop(
            VecDeque::from([tool_call_turn("call-1"), stop_turn("unreachable")]),
            runtime,
            &operations,
        );

        let error = agent_loop
            .run(
                LoopContext::new("be precise").with_messages(history),
                vec![ChatMessage::user("use echo")],
                CancellationToken::new(),
            )
            .await
            .expect_err("an invalid compact decision should fail closed");

        assert!(
            matches!(
                error,
                AgentLoopError::Hook {
                    point: HookPoint::PrepareNextTurn,
                    failure: HookFailure::InvalidOutput,
                }
            ),
            "decision {decision:?} should fail as InvalidOutput"
        );
        let events = durable_events(&snapshot(&operations));
        assert!(
            !events.iter().any(|event| matches!(
                event,
                DurableAgentEvent::TranscriptCompacted { .. }
                    | DurableAgentEvent::IterationCompleted { .. }
                    | DurableAgentEvent::LoopFinished { .. }
            )),
            "decision {decision:?} must not commit the compaction or the boundary"
        );
        assert!(
            !events.iter().any(|event| matches!(
                event,
                DurableAgentEvent::HookInvocationCompleted {
                    decision: HookDecisionRecord::PrepareNextTurn(_),
                    ..
                }
            )),
            "decision {decision:?} must not journal a completed prepare decision"
        );
        assert!(
            events.iter().any(|event| matches!(
                event,
                DurableAgentEvent::HookInvocationFailed {
                    failure: HookFailure::InvalidOutput,
                    ..
                }
            )),
            "decision {decision:?} should journal the typed failure"
        );
    }
}

// 4.2: the compaction event precedes the iteration boundary, the next model
// request and hook snapshots run from the compacted baseline, and the summary
// marker joins the loop outcome.
#[tokio::test]
async fn compaction_executes_before_the_boundary_and_rebases_the_view() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = Arc::new(CompactionRuntime::new(
        &operations,
        vec![PrepareNextTurnDecision::Compact {
            upto: 2,
            summary: ChatMessage::system("earlier exchange summary"),
        }],
    ));
    let agent_loop = recording_loop(
        VecDeque::from([tool_call_turn("call-1"), stop_turn("done")]),
        Arc::clone(&runtime),
        &operations,
    );
    let history = vec![
        ChatMessage::user("old question"),
        ChatMessage::assistant("old answer"),
    ];

    let outcome = agent_loop
        .run(
            LoopContext::new("be precise").with_messages(history),
            vec![ChatMessage::user("use echo")],
            CancellationToken::new(),
        )
        .await
        .expect("a legal compaction should run to completion");

    let marker = marker("earlier exchange summary");
    let recorded = snapshot(&operations);
    let events = durable_events(&recorded);

    // The compaction commits after the journaled decision and before the
    // iteration boundary.
    let compacted = events
        .iter()
        .position(|event| matches!(event, DurableAgentEvent::TranscriptCompacted { .. }))
        .expect("the compaction event should commit");
    let boundary = events
        .iter()
        .position(|event| matches!(event, DurableAgentEvent::IterationCompleted { .. }))
        .expect("the iteration boundary should commit");
    let prepare_completed = events
        .iter()
        .position(|event| {
            matches!(
                event,
                DurableAgentEvent::HookInvocationCompleted {
                    decision: HookDecisionRecord::PrepareNextTurn(_),
                    ..
                }
            )
        })
        .expect("the prepare decision should be journaled");
    assert!(
        prepare_completed < compacted && compacted < boundary,
        "ordering must be completed < transcript_compacted < iteration_completed"
    );
    assert_eq!(
        events[compacted],
        DurableAgentEvent::TranscriptCompacted {
            upto: 2,
            summary: marker.clone(),
            compacted_iteration: 0,
        }
    );

    // The next model request runs from the compacted baseline.
    let requests = requests(&recorded);
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1].messages,
        vec![
            ChatMessage::system("be precise"),
            marker.clone(),
            ChatMessage::user("use echo"),
            assistant_with_call("call-1"),
            ChatMessage::tool(CallId::from("call-1"), json!({"echo": {}})),
        ]
    );

    // The transform snapshot of the next iteration starts with the kernel
    // marker message, so a handler can recognize the compacted baseline.
    let views = runtime.transform_views();
    assert_eq!(views.len(), 2);
    assert_eq!(views[1].first(), Some(&marker));

    // The summary marker is a committed message of this run.
    assert!(outcome.new_messages.contains(&marker));
}

// 4.4: resume replays one compaction; the committed context resumes from the
// compacted baseline.
#[tokio::test]
async fn resume_replays_a_single_compaction() {
    let phase1_operations = Arc::new(Mutex::new(Vec::new()));
    let phase1_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1"), stop_turn("unreachable")]),
        Arc::new(CompactionRuntime::new(
            &phase1_operations,
            vec![PrepareNextTurnDecision::Compact {
                upto: 2,
                summary: ChatMessage::system("earlier questions summary"),
            }],
        )),
        Arc::new(CrashDurableSink {
            operations: Arc::clone(&phase1_operations),
            // Crash after the boundary, at the next iteration's first journal
            // record, so the stream stays resumable.
            crash_on: |event| {
                matches!(
                    event,
                    DurableAgentEvent::HookInvocationPending {
                        point: HookPoint::TransformContext,
                        iteration: 1,
                        ..
                    }
                )
            },
        }),
        &phase1_operations,
    );
    // Every compactable prefix message must be in the durable stream, so the
    // prompts carry the "history": preloaded context never enters the stream.
    phase1_loop
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
    let events_before = durable_events(&snapshot(&phase1_operations));

    let phase2_operations = Arc::new(Mutex::new(Vec::new()));
    let phase2_loop = recording_loop(
        VecDeque::from([stop_turn("done")]),
        Arc::new(CompactionRuntime::new(&phase2_operations, Vec::new())),
        &phase2_operations,
    );
    let outcome = phase2_loop
        .resume("be precise", events_before, CancellationToken::new())
        .await
        .expect("resume should finish from the compacted baseline");

    let phase2 = snapshot(&phase2_operations);
    let requests = requests(&phase2);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].messages,
        vec![
            ChatMessage::system("be precise"),
            marker("earlier questions summary"),
            ChatMessage::user("use echo"),
            assistant_with_call("call-1"),
            ChatMessage::tool(CallId::from("call-1"), json!({"echo": {}})),
        ],
        "the resumed run rebuilds the compacted committed context"
    );
    assert!(
        !outcome
            .new_messages
            .contains(&marker("earlier questions summary")),
        "the marker was committed before the crash, not by this run"
    );
}

// 4.4: resume replays multiple compactions in event order.
#[tokio::test]
async fn resume_replays_multiple_compactions_in_order() {
    let phase1_operations = Arc::new(Mutex::new(Vec::new()));
    let phase1_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1"),
            tool_call_turn("call-2"),
            stop_turn("unreachable"),
        ]),
        Arc::new(CompactionRuntime::new(
            &phase1_operations,
            vec![
                PrepareNextTurnDecision::Compact {
                    upto: 2,
                    summary: ChatMessage::system("summary one"),
                },
                PrepareNextTurnDecision::Compact {
                    upto: 3,
                    summary: ChatMessage::system("summary two"),
                },
            ],
        )),
        Arc::new(CrashDurableSink {
            operations: Arc::clone(&phase1_operations),
            crash_on: |event| {
                matches!(
                    event,
                    DurableAgentEvent::HookInvocationPending {
                        point: HookPoint::TransformContext,
                        iteration: 2,
                        ..
                    }
                )
            },
        }),
        &phase1_operations,
    );
    phase1_loop
        .run(
            LoopContext::new("be precise"),
            vec![
                ChatMessage::user("question one"),
                ChatMessage::user("question two"),
                ChatMessage::user("question three"),
                ChatMessage::user("question four"),
            ],
            CancellationToken::new(),
        )
        .await
        .expect_err("the simulated crash should stop phase 1");
    let events_before = durable_events(&snapshot(&phase1_operations));
    assert_eq!(
        events_before
            .iter()
            .filter(|event| matches!(event, DurableAgentEvent::TranscriptCompacted { .. }))
            .count(),
        2,
        "phase 1 committed two compactions"
    );

    let phase2_operations = Arc::new(Mutex::new(Vec::new()));
    let phase2_loop = recording_loop(
        VecDeque::from([stop_turn("done")]),
        Arc::new(CompactionRuntime::new(&phase2_operations, Vec::new())),
        &phase2_operations,
    );
    phase2_loop
        .resume("be precise", events_before, CancellationToken::new())
        .await
        .expect("resume should finish after both compactions");

    let phase2 = snapshot(&phase2_operations);
    let requests = requests(&phase2);
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].messages,
        vec![
            ChatMessage::system("be precise"),
            // The second compaction absorbed the first marker and the
            // remaining prompts; only its marker survives.
            marker("summary two"),
            assistant_with_call("call-1"),
            ChatMessage::tool(CallId::from("call-1"), json!({"echo": {}})),
            assistant_with_call("call-2"),
            ChatMessage::tool(CallId::from("call-2"), json!({"echo": {}})),
        ],
        "both compactions apply in event order"
    );
}

// 4.4: crash after the compact decision's journal record but before the
// compaction event; resume executes the compaction from the journaled summary
// without calling the handler again.
#[tokio::test]
async fn resume_closes_the_compaction_crash_window_from_the_journal() {
    let phase1_operations = Arc::new(Mutex::new(Vec::new()));
    let phase1_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1"), stop_turn("unreachable")]),
        Arc::new(CompactionRuntime::new(
            &phase1_operations,
            vec![PrepareNextTurnDecision::Compact {
                upto: 2,
                summary: ChatMessage::system("journaled summary"),
            }],
        )),
        Arc::new(CrashDurableSink {
            operations: Arc::clone(&phase1_operations),
            crash_on: |event| matches!(event, DurableAgentEvent::TranscriptCompacted { .. }),
        }),
        &phase1_operations,
    );
    phase1_loop
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
    let events_before = durable_events(&snapshot(&phase1_operations));
    assert!(
        events_before.iter().any(|event| matches!(
            event,
            DurableAgentEvent::HookInvocationCompleted {
                decision: HookDecisionRecord::PrepareNextTurn(_),
                ..
            }
        )),
        "the compact decision is journaled before the crash"
    );

    let phase2_operations = Arc::new(Mutex::new(Vec::new()));
    let phase2_runtime = Arc::new(CompactionRuntime::new(
        &phase2_operations,
        // Would compact differently if called; it must never run.
        vec![PrepareNextTurnDecision::Compact {
            upto: 1,
            summary: ChatMessage::system("regenerated summary"),
        }],
    ));
    let phase2_loop = recording_loop(
        VecDeque::from([stop_turn("done")]),
        phase2_runtime,
        &phase2_operations,
    );
    let outcome = phase2_loop
        .resume("be precise", events_before, CancellationToken::new())
        .await
        .expect("resume should close the crash window and finish");

    let phase2 = snapshot(&phase2_operations);
    assert!(
        !hook_points(&phase2).contains(&HookPoint::PrepareNextTurn),
        "the journaled compact decision is reused; the handler is not called again"
    );
    let compactions = durable_events(&phase2)
        .into_iter()
        .filter(|event| matches!(event, DurableAgentEvent::TranscriptCompacted { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        compactions,
        vec![DurableAgentEvent::TranscriptCompacted {
            upto: 2,
            summary: marker("journaled summary"),
            compacted_iteration: 0,
        }],
        "the compaction executes exactly once, from the journaled summary"
    );
    assert!(
        outcome.new_messages.contains(&marker("journaled summary")),
        "the marker is committed by the resumed run"
    );
    let requests = requests(&phase2);
    assert_eq!(
        requests[0].messages[1],
        marker("journaled summary"),
        "the next request runs from the compacted baseline"
    );
}

// 4.4: crash after the compaction event but before the iteration boundary;
// the journaled prepare decision (recorded before the compaction) still
// matches after replay applied the compaction, and resume commits only the
// missing boundary without compacting twice.
#[tokio::test]
async fn resume_after_a_committed_compaction_does_not_compact_twice() {
    let phase1_operations = Arc::new(Mutex::new(Vec::new()));
    let phase1_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1"), stop_turn("unreachable")]),
        Arc::new(CompactionRuntime::new(
            &phase1_operations,
            vec![PrepareNextTurnDecision::Compact {
                upto: 2,
                summary: ChatMessage::system("durable summary"),
            }],
        )),
        Arc::new(CrashDurableSink {
            operations: Arc::clone(&phase1_operations),
            crash_on: |event| matches!(event, DurableAgentEvent::IterationCompleted { .. }),
        }),
        &phase1_operations,
    );
    phase1_loop
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
    let events_before = durable_events(&snapshot(&phase1_operations));
    assert!(
        events_before
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::TranscriptCompacted { .. })),
        "the compaction is durable before the crash"
    );

    let phase2_operations = Arc::new(Mutex::new(Vec::new()));
    let phase2_loop = recording_loop(
        VecDeque::from([stop_turn("done")]),
        Arc::new(CompactionRuntime::new(&phase2_operations, Vec::new())),
        &phase2_operations,
    );
    let outcome = phase2_loop
        .resume("be precise", events_before, CancellationToken::new())
        .await
        .expect("resume should commit the missing boundary and finish");

    let phase2 = snapshot(&phase2_operations);
    assert!(
        !hook_points(&phase2).contains(&HookPoint::PrepareNextTurn),
        "the prepare decision journaled before the compaction still matches"
    );
    let phase2_events = durable_events(&phase2);
    assert!(
        !phase2_events
            .iter()
            .any(|event| matches!(event, DurableAgentEvent::TranscriptCompacted { .. })),
        "a replayed compaction is never executed or committed twice"
    );
    assert!(
        phase2_events.iter().any(|event| matches!(
            event,
            DurableAgentEvent::IterationCompleted { iteration: 0, .. }
        )),
        "the missing iteration boundary commits"
    );
    assert!(
        !outcome.new_messages.contains(&marker("durable summary")),
        "the marker was committed before the crash, not by this run"
    );
    let requests = requests(&phase2);
    assert_eq!(
        requests[0].messages[1],
        marker("durable summary"),
        "the resumed request runs from the compacted baseline"
    );
}

// V1: resume from a checkpoint window — `LoopStarted` plus the stream from
// the first retained message's line — rebuilds the context byte-identically
// to a full replay, and the window's journaled compact decision is reused
// without calling the handler again.
#[tokio::test]
async fn resume_from_a_checkpoint_window_matches_full_replay_and_reuses_the_journal() {
    // Phase 1 compacts at iteration 0 (dropping the four prompts), decides a
    // second compaction at iteration 1, and crashes before that second
    // compaction commits: the surviving stream ends at the journaled compact
    // decision.
    let phase1_operations = Arc::new(Mutex::new(Vec::new()));
    let phase1_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1"),
            tool_call_turn("call-2"),
            stop_turn("unreachable"),
        ]),
        Arc::new(CompactionRuntime::new(
            &phase1_operations,
            vec![
                PrepareNextTurnDecision::Compact {
                    upto: 4,
                    summary: ChatMessage::system("summary one"),
                },
                PrepareNextTurnDecision::Compact {
                    upto: 3,
                    summary: ChatMessage::system("summary two"),
                },
            ],
        )),
        Arc::new(CrashDurableSink {
            operations: Arc::clone(&phase1_operations),
            crash_on: |event| {
                matches!(
                    event,
                    DurableAgentEvent::TranscriptCompacted {
                        compacted_iteration: 1,
                        ..
                    }
                )
            },
        }),
        &phase1_operations,
    );
    phase1_loop
        .run(
            LoopContext::new("be precise"),
            vec![
                ChatMessage::user("question one"),
                ChatMessage::user("question two"),
                ChatMessage::user("question three"),
                ChatMessage::user("question four"),
            ],
            CancellationToken::new(),
        )
        .await
        .expect_err("the simulated crash should stop phase 1");
    let full_events = durable_events(&snapshot(&phase1_operations));

    // The checkpoint window: `LoopStarted` plus everything from the first
    // retained message's line (the first compaction dropped four messages).
    let window_start = full_events
        .iter()
        .enumerate()
        .filter(|(_, event)| matches!(event, DurableAgentEvent::MessageAppended { .. }))
        .nth(4)
        .map(|(index, _)| index)
        .expect("the stream has a fifth message line");
    let mut window_events = vec![full_events[0].clone()];
    window_events.extend_from_slice(&full_events[window_start..]);
    assert!(
        matches!(
            &window_events[1],
            DurableAgentEvent::MessageAppended { message } if !message.tool_calls.is_empty()
        ),
        "the window starts at the retained assistant message's line"
    );

    // Resume once from the full stream and once from the window.
    let mut reports = Vec::new();
    for events in [full_events, window_events] {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let agent_loop = recording_loop(
            VecDeque::from([stop_turn("done")]),
            Arc::new(CompactionRuntime::new(&operations, Vec::new())),
            &operations,
        );
        let outcome = agent_loop
            .resume("be precise", events, CancellationToken::new())
            .await
            .expect("resume should close the crash window and finish");
        reports.push((snapshot(&operations), outcome));
    }
    let (full_phase2, full_outcome) = &reports[0];
    let (window_phase2, window_outcome) = &reports[1];

    // The journaled compact decision inside the window is reused: the handler
    // is never called and the compaction executes exactly once, from the
    // journaled summary.
    assert!(
        !hook_points(window_phase2).contains(&HookPoint::PrepareNextTurn),
        "the window's journaled compact decision must be reused"
    );
    let compactions = durable_events(window_phase2)
        .into_iter()
        .filter(|event| matches!(event, DurableAgentEvent::TranscriptCompacted { .. }))
        .collect::<Vec<_>>();
    assert_eq!(
        compactions,
        vec![DurableAgentEvent::TranscriptCompacted {
            upto: 3,
            summary: marker("summary two"),
            compacted_iteration: 1,
        }],
        "the pending compaction executes exactly once from the journal"
    );

    // Byte identity: both resumes rebuild the same context, issue the same
    // model request, and return the same outcome.
    assert_eq!(requests(window_phase2), requests(full_phase2));
    assert_eq!(window_outcome, full_outcome);
    assert_eq!(
        requests(window_phase2)[0].messages,
        vec![
            ChatMessage::system("be precise"),
            marker("summary two"),
            assistant_with_call("call-2"),
            ChatMessage::tool(CallId::from("call-2"), json!({"echo": {}})),
        ]
    );
}
