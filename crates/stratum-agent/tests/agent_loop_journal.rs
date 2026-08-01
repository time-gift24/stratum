//! Journal write ordering (task 5.1) and context-patch contract (task 5.3).

use std::{
    collections::VecDeque,
    future::pending,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use futures_util::stream;
use serde_json::{Value, json};
use stratum_agent::{
    AfterToolCallDecision, AfterToolCallInput, AgentLoop, AgentLoopError, ContextPatch,
    DecideToolCallDecision, DecideToolCallInput, HookControl, HookRuntime, LoopCompletionReason,
    LoopContext, LoopLimits, PrepareNextTurnDecision, PrepareNextTurnInput, ToolExecutor,
    TransformContextDecision, TransformContextInput, TransformToolCallDecision,
    TransformToolCallInput,
};
use stratum_core::{
    AgentTelemetryEvent, CallId, ChatMessage, ChatRole, DangerLevel, DurableAgentEvent,
    HookDecisionRecord, HookFailure, HookPoint, ModelId, ToolCall, ToolCallDelta, ToolKind,
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
    Hook(HookPoint),
}

struct RecordingHookRuntime {
    operations: Arc<Mutex<Vec<Operation>>>,
    decide_failure: Option<HookFailure>,
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
        match self.decide_failure {
            Some(failure) => Err(failure),
            None => Ok(DecideToolCallDecision::Execute),
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

impl RecordingHookRuntime {
    fn new(operations: &Arc<Mutex<Vec<Operation>>>) -> Self {
        Self {
            operations: Arc::clone(operations),
            decide_failure: None,
        }
    }

    fn record(&self, point: HookPoint) {
        self.operations
            .lock()
            .expect("operation lock should not be poisoned")
            .push(Operation::Hook(point));
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

fn build_loop(
    behaviors: VecDeque<Vec<ChatStreamEvent>>,
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
            echo_registry(operations),
            Arc::new(RecordingDurableSink {
                operations: Arc::clone(operations),
            }),
        ))
        .telemetry(Arc::new(NullTelemetrySink))
        .limits(LoopLimits::new(8, 4));
    let builder = match hook_runtime {
        Some(runtime) => builder.hook_runtime(runtime),
        None => builder,
    };
    builder
        .build()
        .expect("all agent loop fields should be present")
}

fn snapshot(operations: &Arc<Mutex<Vec<Operation>>>) -> Vec<Operation> {
    operations
        .lock()
        .expect("operation lock should not be poisoned")
        .clone()
}

fn run_once(
    agent_loop: &AgentLoop,
    context: LoopContext,
) -> impl Future<Output = Result<stratum_agent::LoopOutcome, AgentLoopError>> + '_ {
    agent_loop.run(
        context,
        vec![ChatMessage::user("use echo")],
        CancellationToken::new(),
    )
}

/// Position of the first operation matching `predicate`.
fn position_of(
    operations: &[Operation],
    predicate: impl Fn(&Operation) -> bool,
    label: &str,
) -> usize {
    operations
        .iter()
        .position(predicate)
        .unwrap_or_else(|| panic!("missing operation {label}"))
}

/// Position of the pending or completed journal event of one hook point.
fn journal_position(operations: &[Operation], point: HookPoint, completed: bool) -> usize {
    position_of(
        operations,
        |operation| match operation {
            Operation::Durable(DurableAgentEvent::HookInvocationPending {
                point: pending_point,
                ..
            }) if !completed => *pending_point == point,
            Operation::Durable(DurableAgentEvent::HookInvocationCompleted { decision, .. })
                if completed =>
            {
                decision_point(decision) == point
            }
            _ => false,
        },
        if completed {
            "hook completed"
        } else {
            "hook pending"
        },
    )
}

fn decision_point(decision: &HookDecisionRecord) -> HookPoint {
    match decision {
        HookDecisionRecord::TransformContext(_) => HookPoint::TransformContext,
        HookDecisionRecord::TransformToolCall(_) => HookPoint::TransformToolCall,
        HookDecisionRecord::DecideToolCall(_) => HookPoint::DecideToolCall,
        HookDecisionRecord::AfterToolCall(_) => HookPoint::AfterToolCall,
        HookDecisionRecord::PrepareNextTurn(_) => HookPoint::PrepareNextTurn,
        _ => unreachable!("test covers exactly five hook points"),
    }
}

// 5.1: every hook point journals Pending before calling the runtime and
// Completed after the decision validates but before its affected action.
#[tokio::test]
async fn journal_orders_pending_and_completed_around_runtime_and_actions() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let runtime = RecordingHookRuntime::new(&operations);
    let agent_loop = build_loop(
        VecDeque::from([
            tool_call_turn("call-1", "echo", json!({"value": "one"})),
            stop_turn("done"),
        ]),
        Some(Arc::new(runtime)),
        &operations,
    );

    run_once(&agent_loop, LoopContext::new("be precise"))
        .await
        .expect("loop should finish");

    let recorded = snapshot(&operations);
    let points = [
        HookPoint::TransformContext,
        HookPoint::TransformToolCall,
        HookPoint::DecideToolCall,
        HookPoint::AfterToolCall,
        HookPoint::PrepareNextTurn,
    ];
    for point in points {
        let pending = journal_position(&recorded, point, false);
        let called = position_of(
            &recorded,
            |operation| matches!(operation, Operation::Hook(hook) if *hook == point),
            "runtime call",
        );
        let completed = journal_position(&recorded, point, true);
        assert!(
            pending < called && called < completed,
            "{point:?} must journal pending < runtime call < completed"
        );
    }

    // Completed always precedes the affected action.
    let first_request = position_of(
        &recorded,
        |operation| matches!(operation, Operation::ChatStream(_)),
        "model request",
    );
    assert!(
        journal_position(&recorded, HookPoint::TransformContext, true) < first_request,
        "transform completed must precede the model request"
    );
    assert!(
        journal_position(&recorded, HookPoint::TransformToolCall, true)
            < journal_position(&recorded, HookPoint::DecideToolCall, false),
        "tool transform completed must precede the decide invocation"
    );
    let started = position_of(
        &recorded,
        |operation| {
            matches!(
                operation,
                Operation::Durable(DurableAgentEvent::ToolExecutionStarted { .. })
            )
        },
        "tool execution started",
    );
    assert!(
        journal_position(&recorded, HookPoint::DecideToolCall, true) < started,
        "decide completed must precede tool_execution_started"
    );
    let result_commit = position_of(
        &recorded,
        |operation| {
            matches!(
                operation,
                Operation::Durable(DurableAgentEvent::MessageAppended { message })
                    if message.role == ChatRole::Tool
            )
        },
        "tool result commit",
    );
    assert!(
        journal_position(&recorded, HookPoint::AfterToolCall, true) < result_commit,
        "after completed must precede the result commit"
    );
    let boundary = position_of(
        &recorded,
        |operation| {
            matches!(
                operation,
                Operation::Durable(DurableAgentEvent::IterationCompleted { .. })
            )
        },
        "iteration boundary",
    );
    assert!(
        journal_position(&recorded, HookPoint::PrepareNextTurn, true) < boundary,
        "prepare completed must precede the iteration boundary"
    );
}

// 5.1: a typed hook failure journals Pending, then Failed before the terminal
// loop failure, and never journals Completed.
#[tokio::test]
async fn journal_records_failed_before_the_terminal_failure() {
    let operations = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = RecordingHookRuntime::new(&operations);
    runtime.decide_failure = Some(HookFailure::HandlerFailed);
    let agent_loop = build_loop(
        VecDeque::from([tool_call_turn("call-1", "echo", json!({}))]),
        Some(Arc::new(runtime)),
        &operations,
    );

    let error = run_once(&agent_loop, LoopContext::new("be precise"))
        .await
        .expect_err("a failing decide hook should stop the loop");

    assert!(matches!(
        error,
        AgentLoopError::Hook {
            point: HookPoint::DecideToolCall,
            failure: HookFailure::HandlerFailed,
        }
    ));
    let recorded = snapshot(&operations);
    let pending = journal_position(&recorded, HookPoint::DecideToolCall, false);
    let called = position_of(
        &recorded,
        |operation| matches!(operation, Operation::Hook(HookPoint::DecideToolCall)),
        "runtime call",
    );
    let failed = position_of(
        &recorded,
        |operation| {
            matches!(
                operation,
                Operation::Durable(DurableAgentEvent::HookInvocationFailed {
                    failure: HookFailure::HandlerFailed,
                    ..
                })
            )
        },
        "hook failed",
    );
    let terminal = position_of(
        &recorded,
        |operation| {
            matches!(
                operation,
                Operation::Durable(DurableAgentEvent::LoopFailed { .. })
            )
        },
        "loop failed",
    );
    assert!(pending < called && called < failed && failed < terminal);
    assert!(!recorded.iter().any(|operation| matches!(
        operation,
        Operation::Durable(DurableAgentEvent::HookInvocationCompleted { decision, .. })
            if matches!(decision, HookDecisionRecord::DecideToolCall(_))
    )));
}

// 5.3: each patch variant adjusts the current request view only.
#[tokio::test]
async fn patches_adjust_the_request_view_without_touching_committed_state() {
    struct PatchRuntime {
        patch: ContextPatch,
    }

    #[async_trait]
    impl HookRuntime for PatchRuntime {
        async fn transform_context<'a>(
            &self,
            input: TransformContextInput<'a>,
            _control: HookControl,
        ) -> Result<TransformContextDecision, HookFailure> {
            // Patch the first iteration only: later iterations must rebuild
            // from the unpatched committed context.
            if input.snapshot.iteration == 0 {
                Ok(TransformContextDecision::Patch(self.patch.clone()))
            } else {
                Ok(TransformContextDecision::Unchanged)
            }
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

    let history = vec![
        ChatMessage::user("old question"),
        ChatMessage::assistant("old answer"),
    ];
    let cases: Vec<(ContextPatch, Vec<ChatMessage>)> = vec![
        (
            ContextPatch::ReplaceSystemPrompt("fresh system".to_owned()),
            vec![
                ChatMessage::system("fresh system"),
                ChatMessage::user("old question"),
                ChatMessage::assistant("old answer"),
                ChatMessage::user("use echo"),
            ],
        ),
        (
            ContextPatch::DropHistory { upto: 2 },
            vec![
                ChatMessage::system("be precise"),
                ChatMessage::user("use echo"),
            ],
        ),
        (
            ContextPatch::RewriteHistory {
                upto: 2,
                summary: ChatMessage::assistant("summary so far"),
            },
            vec![
                ChatMessage::system("be precise"),
                ChatMessage::assistant("summary so far"),
                ChatMessage::user("use echo"),
            ],
        ),
    ];
    for (patch, expected_first_request) in cases {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let agent_loop = build_loop(
            VecDeque::from([
                tool_call_turn("call-1", "echo", json!({})),
                stop_turn("done"),
            ]),
            Some(Arc::new(PatchRuntime {
                patch: patch.clone(),
            })),
            &operations,
        );

        let outcome = run_once(
            &agent_loop,
            LoopContext::new("be precise").with_messages(history.clone()),
        )
        .await
        .expect("patched loop should finish");

        let recorded = snapshot(&operations);
        let requests = recorded
            .iter()
            .filter_map(|operation| match operation {
                Operation::ChatStream(request) => Some(request.clone()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            requests[0].messages, expected_first_request,
            "patch {patch:?} should shape the first request"
        );
        assert_eq!(
            requests[1].messages[..4],
            [
                ChatMessage::system("be precise"),
                ChatMessage::user("old question"),
                ChatMessage::assistant("old answer"),
                ChatMessage::user("use echo"),
            ],
            "the next iteration must rebuild from the unpatched committed context"
        );
        // The patch never becomes a durable message or a loop outcome message.
        assert!(!outcome.new_messages.iter().any(|message| {
            message.role == ChatRole::System || message == &ChatMessage::assistant("summary so far")
        }));
        assert!(!recorded.iter().any(|operation| matches!(
            operation,
            Operation::Durable(DurableAgentEvent::MessageAppended { message })
                if message.role == ChatRole::System
                    || message == &ChatMessage::assistant("summary so far")
        )));
    }
}

// 5.3: invalid patches fail closed as InvalidOutput before the model request,
// journaling the failure first.
#[tokio::test]
async fn invalid_patches_fail_closed_before_the_model_request() {
    struct InvalidPatchRuntime {
        patch: ContextPatch,
    }

    #[async_trait]
    impl HookRuntime for InvalidPatchRuntime {
        async fn transform_context<'a>(
            &self,
            _input: TransformContextInput<'a>,
            _control: HookControl,
        ) -> Result<TransformContextDecision, HookFailure> {
            Ok(TransformContextDecision::Patch(self.patch.clone()))
        }

        async fn transform_tool_call<'a>(
            &self,
            _input: TransformToolCallInput<'a>,
            _control: HookControl,
        ) -> Result<TransformToolCallDecision, HookFailure> {
            pending().await
        }

        async fn decide_tool_call<'a>(
            &self,
            _input: DecideToolCallInput<'a>,
            _control: HookControl,
        ) -> Result<DecideToolCallDecision, HookFailure> {
            pending().await
        }

        async fn after_tool_call<'a>(
            &self,
            _input: AfterToolCallInput<'a>,
            _control: HookControl,
        ) -> Result<AfterToolCallDecision, HookFailure> {
            pending().await
        }

        async fn prepare_next_turn<'a>(
            &self,
            _input: PrepareNextTurnInput<'a>,
            _control: HookControl,
        ) -> Result<PrepareNextTurnDecision, HookFailure> {
            pending().await
        }
    }

    let paired_history = vec![
        ChatMessage::assistant("").with_tool_calls(vec![ToolCall {
            call_id: CallId::from("call-historical"),
            name: "echo".to_owned(),
            arguments: json!({}),
        }]),
        ChatMessage::tool(CallId::from("call-historical"), json!({"ok": true})),
    ];
    let cases: Vec<(Vec<ChatMessage>, ContextPatch)> = vec![
        // Out of bounds.
        (Vec::new(), ContextPatch::DropHistory { upto: 3 }),
        // Cuts a tool_call/tool_result pair: the assistant message is dropped
        // but its result would survive.
        (
            paired_history.clone(),
            ContextPatch::DropHistory { upto: 1 },
        ),
        (
            paired_history.clone(),
            ContextPatch::RewriteHistory {
                upto: 1,
                summary: ChatMessage::assistant("summary"),
            },
        ),
        // A summary must not open a tool-call pair itself.
        (
            Vec::new(),
            ContextPatch::RewriteHistory {
                upto: 0,
                summary: ChatMessage::assistant("").with_tool_calls(vec![ToolCall {
                    call_id: CallId::from("call-forged"),
                    name: "echo".to_owned(),
                    arguments: json!({}),
                }]),
            },
        ),
        // A nested composition: each drop is individually in bounds for the
        // committed history, but the trailing drop would overrun the view the
        // inner composition produces. Validation must reject the nesting
        // closed instead of letting a stale view through to a panicking drain.
        (
            vec![
                ChatMessage::user("one"),
                ChatMessage::user("two"),
                ChatMessage::user("three"),
            ],
            ContextPatch::Composite(vec![
                ContextPatch::Composite(vec![ContextPatch::DropHistory { upto: 2 }]),
                ContextPatch::DropHistory { upto: 2 },
            ]),
        ),
    ];
    for (history, patch) in cases {
        let operations = Arc::new(Mutex::new(Vec::new()));
        let agent_loop = build_loop(
            VecDeque::from([stop_turn("unused")]),
            Some(Arc::new(InvalidPatchRuntime {
                patch: patch.clone(),
            })),
            &operations,
        );

        let error = run_once(
            &agent_loop,
            LoopContext::new("be precise").with_messages(history),
        )
        .await
        .expect_err("an invalid patch should fail closed");

        assert!(
            matches!(
                error,
                AgentLoopError::Hook {
                    point: HookPoint::TransformContext,
                    failure: HookFailure::InvalidOutput,
                }
            ),
            "patch {patch:?} should fail as InvalidOutput"
        );
        let recorded = snapshot(&operations);
        assert!(
            !recorded
                .iter()
                .any(|operation| matches!(operation, Operation::ChatStream(_))),
            "no model request may start after an invalid patch"
        );
        let failed = position_of(
            &recorded,
            |operation| {
                matches!(
                    operation,
                    Operation::Durable(DurableAgentEvent::HookInvocationFailed {
                        failure: HookFailure::InvalidOutput,
                        ..
                    })
                )
            },
            "hook failed",
        );
        let terminal = position_of(
            &recorded,
            |operation| {
                matches!(
                    operation,
                    Operation::Durable(DurableAgentEvent::LoopFailed { .. })
                )
            },
            "loop failed",
        );
        assert!(
            failed < terminal,
            "the journal failure precedes loop_failed"
        );
    }
}

// 5.3: a drop boundary that keeps the assistant and its results on the same
// side is accepted.
#[tokio::test]
async fn drop_history_allows_boundaries_that_keep_tool_pairs_together() {
    struct DropRuntime;

    #[async_trait]
    impl HookRuntime for DropRuntime {
        async fn transform_context<'a>(
            &self,
            _input: TransformContextInput<'a>,
            _control: HookControl,
        ) -> Result<TransformContextDecision, HookFailure> {
            Ok(TransformContextDecision::Patch(ContextPatch::DropHistory {
                upto: 2,
            }))
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

    let operations = Arc::new(Mutex::new(Vec::new()));
    let agent_loop = build_loop(
        VecDeque::from([stop_turn("done")]),
        Some(Arc::new(DropRuntime)),
        &operations,
    );
    // The assistant/result pair sits below the cut together.
    let history = vec![
        ChatMessage::assistant("").with_tool_calls(vec![ToolCall {
            call_id: CallId::from("call-historical"),
            name: "echo".to_owned(),
            arguments: json!({}),
        }]),
        ChatMessage::tool(CallId::from("call-historical"), json!({"ok": true})),
    ];

    let outcome = agent_loop
        .run(
            LoopContext::new("be precise").with_messages(history),
            vec![ChatMessage::user("fresh question")],
            CancellationToken::new(),
        )
        .await
        .expect("a pair-preserving cut should be accepted");

    assert_eq!(
        outcome.completion,
        LoopCompletionReason::Model(FinishReason::Stop)
    );
    let recorded = snapshot(&operations);
    let request = recorded
        .iter()
        .find_map(|operation| match operation {
            Operation::ChatStream(request) => Some(request.clone()),
            _ => None,
        })
        .expect("the model request should run");
    assert_eq!(
        request.messages,
        vec![
            ChatMessage::system("be precise"),
            ChatMessage::user("fresh question"),
        ]
    );
}
