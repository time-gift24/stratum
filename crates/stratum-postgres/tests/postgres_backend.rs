//! Container-backed integration tests for the Postgres execution-storage
//! backend. All tests are `#[ignore]` by default; run them against the crate's
//! compose stack via `make test-integration` (or manually with
//! `cargo test -p stratum-postgres --test postgres_backend -- --ignored`).
//!
//! The database URL defaults to the compose stack in
//! `docker-compose.test.yml` and can be overridden with
//! `STRATUM_POSTGRES_TEST_URL`.

use std::collections::BTreeSet;

use serde_json::{Map, json};
use stratum_core::{
    AgentId, AgentRuntimeContext, ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel,
    DecideToolCallDecisionRecord, DurableAgentEvent, HookDecisionRecord, HookFailure,
    HookInvocationId, HookPoint, ModelConfig, ModelId, NewAgentMessage, SessionId, TokenUsage,
    ToolKind, ToolName, ToolSetFingerprint, TurnId, TurnRuntimeSnapshot,
};
use stratum_infra::{DurableEventSink, DurableEventSinkError};
use stratum_postgres::{PostgresBackend, PostgresEventSinkError, read_events};
use stratum_store::{AgentState, AgentStatus, AgentStore, StoreError};

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

fn model_config() -> ModelConfig {
    ModelConfig::new(
        ModelId::new("openai", "test-model").expect("static model is valid"),
        Map::new(),
    )
}

fn tool_set_fingerprint() -> ToolSetFingerprint {
    "a".repeat(64).parse().expect("valid fingerprint")
}

fn snapshot_for(state: &AgentState) -> TurnRuntimeSnapshot {
    TurnRuntimeSnapshot::new(
        state.agent_version_id,
        model_config(),
        tool_set_fingerprint(),
        state.skill_set_version_id,
        state.extension_set_version_id,
        state.hook_handler_versions.clone(),
    )
}

fn usage() -> TokenUsage {
    TokenUsage {
        input_tokens: 1,
        output_tokens: 2,
        total_tokens: 3,
    }
}

fn sample_events() -> Vec<DurableAgentEvent> {
    vec![
        DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        },
        DurableAgentEvent::MessageAppended {
            message: ChatMessage::user("hello 中 🚀"),
        },
        DurableAgentEvent::ToolApprovalRequested {
            approval_id: ApprovalId::new(),
            call_id: CallId::from("tool-call-1"),
            tool_name: ToolName::from("echo"),
            arguments: json!({ "text": "hello", "nested": { "order": [3, 1, 2] } }),
            tool_kind: ToolKind::Read,
            danger_level: DangerLevel::Low,
        },
        DurableAgentEvent::ToolApprovalResolved {
            approval_id: ApprovalId::new(),
            decision: ApprovalDecision::Approve,
        },
        DurableAgentEvent::ToolExecutionStarted {
            call_id: CallId::from("tool-call-1"),
            tool_name: ToolName::from("echo"),
        },
        DurableAgentEvent::HookInvocationPending {
            invocation_id: HookInvocationId::new(),
            point: HookPoint::DecideToolCall,
            iteration: 0,
            call_id: Some(CallId::from("tool-call-1")),
            input_digest: "b".repeat(64).parse().expect("valid digest"),
        },
        DurableAgentEvent::HookInvocationCompleted {
            invocation_id: HookInvocationId::new(),
            decision: HookDecisionRecord::DecideToolCall(DecideToolCallDecisionRecord::Execute),
        },
        DurableAgentEvent::HookInvocationFailed {
            invocation_id: HookInvocationId::new(),
            failure: HookFailure::TimedOut,
        },
        DurableAgentEvent::TranscriptCompacted {
            upto: 2,
            summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary so far"),
            compacted_iteration: 1,
        },
        DurableAgentEvent::IterationCompleted {
            iteration: 1,
            usage: usage(),
        },
        DurableAgentEvent::LoopFinished {
            finish_reason: "stop".to_owned(),
            usage: usage(),
        },
        DurableAgentEvent::LoopFailed {
            error_text: "provider unavailable".to_owned(),
            usage: usage(),
        },
        DurableAgentEvent::LoopCancelled { usage: usage() },
    ]
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn migration_creates_execution_tables() {
    let backend = backend().await;

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables WHERE table_schema = 'public'",
    )
    .fetch_all(backend.pool())
    .await
    .expect("tables query succeeds");
    for table in ["durable_events", "agent_messages", "agent_state"] {
        assert!(tables.iter().any(|name| name == table), "missing {table}");
    }

    let unique_constraints: Vec<String> = sqlx::query_scalar(
        "SELECT constraint_name FROM information_schema.table_constraints \
         WHERE table_schema = 'public' AND table_name = 'durable_events' \
           AND constraint_type = 'UNIQUE'",
    )
    .fetch_all(backend.pool())
    .await
    .expect("constraints query succeeds");
    assert!(
        unique_constraints
            .iter()
            .any(|name| name == "durable_events_turn_id_seq_unique")
    );

    let indexes: Vec<String> =
        sqlx::query_scalar("SELECT indexname FROM pg_indexes WHERE schemaname = 'public'")
            .fetch_all(backend.pool())
            .await
            .expect("indexes query succeeds");
    assert!(
        indexes
            .iter()
            .any(|name| name == "durable_events_session_id_id_idx")
    );
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn sink_appended_events_read_back_equal_and_in_order() {
    let backend = backend().await;
    let (session_id, agent_id, turn_id) = (SessionId::new(), AgentId::new(), TurnId::new());
    let sink = backend
        .event_sink(session_id, agent_id, turn_id)
        .await
        .expect("sink opens");
    let events = sample_events();

    for event in &events {
        sink.append(event.clone()).await.expect("append succeeds");
    }

    assert_eq!(
        read_events(backend.pool(), turn_id)
            .await
            .expect("events read back"),
        events
    );

    // The payload column carries the complete canonical event JSON: equal to
    // the serde value of the event (jsonb normalizes key order/whitespace, so
    // equality is field-by-field after deserialization).
    let payload: serde_json::Value =
        sqlx::query_scalar("SELECT payload FROM durable_events WHERE turn_id = $1 AND seq = 2")
            .bind(turn_id.as_uuid())
            .fetch_one(backend.pool())
            .await
            .expect("payload row exists");
    assert_eq!(
        payload,
        serde_json::to_value(&events[1]).expect("event serializes")
    );

    // Sequences are 1-based, mirroring events.jsonl line numbers.
    let first_seq: i64 =
        sqlx::query_scalar("SELECT min(seq) FROM durable_events WHERE turn_id = $1")
            .bind(turn_id.as_uuid())
            .fetch_one(backend.pool())
            .await
            .expect("sequence query succeeds");
    assert_eq!(first_seq, 1);
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn reopened_sink_continues_sequence_for_resumed_run() {
    let backend = backend().await;
    let (session_id, agent_id, turn_id) = (SessionId::new(), AgentId::new(), TurnId::new());
    let events = sample_events();

    let sink = backend
        .event_sink(session_id, agent_id, turn_id)
        .await
        .expect("sink opens");
    for event in &events[..2] {
        sink.append(event.clone()).await.expect("append succeeds");
    }
    drop(sink);

    // Resume re-opens the same run and must continue numbering after the
    // persisted frontier instead of restarting at 1.
    let resumed = backend
        .event_sink(session_id, agent_id, turn_id)
        .await
        .expect("sink reopens");
    resumed
        .append(events[2].clone())
        .await
        .expect("append after resume succeeds");

    assert_eq!(
        read_events(backend.pool(), turn_id)
            .await
            .expect("events read back"),
        events[..3]
    );
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn duplicate_sequence_is_rejected_with_typed_error() {
    let backend = backend().await;
    let (session_id, agent_id, turn_id) = (SessionId::new(), AgentId::new(), TurnId::new());
    // Two sinks opened before any append both plan to write seq 1; the unique
    // constraint is the fail-closed backstop for the stale one.
    let sink_a = backend
        .event_sink(session_id, agent_id, turn_id)
        .await
        .expect("sink a opens");
    let sink_b = backend
        .event_sink(session_id, agent_id, turn_id)
        .await
        .expect("sink b opens");

    sink_a
        .append(DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        })
        .await
        .expect("first append succeeds");
    let error = sink_b
        .append(DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        })
        .await
        .expect_err("stale sink must be rejected");

    let DurableEventSinkError::Backend(source) = error else {
        panic!("unexpected error: {error:?}");
    };
    assert!(
        matches!(
            source.downcast_ref::<PostgresEventSinkError>(),
            Some(PostgresEventSinkError::DuplicateSequence { seq: 1, .. })
        ),
        "unexpected source: {source:?}"
    );
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn read_events_of_unknown_turn_is_empty() {
    let backend = backend().await;

    assert_eq!(
        read_events(backend.pool(), TurnId::new())
            .await
            .expect("read succeeds"),
        Vec::new()
    );
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn store_initialize_and_load_round_trip() {
    let backend = backend().await;
    let agent_id = AgentId::new();
    let store = backend.agent_store(agent_id);

    let error = store
        .load_agent()
        .await
        .expect_err("load before initialize must fail");
    assert!(matches!(error, StoreError::AgentMissing));

    let error = store
        .initialize(AgentId::new(), "writer".to_owned())
        .await
        .expect_err("mismatched agent id must fail");
    assert!(matches!(error, StoreError::AgentMismatch { .. }));

    let state = store
        .initialize_with_model_config(agent_id, "writer".to_owned(), model_config())
        .await
        .expect("initialize succeeds");
    assert_eq!(state.status, AgentStatus::Idle);
    assert_eq!(state.last_seq, 0);
    assert_eq!(state.next_iteration, 0);

    let loaded = store.load_agent().await.expect("load succeeds");
    assert_eq!(loaded.agent_id, agent_id);
    assert_eq!(loaded.name, "writer");
    assert_eq!(loaded.status, AgentStatus::Idle);
    assert_eq!(loaded.agent_version_id, state.agent_version_id);
    assert_eq!(loaded.hook_handler_versions, state.hook_handler_versions);
    assert_eq!(loaded.model_config, state.model_config);

    let error = store
        .initialize_with_model_config(agent_id, "writer".to_owned(), model_config())
        .await
        .expect_err("duplicate initialize must fail");
    assert!(matches!(error, StoreError::Backend(_)));
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn store_unconfigured_agent_fails_state_validation() {
    let backend = backend().await;
    let agent_id = AgentId::new();
    let store = backend.agent_store(agent_id);
    store
        .initialize(agent_id, "writer".to_owned())
        .await
        .expect("initialize succeeds");

    // Same contract as the filesystem backend: state without a model
    // configuration cannot be loaded.
    let error = store
        .load_agent()
        .await
        .expect_err("unconfigured state must fail validation");
    assert!(matches!(error, StoreError::MissingModelConfig));
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn store_start_turn_pins_snapshot_and_enforces_preconditions() {
    let backend = backend().await;
    let agent_id = AgentId::new();
    let store = backend.agent_store(agent_id);
    let state = store
        .initialize_with_model_config(agent_id, "writer".to_owned(), model_config())
        .await
        .expect("initialize succeeds");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let context = AgentRuntimeContext::direct(session_id);

    let started = store
        .start_turn(&context, turn_id, snapshot_for(&state))
        .await
        .expect("start_turn succeeds");
    assert_eq!(started.status, AgentStatus::Running);
    assert_eq!(started.session_id, Some(session_id));
    assert_eq!(started.turn_id, Some(turn_id));
    assert_eq!(started.next_iteration, 0);
    assert_eq!(started.usage, TokenUsage::default());
    assert_eq!(started.turn_runtime_snapshot, Some(snapshot_for(&state)));

    // Retrying the same start is idempotent.
    store
        .start_turn(&context, turn_id, snapshot_for(&state))
        .await
        .expect("same start_turn retry succeeds");

    // A different turn while running conflicts.
    let error = store
        .start_turn(&context, TurnId::new(), snapshot_for(&state))
        .await
        .expect_err("conflicting start_turn must fail");
    assert!(matches!(
        error,
        StoreError::RunningSessionConflict {
            current: Some(current),
            attempted: Some(attempted),
        } if current == session_id && attempted == session_id
    ));

    // A snapshot pinning different definition versions fails closed.
    let other_agent_id = AgentId::new();
    let other = backend.agent_store(other_agent_id);
    let other_state = other
        .initialize_with_model_config(other_agent_id, "writer".to_owned(), model_config())
        .await
        .expect("initialize succeeds");
    let mismatched = TurnRuntimeSnapshot::new(
        stratum_core::AgentVersionId::new(),
        model_config(),
        tool_set_fingerprint(),
        other_state.skill_set_version_id,
        other_state.extension_set_version_id,
        other_state.hook_handler_versions.clone(),
    );
    let error = other
        .start_turn(&context, TurnId::new(), mismatched)
        .await
        .expect_err("snapshot mismatch must fail");
    assert!(matches!(
        error,
        StoreError::RuntimeSnapshotMismatch {
            component: "agent_version"
        }
    ));
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn store_update_state_detects_running_conflict() {
    let backend = backend().await;
    let agent_id = AgentId::new();
    let store = backend.agent_store(agent_id);
    let state = store
        .initialize_with_model_config(agent_id, "writer".to_owned(), model_config())
        .await
        .expect("initialize succeeds");
    let session_a = SessionId::new();
    let turn_a = TurnId::new();
    store
        .start_turn(
            &AgentRuntimeContext::direct(session_a),
            turn_a,
            snapshot_for(&state),
        )
        .await
        .expect("start_turn succeeds");

    // Re-affirming the same running session operation succeeds.
    store
        .update_state(AgentStatus::Running, Some(session_a), Some(turn_a), usage())
        .await
        .expect("same running update succeeds");

    // Replacing it with another running session operation conflicts.
    let attempted = SessionId::new();
    let error = store
        .update_state(
            AgentStatus::Running,
            Some(attempted),
            Some(TurnId::new()),
            usage(),
        )
        .await
        .expect_err("conflicting running update must fail");
    assert!(matches!(
        error,
        StoreError::RunningSessionConflict {
            current: Some(current),
            attempted: Some(attempted_session),
        } if current == session_a && attempted_session == attempted
    ));

    // Leaving the turn clears the conflict window.
    let idle = store
        .update_state(
            AgentStatus::Finished,
            Some(session_a),
            Some(turn_a),
            usage(),
        )
        .await
        .expect("finish update succeeds");
    assert_eq!(idle.status, AgentStatus::Finished);
    assert_eq!(idle.usage, usage());
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn store_complete_iteration_enforces_preconditions() {
    let backend = backend().await;
    let agent_id = AgentId::new();
    let store = backend.agent_store(agent_id);
    let state = store
        .initialize_with_model_config(agent_id, "writer".to_owned(), model_config())
        .await
        .expect("initialize succeeds");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();

    let error = store
        .complete_iteration(session_id, turn_id, 0, usage())
        .await
        .expect_err("idle agent cannot complete iterations");
    assert!(matches!(
        error,
        StoreError::AgentNotRunning {
            actual: AgentStatus::Idle
        }
    ));

    store
        .start_turn(
            &AgentRuntimeContext::direct(session_id),
            turn_id,
            snapshot_for(&state),
        )
        .await
        .expect("start_turn succeeds");

    let wrong_session = SessionId::new();
    let error = store
        .complete_iteration(wrong_session, turn_id, 0, usage())
        .await
        .expect_err("wrong session must fail");
    assert!(matches!(
        error,
        StoreError::SessionMismatch {
            expected,
            actual,
        } if expected == session_id && actual == wrong_session
    ));

    let error = store
        .complete_iteration(session_id, TurnId::new(), 0, usage())
        .await
        .expect_err("wrong turn must fail");
    assert!(matches!(error, StoreError::TurnMismatch { .. }));

    let completed = store
        .complete_iteration(session_id, turn_id, 0, usage())
        .await
        .expect("iteration 0 completes");
    assert_eq!(completed.next_iteration, 1);
    assert_eq!(completed.usage, usage());

    let error = store
        .complete_iteration(session_id, turn_id, 0, usage())
        .await
        .expect_err("stale iteration must fail");
    assert!(matches!(
        error,
        StoreError::IterationMismatch {
            expected: 1,
            actual: 0
        }
    ));
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn store_append_message_and_history_page_semantics() {
    let backend = backend().await;
    let agent_id = AgentId::new();
    let store = backend.agent_store(agent_id);
    let state = store
        .initialize_with_model_config(agent_id, "writer".to_owned(), model_config())
        .await
        .expect("initialize succeeds");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let context = AgentRuntimeContext::direct(session_id);
    store
        .start_turn(&context, turn_id, snapshot_for(&state))
        .await
        .expect("start_turn succeeds");

    let error = store
        .append_message(NewAgentMessage::new(
            &context,
            agent_id,
            turn_id,
            ChatMessage::system("not storable"),
        ))
        .await
        .expect_err("system messages are not storable");
    assert!(matches!(error, StoreError::InvalidMessageRole { .. }));

    let error = store
        .append_message(NewAgentMessage::new(
            &context,
            AgentId::new(),
            turn_id,
            ChatMessage::user("wrong agent"),
        ))
        .await
        .expect_err("wrong agent must fail");
    assert!(matches!(error, StoreError::AgentMismatch { .. }));

    for index in 0..5 {
        let envelope = store
            .append_message(NewAgentMessage::new(
                &context,
                agent_id,
                turn_id,
                ChatMessage::user(format!("message {index}")),
            ))
            .await
            .expect("append succeeds");
        assert_eq!(envelope.message_seq(), Some(index + 1));
        assert_eq!(envelope.session_id, session_id);
    }
    assert_eq!(store.load_agent().await.expect("load succeeds").last_seq, 5);

    let page = store
        .history_page(stratum_core::HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 256,
        })
        .await
        .expect("full page succeeds");
    assert_eq!(page.events.len(), 5);
    assert_eq!(page.through_seq, 5);
    assert_eq!(page.next_front_seq, 5);
    assert!(!page.has_more);

    let page = store
        .history_page(stratum_core::HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 2,
        })
        .await
        .expect("first page succeeds");
    assert_eq!(page.events.len(), 2);
    assert_eq!(page.next_front_seq, 2);
    assert!(page.has_more);

    let page = store
        .history_page(stratum_core::HistoryQuery {
            after_seq: page.next_front_seq,
            through_seq: Some(4),
            limit: 2,
        })
        .await
        .expect("continuation succeeds");
    assert_eq!(page.events.len(), 2);
    assert_eq!(page.events[0].message_seq(), Some(3));
    assert_eq!(page.events[1].message_seq(), Some(4));
    assert_eq!(page.next_front_seq, 4);
    assert!(!page.has_more);

    let page = store
        .history_page(stratum_core::HistoryQuery {
            after_seq: 4,
            through_seq: Some(4),
            limit: 2,
        })
        .await
        .expect("empty range succeeds");
    assert!(page.events.is_empty());
    assert!(!page.has_more);

    let error = store
        .history_page(stratum_core::HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 0,
        })
        .await
        .expect_err("zero limit must fail");
    assert!(matches!(error, StoreError::InvalidHistoryLimit { .. }));

    let error = store
        .history_page(stratum_core::HistoryQuery {
            after_seq: 0,
            through_seq: Some(6),
            limit: 10,
        })
        .await
        .expect_err("barrier beyond last must fail");
    assert!(matches!(
        error,
        StoreError::HistoryBarrierBeyondLast {
            through_seq: 6,
            last_seq: 5
        }
    ));

    let error = store
        .history_page(stratum_core::HistoryQuery {
            after_seq: 3,
            through_seq: Some(2),
            limit: 10,
        })
        .await
        .expect_err("inverted range must fail");
    assert!(matches!(error, StoreError::InvalidHistoryRange { .. }));
}

#[tokio::test]
#[ignore = "requires the postgres test container"]
async fn store_concurrent_append_message_allocates_unique_contiguous_sequences() {
    let backend = backend().await;
    let agent_id = AgentId::new();
    let store = backend.agent_store(agent_id);
    let state = store
        .initialize_with_model_config(agent_id, "writer".to_owned(), model_config())
        .await
        .expect("initialize succeeds");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let context = AgentRuntimeContext::direct(session_id);
    store
        .start_turn(&context, turn_id, snapshot_for(&state))
        .await
        .expect("start_turn succeeds");

    const TASKS: u64 = 6;
    const APPENDS_PER_TASK: u64 = 10;
    let mut handles = Vec::new();
    for task in 0..TASKS {
        let store = store.clone();
        let context = context.clone();
        handles.push(tokio::spawn(async move {
            let mut sequences = Vec::new();
            for index in 0..APPENDS_PER_TASK {
                let envelope = store
                    .append_message(NewAgentMessage::new(
                        &context,
                        agent_id,
                        turn_id,
                        ChatMessage::user(format!("task-{task}-message-{index}")),
                    ))
                    .await
                    .expect("append succeeds");
                sequences.push(envelope.message_seq().expect("message sequence"));
            }
            sequences
        }));
    }
    let mut all_sequences = BTreeSet::new();
    for handle in handles {
        for seq in handle.await.expect("append task joins") {
            assert!(all_sequences.insert(seq), "duplicate sequence {seq}");
        }
    }

    let expected: BTreeSet<u64> = (1..=TASKS * APPENDS_PER_TASK).collect();
    assert_eq!(all_sequences, expected);

    let page = store
        .history_page(stratum_core::HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 256,
        })
        .await
        .expect("history page succeeds");
    assert_eq!(
        u64::try_from(page.events.len()).expect("event count fits"),
        TASKS * APPENDS_PER_TASK
    );
}
