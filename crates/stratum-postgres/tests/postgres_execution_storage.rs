//! Container-backed integration tests for the Postgres execution storage.
//!
//! All tests are `#[ignore]` by default and run against the crate's compose
//! stack: `make test-integration` (or `make test-up` plus
//! `cargo test -p stratum-postgres -- --ignored --test-threads=1`). The
//! database URL defaults to the compose stack and can be overridden with
//! `STRATUM_POSTGRES_TEST_URL`. Tests truncate all four tables on entry and
//! must run single-threaded to stay deterministic.

use std::sync::Arc;

use serde_json::{Map, Value, json};
use sqlx::PgPool;
use stratum_core::{
    AgentId, AgentVersionId, ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel,
    DecideToolCallDecisionRecord, DurableAgentEvent, ExtensionSetVersionId, HookDecisionRecord,
    HookHandlerVersionId, HookInputDigest, HookInvocationId, HookPoint, ModelConfig, ModelId,
    SessionId, SkillSetVersionId, TokenUsage, ToolKind, ToolName, ToolSetFingerprint, TurnId,
    TurnRuntimeSnapshot,
};
use stratum_postgres::{
    AppendEvent, ApprovalLookup, BeginTurn, CompactionInput, CreateAgent, CreateAgentOutcome,
    EVENT_SEQ_MAX, HistoryQuery, HookInvocationLookup, PostgresBackend, PostgresError,
    ResolveApproval, ResolveApprovalOutcome, ResumeSliceQuery, VersionedKind,
};
use tokio::sync::Barrier;
use uuid::Uuid;

fn test_url() -> String {
    std::env::var("STRATUM_POSTGRES_TEST_URL").unwrap_or_else(|_| {
        "postgres://stratum:stratum@127.0.0.1:45432/stratum_test?sslmode=disable".to_owned()
    })
}

async fn raw_pool() -> PgPool {
    PgPool::connect(&test_url())
        .await
        .expect("raw test pool connects")
}

/// Connects (applying the baseline) and truncates every table so each test
/// starts from an empty execution store.
async fn reset_backend() -> PostgresBackend {
    let backend = PostgresBackend::connect(&test_url())
        .await
        .expect("postgres backend connects and migrates");
    let pool = raw_pool().await;
    sqlx::query("TRUNCATE transcript_compactions, durable_events, agent_state, agents")
        .execute(&pool)
        .await
        .expect("tables truncate");
    backend
}

fn model_config(model_name: &str) -> ModelConfig {
    ModelConfig::new(
        ModelId::new("openai", model_name).expect("static model id is valid"),
        Map::new(),
    )
}

fn fingerprint(byte: char) -> ToolSetFingerprint {
    byte.to_string()
        .repeat(64)
        .parse()
        .expect("fingerprint is valid")
}

fn snapshot(model: ModelConfig) -> TurnRuntimeSnapshot {
    TurnRuntimeSnapshot::new(
        AgentVersionId::new(),
        model,
        fingerprint('a'),
        SkillSetVersionId::new(),
        ExtensionSetVersionId::new(),
        vec![HookHandlerVersionId::new()],
    )
}

fn create_command(template: &str, key: Uuid, model_override: Option<ModelConfig>) -> CreateAgent {
    let effective = model_override
        .clone()
        .unwrap_or_else(|| model_config("default-model"));
    CreateAgent {
        agent_id: AgentId::new(),
        agent_version_id: AgentVersionId::new(),
        idempotency_key: key,
        source_template_name: template.to_owned(),
        creation_model_override: model_override,
        resolved_definition: json!({
            "name": template,
            "system_prompt": "you are a test agent",
            "tools": ["echo"],
            "model": effective,
        }),
        default_model_config: effective,
    }
}

async fn create_agent(backend: &PostgresBackend, template: &str) -> AgentId {
    let outcome = backend
        .create_agent(create_command(template, Uuid::now_v7(), None))
        .await
        .expect("create commits");
    match outcome {
        CreateAgentOutcome::Created { agent_id } => agent_id,
        _ => panic!("fresh key must create"),
    }
}

fn begin_command(
    agent_id: AgentId,
    expected: Option<TurnId>,
    session_id: SessionId,
) -> (BeginTurn, TurnId) {
    let turn_id = TurnId::new();
    (
        BeginTurn {
            agent_id,
            expected_current_turn_id: expected,
            turn_id,
            session_id,
            snapshot: snapshot(model_config("turn-model")),
        },
        turn_id,
    )
}

/// Creates an agent and admits its first turn; returns
/// `(session_id, turn_id)` with `LoopStarted` at sequence 1.
async fn running_turn(backend: &PostgresBackend, agent_id: AgentId) -> (SessionId, TurnId) {
    let session_id = SessionId::new();
    let (command, turn_id) = begin_command(agent_id, None, session_id);
    let receipt = backend
        .begin_turn(command)
        .await
        .expect("first turn admits");
    assert_eq!(receipt.event_seq, 1);
    (session_id, turn_id)
}

fn append_command(
    agent_id: AgentId,
    session_id: SessionId,
    turn_id: TurnId,
    event: DurableAgentEvent,
) -> AppendEvent {
    AppendEvent {
        agent_id,
        session_id,
        turn_id,
        event,
        approval_hook_invocation_id: None,
        default_model_update: None,
        compaction: None,
    }
}

async fn append_message(
    backend: &PostgresBackend,
    agent_id: AgentId,
    session_id: SessionId,
    turn_id: TurnId,
    text: &str,
) -> u64 {
    backend
        .append_event(append_command(
            agent_id,
            session_id,
            turn_id,
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user(text),
            },
        ))
        .await
        .expect("message appends")
        .event_seq
}

fn usage(input: u64) -> TokenUsage {
    TokenUsage {
        input_tokens: input,
        output_tokens: input + 1,
        total_tokens: 2 * input + 1,
    }
}

fn approval_request(approval_id: ApprovalId) -> DurableAgentEvent {
    DurableAgentEvent::ToolApprovalRequested {
        approval_id,
        call_id: CallId::from("call-1"),
        tool_name: ToolName::from("echo"),
        arguments: json!({ "text": "hello" }),
        tool_kind: ToolKind::Read,
        danger_level: DangerLevel::Low,
    }
}

fn resolve_command(
    agent_id: AgentId,
    turn_id: TurnId,
    approval_id: ApprovalId,
    decision: ApprovalDecision,
) -> ResolveApproval {
    ResolveApproval {
        agent_id,
        approval_id,
        turn_id,
        decision,
    }
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn baseline_applies_and_forbidden_beta_schema_is_absent() {
    let _backend = reset_backend().await;
    let pool = raw_pool().await;

    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT table_name FROM information_schema.tables \
         WHERE table_schema = 'public' AND table_type = 'BASE TABLE'",
    )
    .fetch_all(&pool)
    .await
    .expect("tables list");
    for expected in [
        "agents",
        "agent_state",
        "durable_events",
        "transcript_compactions",
    ] {
        assert!(tables.contains(&expected.to_owned()), "missing {expected}");
    }
    for forbidden in [
        "agent_messages",
        "tool_approvals",
        "session_operation_claims",
        "sessions",
    ] {
        assert!(
            !tables.contains(&forbidden.to_owned()),
            "forbidden beta table {forbidden} exists"
        );
    }

    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT table_name || '.' || column_name FROM information_schema.columns \
         WHERE table_schema = 'public'",
    )
    .fetch_all(&pool)
    .await
    .expect("columns list");
    // No per-turn sequence frontier and no message sequence allocator.
    assert!(!columns.contains(&"durable_events.seq".to_owned()));
    assert!(!columns.contains(&"agent_state.next_message_seq".to_owned()));
    assert!(
        !columns
            .iter()
            .any(|column| column.ends_with(".message_seq"))
    );
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn create_is_idempotent_per_key_and_conflicts_on_different_requests() {
    let backend = reset_backend().await;
    let key = Uuid::now_v7();
    let command = create_command("alpha", key, None);

    let first = backend
        .create_agent(command.clone())
        .await
        .expect("first create commits");
    let CreateAgentOutcome::Created { agent_id } = first else {
        panic!("first create must create");
    };

    let replay = backend
        .create_agent(command.clone())
        .await
        .expect("replay succeeds");
    assert_eq!(replay, CreateAgentOutcome::Replay { agent_id });

    let mut other_template = command.clone();
    other_template.source_template_name = "beta".to_owned();
    let conflict = backend
        .create_agent(other_template)
        .await
        .expect_err("different template conflicts");
    assert!(matches!(
        conflict,
        PostgresError::IdempotencyKeyConflict { idempotency_key } if idempotency_key == key
    ));

    let mut other_override = command.clone();
    other_override.creation_model_override = Some(model_config("other-model"));
    let conflict = backend
        .create_agent(other_override)
        .await
        .expect_err("different override conflicts");
    assert!(matches!(
        conflict,
        PostgresError::IdempotencyKeyConflict { .. }
    ));

    // A different key from the same template creates a distinct Agent.
    let second = backend
        .create_agent(create_command("alpha", Uuid::now_v7(), None))
        .await
        .expect("second key creates");
    let CreateAgentOutcome::Created {
        agent_id: second_id,
    } = second
    else {
        panic!("second key must create");
    };
    assert_ne!(agent_id, second_id);

    let state = backend
        .read_agent_state(agent_id)
        .await
        .expect("state reads");
    assert_eq!(state.status, stratum_postgres::AgentStatus::Idle);
    assert_eq!(state.session_id, None);
    assert_eq!(state.current_turn_id, None);
    assert_eq!(state.last_event_seq, 0);
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn concurrent_create_with_one_key_converges_on_one_agent() {
    let backend = reset_backend().await;
    let key = Uuid::now_v7();

    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let backend = backend.clone();
        tasks.spawn(async move {
            backend
                .create_agent(create_command("alpha", key, None))
                .await
                .expect("concurrent create resolves")
        });
    }
    let mut outcomes = Vec::new();
    while let Some(outcome) = tasks.join_next().await {
        outcomes.push(outcome.expect("task joins"));
    }

    let created = outcomes
        .iter()
        .filter(|outcome| matches!(outcome, CreateAgentOutcome::Created { .. }))
        .count();
    assert_eq!(created, 1, "exactly one concurrent create wins");
    let agent_ids: Vec<AgentId> = outcomes
        .iter()
        .map(|outcome| match outcome {
            CreateAgentOutcome::Created { agent_id } | CreateAgentOutcome::Replay { agent_id } => {
                *agent_id
            }
            _ => panic!("outcome is closed"),
        })
        .collect();
    assert!(
        agent_ids.windows(2).all(|pair| pair[0] == pair[1]),
        "all outcomes reference the same agent"
    );
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn begin_turn_enforces_cas_session_binding_and_single_active() {
    let backend = reset_backend().await;

    // Unknown agent.
    let missing = AgentId::new();
    let (command, _) = begin_command(missing, None, SessionId::new());
    let error = backend
        .begin_turn(command)
        .await
        .expect_err("unknown agent");
    assert!(matches!(error, PostgresError::AgentNotFound { .. }));

    let agent = create_agent(&backend, "alpha").await;
    let session = SessionId::new();

    // Wrong first-admission CAS expectation.
    let (command, _) = begin_command(agent, Some(TurnId::new()), session);
    let error = backend.begin_turn(command).await.expect_err("stale cas");
    assert!(matches!(
        error,
        PostgresError::StaleTurn {
            expected: Some(_),
            actual: None,
            ..
        }
    ));

    // First admission binds the session.
    let (command, turn_one) = begin_command(agent, None, session);
    let receipt = backend
        .begin_turn(command)
        .await
        .expect("admission commits");
    assert_eq!(receipt.event_seq, 1);

    // A stale expectation fails as StaleTurn even while the agent is running:
    // a lost-response retry with the old expected value must never see busy.
    let (command, _) = begin_command(agent, None, session);
    let error = backend
        .begin_turn(command)
        .await
        .expect_err("stale first-admission expectation while running");
    assert!(matches!(
        error,
        PostgresError::StaleTurn {
            expected: None,
            actual: Some(actual),
            ..
        } if actual == turn_one
    ));
    let (command, _) = begin_command(agent, Some(TurnId::new()), session);
    let error = backend
        .begin_turn(command)
        .await
        .expect_err("stale expectation while running");
    assert!(matches!(error, PostgresError::StaleTurn { .. }));

    // A running agent rejects admission when the expectation still matches.
    let (command, _) = begin_command(agent, Some(turn_one), session);
    let error = backend.begin_turn(command).await.expect_err("busy agent");
    assert!(matches!(error, PostgresError::AgentBusy { .. }));

    // A second agent cannot run on the same session.
    let other = create_agent(&backend, "beta").await;
    let (command, _) = begin_command(other, None, session);
    let error = backend.begin_turn(command).await.expect_err("session busy");
    assert!(matches!(
        error,
        PostgresError::SessionBusy { session_id } if session_id == session
    ));
    // The failed admission left no durable rows behind.
    let rows = backend
        .read_events_range(other, 0, 100)
        .await
        .expect("range reads");
    assert!(rows.is_empty());
    let state = backend.read_agent_state(other).await.expect("state reads");
    assert_eq!(state.status, stratum_postgres::AgentStatus::Idle);

    // Finish the turn; the next admission reuses the bound session.
    backend
        .append_event(append_command(
            agent,
            session,
            turn_one,
            DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage: usage(1),
            },
        ))
        .await
        .expect("terminal appends");

    let (command, _) = begin_command(agent, Some(turn_one), SessionId::new());
    let error = backend
        .begin_turn(command)
        .await
        .expect_err("session mismatch");
    assert!(matches!(error, PostgresError::SessionMismatch { .. }));

    let (command, _) = begin_command(agent, None, session);
    let error = backend.begin_turn(command).await.expect_err("stale cas");
    assert!(matches!(error, PostgresError::StaleTurn { .. }));

    let (command, turn_two) = begin_command(agent, Some(turn_one), session);
    let receipt = backend
        .begin_turn(command)
        .await
        .expect("second turn admits");
    assert_eq!(receipt.event_seq, 3, "sequence continues across turns");

    // Once the first agent finished, the freed session admits another agent —
    // but only after it actually went terminal.
    let (command, _) = begin_command(other, None, session);
    let error = backend
        .begin_turn(command)
        .await
        .expect_err("still busy while another turn runs");
    assert!(matches!(error, PostgresError::SessionBusy { .. }));

    let state = backend.read_agent_state(agent).await.expect("state reads");
    assert_eq!(state.status, stratum_postgres::AgentStatus::Running);
    assert_eq!(state.current_turn_id, Some(turn_two));
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn concurrent_appends_linearize_with_gapless_sequence() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;

    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..8 {
        let backend = backend.clone();
        tasks.spawn(async move {
            backend
                .append_event(append_command(
                    agent,
                    session,
                    turn,
                    DurableAgentEvent::MessageAppended {
                        message: ChatMessage::assistant(format!("message {index}")),
                    },
                ))
                .await
                .expect("concurrent append commits")
                .event_seq
        });
    }
    let mut seqs: Vec<u64> = Vec::new();
    while let Some(result) = tasks.join_next().await {
        seqs.push(result.expect("task joins"));
    }
    seqs.sort_unstable();
    assert_eq!(
        seqs,
        (2..=9).collect::<Vec<_>>(),
        "adjacent unique sequences"
    );

    let state = backend.read_agent_state(agent).await.expect("state reads");
    assert_eq!(state.last_event_seq, 9);

    let rows = backend
        .read_events_range(agent, 0, 9)
        .await
        .expect("range reads");
    let persisted: Vec<u64> = rows.iter().map(|row| row.event_seq).collect();
    assert_eq!(persisted, (1..=9).collect::<Vec<_>>(), "no gaps");
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn approval_resolver_and_terminal_append_linearize_without_gaps() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;
    let approval = ApprovalId::new();
    let mut request = append_command(agent, session, turn, approval_request(approval));
    request.approval_hook_invocation_id = Some(HookInvocationId::new());
    let request_seq = backend
        .append_event(request)
        .await
        .expect("approval request appends")
        .event_seq;

    // Both writers cross the same barrier before contending for the exact
    // agent_state row lock. The resolver and the kernel-style terminal append
    // must therefore linearize into exactly one of the two outcomes below.
    let barrier = Arc::new(Barrier::new(3));
    let resolve_task = {
        let backend = backend.clone();
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            backend
                .resolve_approval(resolve_command(
                    agent,
                    turn,
                    approval,
                    ApprovalDecision::Approve,
                ))
                .await
        })
    };
    let terminal_task = {
        let backend = backend.clone();
        let barrier = Arc::clone(&barrier);
        tokio::spawn(async move {
            barrier.wait().await;
            backend
                .append_event(append_command(
                    agent,
                    session,
                    turn,
                    DurableAgentEvent::LoopFinished {
                        finish_reason: "stop".to_owned(),
                        usage: usage(1),
                    },
                ))
                .await
        })
    };
    barrier.wait().await;

    let resolve_result = resolve_task.await.expect("resolver task joins");
    let terminal_receipt = terminal_task
        .await
        .expect("terminal task joins")
        .expect("terminal append always commits");
    let state = backend.read_agent_state(agent).await.expect("state reads");
    assert_eq!(state.status, stratum_postgres::AgentStatus::Finished);
    assert_eq!(state.last_event_seq, terminal_receipt.event_seq);

    let rows = backend
        .read_events_range(agent, 0, state.last_event_seq)
        .await
        .expect("truth range reads");
    let persisted: Vec<u64> = rows.iter().map(|row| row.event_seq).collect();
    assert_eq!(
        persisted,
        (1..=state.last_event_seq).collect::<Vec<_>>(),
        "resolver/terminal contention never leaves an event_seq gap"
    );
    assert!(matches!(
        rows.last().map(|row| &row.event),
        Some(DurableAgentEvent::LoopFinished { .. })
    ));
    let resolved_seq = rows.iter().find_map(|row| match &row.event {
        DurableAgentEvent::ToolApprovalResolved {
            approval_id,
            decision,
        } if *approval_id == approval && *decision == ApprovalDecision::Approve => {
            Some(row.event_seq)
        }
        _ => None,
    });

    match resolve_result {
        Ok(ResolveApprovalOutcome::Resolved { receipt }) => {
            let expected_resolved = request_seq.checked_add(1).expect("test sequence has room");
            let expected_terminal = receipt
                .event_seq
                .checked_add(1)
                .expect("test sequence has room");
            assert_eq!(receipt.event_seq, expected_resolved);
            assert_eq!(resolved_seq, Some(receipt.event_seq));
            assert_eq!(terminal_receipt.event_seq, expected_terminal);
        }
        Err(PostgresError::ApprovalInvalidated { approval_id }) => {
            assert_eq!(approval_id, approval);
            assert_eq!(resolved_seq, None);
            assert_eq!(
                terminal_receipt.event_seq,
                request_seq.checked_add(1).expect("test sequence has room")
            );
        }
        other => panic!("unexpected resolver/terminal linearization: {other:?}"),
    }
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn failed_append_rolls_back_without_consuming_sequence() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;
    assert_eq!(
        append_message(&backend, agent, session, turn, "hi").await,
        2
    );

    // A compaction whose retained pointer addresses nothing fails closed.
    let mut command = append_command(
        agent,
        session,
        turn,
        DurableAgentEvent::TranscriptCompacted {
            upto: 1,
            summary: ChatMessage::system("summary"),
            compacted_iteration: 1,
        },
    );
    command.compaction = Some(CompactionInput {
        compacted_iteration: 1,
        upto: 1,
        retained_from_event_seq: 99,
        summary: ChatMessage::system("summary"),
    });
    let error = backend
        .append_event(command)
        .await
        .expect_err("invalid pointer fails");
    assert!(matches!(
        error,
        PostgresError::InvalidCompactionPointer {
            retained_from_event_seq: 99,
            ..
        }
    ));

    // Command-shape invariants are rejected before any write.
    let mut malformed = append_command(
        agent,
        session,
        turn,
        DurableAgentEvent::TranscriptCompacted {
            upto: 1,
            summary: ChatMessage::system("summary"),
            compacted_iteration: 1,
        },
    );
    malformed.compaction = None;
    let error = backend
        .append_event(malformed)
        .await
        .expect_err("missing companion fails");
    assert!(matches!(error, PostgresError::InvalidCommand(_)));

    // The next successful append reuses the next sequence value: no gap.
    assert_eq!(
        append_message(&backend, agent, session, turn, "after failure").await,
        3
    );
    let state = backend.read_agent_state(agent).await.expect("state reads");
    assert_eq!(state.last_event_seq, 3);
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn transcript_compaction_commits_atomically_and_validates_pointer() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;
    assert_eq!(
        append_message(&backend, agent, session, turn, "first").await,
        2
    );
    assert_eq!(
        append_message(&backend, agent, session, turn, "second").await,
        3
    );

    // The retained pointer must address a real earlier MessageAppended; a
    // loop_started row is not a message.
    let mut command = append_command(
        agent,
        session,
        turn,
        DurableAgentEvent::TranscriptCompacted {
            upto: 1,
            summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary"),
            compacted_iteration: 1,
        },
    );
    command.compaction = Some(CompactionInput {
        compacted_iteration: 1,
        upto: 1,
        retained_from_event_seq: 1,
        summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary"),
    });
    let error = backend
        .append_event(command)
        .await
        .expect_err("non-message pointer fails");
    assert!(matches!(
        error,
        PostgresError::InvalidCompactionPointer { .. }
    ));

    // A valid compaction commits discriminator and companion atomically.
    let mut command = append_command(
        agent,
        session,
        turn,
        DurableAgentEvent::TranscriptCompacted {
            upto: 1,
            summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary"),
            compacted_iteration: 1,
        },
    );
    command.compaction = Some(CompactionInput {
        compacted_iteration: 1,
        upto: 1,
        retained_from_event_seq: 3,
        summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary"),
    });
    let receipt = backend
        .append_event(command)
        .await
        .expect("compaction commits");
    assert_eq!(receipt.event_seq, 4);

    // The durable payload is exactly the empty object; the summary lives only
    // in the companion.
    let payload: Value = sqlx::query_scalar(
        "SELECT payload FROM durable_events WHERE agent_id = $1 AND event_seq = 4",
    )
    .bind(agent.as_uuid())
    .fetch_one(&raw_pool().await)
    .await
    .expect("payload reads");
    assert_eq!(payload, json!({}));

    // The typed event is materialized by joining the companion.
    let rows = backend
        .read_events_range(agent, 0, 4)
        .await
        .expect("range reads");
    assert_eq!(rows.len(), 4);
    assert_eq!(
        rows[3].event,
        DurableAgentEvent::TranscriptCompacted {
            upto: 1,
            summary: ChatMessage::system("[stratum:transcript-compacted]\nsummary"),
            compacted_iteration: 1,
        }
    );

    // The latest companion is visible at or below its own sequence only.
    let companion = backend
        .read_latest_companion(agent, 4)
        .await
        .expect("companion reads")
        .expect("companion exists");
    assert_eq!(companion.event_seq, 4);
    assert_eq!(companion.retained_from_event_seq, 3);
    assert_eq!(companion.turn_id, turn);
    set_event_version(agent, receipt.event_seq, 2).await;
    let error = backend
        .read_latest_companion(agent, receipt.event_seq)
        .await
        .expect_err("unsupported companion discriminator version fails closed");
    assert_incompatible(error, 2);
    set_event_version(agent, receipt.event_seq, 1).await;
    assert!(
        backend
            .read_latest_companion(agent, 3)
            .await
            .expect("companion reads")
            .is_none()
    );

    // History exposes the compaction as a materialized typed marker.
    let page = backend
        .read_history_page(HistoryQuery {
            agent_id: agent,
            through_event_seq: 4,
            before_event_seq: None,
            limit: 50,
        })
        .await
        .expect("history reads");
    let seqs: Vec<u64> = page.items.iter().map(|item| item.event_seq).collect();
    assert_eq!(seqs, vec![2, 3, 4]);
    assert!(matches!(
        page.items.last().map(|item| &item.event),
        Some(DurableAgentEvent::TranscriptCompacted { .. })
    ));

    // A manually corrupted locator is surfaced as stored and never repaired
    // by reads. The API baseline owns the pure message-row usability check and
    // falls back to full replay for this value.
    let pool = raw_pool().await;
    sqlx::query(
        "UPDATE transcript_compactions SET retained_from_event_seq = 1 \
         WHERE agent_id = $1 AND event_seq = $2",
    )
    .bind(agent.as_uuid())
    .bind(i64::try_from(receipt.event_seq).expect("test sequence fits bigint"))
    .execute(&pool)
    .await
    .expect("locator corruption applies");
    let corrupted = backend
        .read_latest_companion(agent, receipt.event_seq)
        .await
        .expect("corrupted locator still reads")
        .expect("companion exists");
    assert_eq!(corrupted.retained_from_event_seq, 1);
    let stored_pointer: i64 = sqlx::query_scalar(
        "SELECT retained_from_event_seq FROM transcript_compactions \
         WHERE agent_id = $1 AND event_seq = $2",
    )
    .bind(agent.as_uuid())
    .bind(i64::try_from(receipt.event_seq).expect("test sequence fits bigint"))
    .fetch_one(&pool)
    .await
    .expect("stored locator reads");
    assert_eq!(stored_pointer, 1, "read path never repairs the locator");
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn non_compaction_event_with_a_companion_row_fails_closed() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;
    assert_eq!(
        append_message(&backend, agent, session, turn, "first").await,
        2
    );
    let message_event_seq = append_message(&backend, agent, session, turn, "second").await;
    assert_eq!(message_event_seq, 3);

    insert_illegal_companion(agent, message_event_seq, turn, 2).await;

    let error = backend
        .read_events_range(agent, 0, message_event_seq)
        .await
        .expect_err("non-compaction companion fails closed");
    assert_corrupt(error);

    // AgentView's latest-usage selector must join the same companion relation.
    let usage_agent = create_agent(&backend, "usage").await;
    let (usage_session, usage_turn) = running_turn(&backend, usage_agent).await;
    let usage_retained =
        append_message(&backend, usage_agent, usage_session, usage_turn, "retained").await;
    let usage_seq = backend
        .append_event(append_command(
            usage_agent,
            usage_session,
            usage_turn,
            DurableAgentEvent::IterationCompleted {
                iteration: 0,
                usage: usage(1),
            },
        ))
        .await
        .expect("usage appends")
        .event_seq;
    insert_illegal_companion(usage_agent, usage_seq, usage_turn, usage_retained).await;
    let error = backend
        .read_agent_view(usage_agent)
        .await
        .expect_err("latest usage companion fails closed");
    assert_corrupt(error);

    // A malformed companion on the Requested row is rejected by pending-view,
    // exact approval lookup, and the resolver before it can append a decision.
    let approval_agent = create_agent(&backend, "approval-request").await;
    let (approval_session, approval_turn) = running_turn(&backend, approval_agent).await;
    let approval_retained = append_message(
        &backend,
        approval_agent,
        approval_session,
        approval_turn,
        "retained",
    )
    .await;
    let approval_id = ApprovalId::new();
    let hook_invocation_id = HookInvocationId::new();
    let mut request = append_command(
        approval_agent,
        approval_session,
        approval_turn,
        approval_request(approval_id),
    );
    request.approval_hook_invocation_id = Some(hook_invocation_id);
    let request_seq = backend
        .append_event(request)
        .await
        .expect("approval request appends")
        .event_seq;
    insert_illegal_companion(
        approval_agent,
        request_seq,
        approval_turn,
        approval_retained,
    )
    .await;
    let error = backend
        .read_agent_view(approval_agent)
        .await
        .expect_err("pending approval companion fails closed");
    assert_corrupt(error);
    let error = backend
        .read_approval(
            approval_agent,
            approval_turn,
            ApprovalLookup::ByApprovalId(approval_id),
        )
        .await
        .expect_err("approval lookup companion fails closed");
    assert_corrupt(error);
    let error = backend
        .resolve_approval(resolve_command(
            approval_agent,
            approval_turn,
            approval_id,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("approval request companion prevents resolution");
    assert_corrupt(error);

    // A companion on an existence-only Resolved fact is explicit corruption;
    // it cannot silently hide the pending approval through NOT EXISTS.
    let resolved_agent = create_agent(&backend, "approval-resolved").await;
    let (resolved_session, resolved_turn) = running_turn(&backend, resolved_agent).await;
    let resolved_retained = append_message(
        &backend,
        resolved_agent,
        resolved_session,
        resolved_turn,
        "retained",
    )
    .await;
    let resolved_approval_id = ApprovalId::new();
    let mut request = append_command(
        resolved_agent,
        resolved_session,
        resolved_turn,
        approval_request(resolved_approval_id),
    );
    request.approval_hook_invocation_id = Some(HookInvocationId::new());
    backend
        .append_event(request)
        .await
        .expect("approval request appends");
    let ResolveApprovalOutcome::Resolved { receipt } = backend
        .resolve_approval(resolve_command(
            resolved_agent,
            resolved_turn,
            resolved_approval_id,
            ApprovalDecision::Approve,
        ))
        .await
        .expect("resolution appends")
    else {
        panic!("first resolution must append");
    };
    insert_illegal_companion(
        resolved_agent,
        receipt.event_seq,
        resolved_turn,
        resolved_retained,
    )
    .await;
    let error = backend
        .read_agent_view(resolved_agent)
        .await
        .expect_err("resolved companion cannot silently exclude pending approval");
    assert_corrupt(error);
    let error = backend
        .resolve_approval(resolve_command(
            resolved_agent,
            resolved_turn,
            resolved_approval_id,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("resolved companion prevents idempotent resolution");
    assert_corrupt(error);

    // Open-hook derivation checks both the selected Pending row and the
    // Completed/Failed facts used by its NOT EXISTS exclusion.
    let hook_agent = create_agent(&backend, "hook-pending").await;
    let (hook_session, hook_turn) = running_turn(&backend, hook_agent).await;
    let hook_retained =
        append_message(&backend, hook_agent, hook_session, hook_turn, "retained").await;
    let pending_invocation_id = HookInvocationId::new();
    let pending_seq = backend
        .append_event(append_command(
            hook_agent,
            hook_session,
            hook_turn,
            DurableAgentEvent::HookInvocationPending {
                invocation_id: pending_invocation_id,
                point: HookPoint::DecideToolCall,
                iteration: 0,
                call_id: Some(CallId::from("call-pending")),
                input_digest: "c".repeat(64).parse::<HookInputDigest>().expect("digest"),
            },
        ))
        .await
        .expect("hook pending appends")
        .event_seq;
    insert_illegal_companion(hook_agent, pending_seq, hook_turn, hook_retained).await;
    let error = backend
        .read_open_hook_invocation(HookInvocationLookup {
            agent_id: hook_agent,
            turn_id: hook_turn,
            point: HookPoint::DecideToolCall,
            iteration: 0,
            call_id: Some(CallId::from("call-pending")),
        })
        .await
        .expect_err("pending hook companion fails closed");
    assert_corrupt(error);

    let completed_agent = create_agent(&backend, "hook-completed").await;
    let (completed_session, completed_turn) = running_turn(&backend, completed_agent).await;
    let completed_retained = append_message(
        &backend,
        completed_agent,
        completed_session,
        completed_turn,
        "retained",
    )
    .await;
    let completed_invocation_id = HookInvocationId::new();
    backend
        .append_event(append_command(
            completed_agent,
            completed_session,
            completed_turn,
            DurableAgentEvent::HookInvocationPending {
                invocation_id: completed_invocation_id,
                point: HookPoint::DecideToolCall,
                iteration: 0,
                call_id: Some(CallId::from("call-completed")),
                input_digest: "d".repeat(64).parse::<HookInputDigest>().expect("digest"),
            },
        ))
        .await
        .expect("hook pending appends");
    let completed_seq = backend
        .append_event(append_command(
            completed_agent,
            completed_session,
            completed_turn,
            DurableAgentEvent::HookInvocationCompleted {
                invocation_id: completed_invocation_id,
                decision: HookDecisionRecord::DecideToolCall(DecideToolCallDecisionRecord::Execute),
            },
        ))
        .await
        .expect("hook completion appends")
        .event_seq;
    insert_illegal_companion(
        completed_agent,
        completed_seq,
        completed_turn,
        completed_retained,
    )
    .await;
    let error = backend
        .read_open_hook_invocation(HookInvocationLookup {
            agent_id: completed_agent,
            turn_id: completed_turn,
            point: HookPoint::DecideToolCall,
            iteration: 0,
            call_id: Some(CallId::from("call-completed")),
        })
        .await
        .expect_err("completed hook companion cannot silently consume pending");
    assert_corrupt(error);
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn approval_resolution_follows_the_exact_matrix() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;

    // Unknown approval and wrong turn fence first.
    let error = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            ApprovalId::new(),
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("unknown approval");
    assert!(matches!(error, PostgresError::ApprovalNotFound { .. }));

    let approval_one = ApprovalId::new();
    let hook_one = HookInvocationId::new();
    let mut command = append_command(agent, session, turn, approval_request(approval_one));
    command.approval_hook_invocation_id = Some(hook_one);
    backend
        .append_event(command)
        .await
        .expect("request appends");

    // Duplicate request identity for the same hook invocation fails.
    let mut duplicate = append_command(agent, session, turn, approval_request(ApprovalId::new()));
    duplicate.approval_hook_invocation_id = Some(hook_one);
    let error = backend
        .append_event(duplicate)
        .await
        .expect_err("duplicate request fails");
    assert!(matches!(
        error,
        PostgresError::ApprovalAlreadyRequested {
            hook_invocation_id
        } if hook_invocation_id == hook_one
    ));

    // ApprovalId is independently unique: another invocation cannot alias the
    // resolver/read identity of the first request.
    let mut duplicate_id = append_command(agent, session, turn, approval_request(approval_one));
    duplicate_id.approval_hook_invocation_id = Some(HookInvocationId::new());
    let error = backend
        .append_event(duplicate_id)
        .await
        .expect_err("duplicate approval identity fails");
    assert!(matches!(
        error,
        PostgresError::ApprovalIdConflict { approval_id } if approval_id == approval_one
    ));

    // Handler lookups work by approval id and by hook invocation id.
    let facts = backend
        .read_approval(agent, turn, ApprovalLookup::ByApprovalId(approval_one))
        .await
        .expect("approval reads")
        .expect("approval exists");
    assert_eq!(facts.approval_id, approval_one);
    assert_eq!(facts.hook_invocation_id, hook_one);
    assert_eq!(facts.resolution, None);
    let by_hook = backend
        .read_approval(agent, turn, ApprovalLookup::ByHookInvocationId(hook_one))
        .await
        .expect("approval reads")
        .expect("approval exists");
    assert_eq!(by_hook.approval_id, approval_one);

    // The approval is pending in the view.
    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.pending_approvals.len(), 1);
    assert_eq!(view.pending_approvals[0].approval_id, approval_one);

    // Wrong turn id is a stale-turn fence.
    let error = backend
        .resolve_approval(resolve_command(
            agent,
            TurnId::new(),
            approval_one,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("wrong turn");
    assert!(matches!(error, PostgresError::StaleTurn { .. }));

    // First decision commits.
    let outcome = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval_one,
            ApprovalDecision::Approve,
        ))
        .await
        .expect("first resolve commits");
    let ResolveApprovalOutcome::Resolved { receipt } = outcome else {
        panic!("first resolve must commit");
    };
    assert_eq!(receipt.event_seq, 3);

    // Identical retry is an idempotent success and appends nothing.
    let outcome = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval_one,
            ApprovalDecision::Approve,
        ))
        .await
        .expect("same decision succeeds");
    assert_eq!(outcome, ResolveApprovalOutcome::AlreadyResolvedSame);
    let state = backend.read_agent_state(agent).await.expect("state reads");
    assert_eq!(state.last_event_seq, 3, "no second resolved row");

    // The opposite decision conflicts.
    let error = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval_one,
            ApprovalDecision::Reject,
        ))
        .await
        .expect_err("opposite decision conflicts");
    assert!(matches!(
        error,
        PostgresError::ApprovalAlreadyResolved { .. }
    ));

    // A second approval stays independent, then the turn goes terminal.
    let approval_two = ApprovalId::new();
    let mut command = append_command(agent, session, turn, approval_request(approval_two));
    command.approval_hook_invocation_id = Some(HookInvocationId::new());
    backend
        .append_event(command)
        .await
        .expect("second request appends");
    backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::LoopCancelled { usage: usage(1) },
        ))
        .await
        .expect("terminal appends");

    // Terminal invalidates the undecided approval — and takes priority over
    // the already-resolved one.
    let error = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval_two,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("terminal invalidates");
    assert!(matches!(error, PostgresError::ApprovalInvalidated { .. }));
    let error = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval_one,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("terminal wins over resolved");
    assert!(matches!(error, PostgresError::ApprovalInvalidated { .. }));

    // A terminal turn reports no pending approvals.
    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert!(view.pending_approvals.is_empty());
    assert_eq!(view.status, stratum_postgres::AgentStatus::Cancelled);

    // A resolution carrying the same Agent-wide approval identity but a
    // foreign Turn must not consume the current request. The resolver detects
    // that impossible identity relation explicitly instead of falling through
    // to a unique-index error classified as store unavailability.
    let foreign_agent = create_agent(&backend, "foreign-resolution-turn").await;
    let (foreign_session, foreign_turn) = running_turn(&backend, foreign_agent).await;
    let foreign_approval = ApprovalId::new();
    let mut request = append_command(
        foreign_agent,
        foreign_session,
        foreign_turn,
        approval_request(foreign_approval),
    );
    request.approval_hook_invocation_id = Some(HookInvocationId::new());
    backend
        .append_event(request)
        .await
        .expect("foreign-turn fixture request appends");
    let ResolveApprovalOutcome::Resolved { receipt } = backend
        .resolve_approval(resolve_command(
            foreign_agent,
            foreign_turn,
            foreign_approval,
            ApprovalDecision::Approve,
        ))
        .await
        .expect("foreign-turn fixture resolution appends")
    else {
        panic!("foreign-turn fixture resolution must append");
    };
    sqlx::query("UPDATE durable_events SET turn_id = $3 WHERE agent_id = $1 AND event_seq = $2")
        .bind(foreign_agent.as_uuid())
        .bind(i64::try_from(receipt.event_seq).expect("test sequence fits bigint"))
        .bind(TurnId::new().as_uuid())
        .execute(&raw_pool().await)
        .await
        .expect("resolution moves to a foreign turn");

    let view = backend
        .read_agent_view(foreign_agent)
        .await
        .expect("foreign resolution does not consume current request");
    assert_eq!(view.pending_approvals.len(), 1);
    assert_eq!(view.pending_approvals[0].approval_id, foreign_approval);
    let error = backend
        .resolve_approval(resolve_command(
            foreign_agent,
            foreign_turn,
            foreign_approval,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("foreign-turn resolution fails closed");
    assert_corrupt(error);
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn history_paginates_ascending_with_cursor_and_skips_internal_rows() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;

    for index in 0..5 {
        append_message(&backend, agent, session, turn, &format!("message {index}")).await;
    }
    // A hook journal row occupies a sequence but is not product-visible.
    backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::HookInvocationPending {
                invocation_id: HookInvocationId::new(),
                point: stratum_core::HookPoint::TransformContext,
                iteration: 0,
                call_id: None,
                input_digest: "b".repeat(64).parse().expect("digest is valid"),
            },
        ))
        .await
        .expect("hook journal appends");
    backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::LoopFailed {
                error_text: "provider unavailable".to_owned(),
                usage: usage(1),
            },
        ))
        .await
        .expect("terminal appends");
    let barrier = backend
        .read_agent_state(agent)
        .await
        .expect("state reads")
        .last_event_seq;
    assert_eq!(barrier, 8);

    // Full page: five messages plus the failed marker, ascending; finished is
    // not a history marker and the hook row leaves a product-visible gap.
    let page = backend
        .read_history_page(HistoryQuery {
            agent_id: agent,
            through_event_seq: barrier,
            before_event_seq: None,
            limit: 50,
        })
        .await
        .expect("history reads");
    let seqs: Vec<u64> = page.items.iter().map(|item| item.event_seq).collect();
    assert_eq!(seqs, vec![2, 3, 4, 5, 6, 8]);
    assert!(!page.has_more);
    assert_eq!(page.next_before_event_seq, Some(2));
    assert!(matches!(
        page.items.last().map(|item| &item.event),
        Some(DurableAgentEvent::LoopFailed { .. })
    ));

    // Cursor pagination walks backwards in ascending pages.
    let page = backend
        .read_history_page(HistoryQuery {
            agent_id: agent,
            through_event_seq: barrier,
            before_event_seq: None,
            limit: 2,
        })
        .await
        .expect("history reads");
    let seqs: Vec<u64> = page.items.iter().map(|item| item.event_seq).collect();
    assert_eq!(seqs, vec![6, 8]);
    assert!(page.has_more);
    assert_eq!(page.next_before_event_seq, Some(6));

    let page = backend
        .read_history_page(HistoryQuery {
            agent_id: agent,
            through_event_seq: barrier,
            before_event_seq: page.next_before_event_seq,
            limit: 2,
        })
        .await
        .expect("history reads");
    let seqs: Vec<u64> = page.items.iter().map(|item| item.event_seq).collect();
    assert_eq!(seqs, vec![4, 5]);
    assert!(page.has_more);

    let mut cursor = page.next_before_event_seq;
    let mut walked = Vec::new();
    while let Some(before) = cursor {
        let page = backend
            .read_history_page(HistoryQuery {
                agent_id: agent,
                through_event_seq: barrier,
                before_event_seq: Some(before),
                limit: 2,
            })
            .await
            .expect("history reads");
        walked.extend(page.items.iter().map(|item| item.event_seq));
        cursor = if page.has_more {
            page.next_before_event_seq
        } else {
            None
        };
    }
    assert_eq!(walked, vec![2, 3], "all older rows exactly once");
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn history_soft_budget_keeps_first_oversized_item_whole() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;

    // Two ~700 KiB messages: together they exceed the 1 MiB soft budget.
    let big = "x".repeat(700_000);
    append_message(&backend, agent, session, turn, &big).await;
    append_message(&backend, agent, session, turn, &big).await;
    let barrier = backend
        .read_agent_state(agent)
        .await
        .expect("state reads")
        .last_event_seq;

    let page = backend
        .read_history_page(HistoryQuery {
            agent_id: agent,
            through_event_seq: barrier,
            before_event_seq: None,
            limit: 10,
        })
        .await
        .expect("history reads");
    assert_eq!(page.items.len(), 1, "only the newest oversized item fits");
    assert_eq!(page.items[0].event_seq, 3);
    assert!(page.has_more, "the trimmed older row still exists");
    assert_eq!(page.next_before_event_seq, Some(3));

    // A single item larger than the budget is still returned whole.
    let huge = "y".repeat(1_200_000);
    append_message(&backend, agent, session, turn, &huge).await;
    let barrier = backend
        .read_agent_state(agent)
        .await
        .expect("state reads")
        .last_event_seq;
    let page = backend
        .read_history_page(HistoryQuery {
            agent_id: agent,
            through_event_seq: barrier,
            before_event_seq: None,
            limit: 1,
        })
        .await
        .expect("history reads");
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].event_seq, 4);
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn started_only_turn_reconciles_with_atomic_loop_failed() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;

    // The started-only reconciliation is an ordinary centralized append.
    let receipt = backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::LoopFailed {
                error_text: "turn preamble incomplete".to_owned(),
                usage: usage(0),
            },
        ))
        .await
        .expect("reconciliation appends");
    assert_eq!(receipt.event_seq, 2);

    let state = backend.read_agent_state(agent).await.expect("state reads");
    assert_eq!(state.status, stratum_postgres::AgentStatus::Failed);
    assert_eq!(state.session_id, Some(session));
    assert_eq!(state.current_turn_id, Some(turn), "recent turn is retained");

    // A second terminal event can never be appended.
    let error = backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::LoopCancelled { usage: usage(0) },
        ))
        .await
        .expect_err("second terminal rejected");
    assert!(matches!(error, PostgresError::TurnNotRunning { .. }));
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn resume_reads_verify_snapshot_identity_and_continuity() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;
    append_message(&backend, agent, session, turn, "hello").await;

    let started = backend
        .read_loop_started(agent, turn)
        .await
        .expect("loop started reads");
    assert_eq!(started.event_seq, 1);
    assert_eq!(started.snapshot_version, 1);
    assert_eq!(started.snapshot.model, model_config("turn-model"));
    assert_eq!(started.snapshot.tool_set_fingerprint, fingerprint('a'));
    let base = started
        .event_seq
        .checked_sub(1)
        .expect("loop_started sequence is positive");

    // Unknown turn.
    let error = backend
        .read_loop_started(agent, TurnId::new())
        .await
        .expect_err("unknown turn");
    assert!(matches!(error, PostgresError::TurnNotFound { .. }));

    let above_bigint = EVENT_SEQ_MAX
        .checked_add(1)
        .expect("u64 has room above bigint max");
    let error = backend
        .read_events_range(agent, above_bigint, above_bigint)
        .await
        .expect_err("even an empty out-of-domain range is rejected");
    assert!(matches!(error, PostgresError::InvalidCommand(_)));

    // The exact slice verifies continuity and identity.
    let through = backend
        .read_agent_state(agent)
        .await
        .expect("state reads")
        .last_event_seq;
    let slice = backend
        .read_resume_slice(ResumeSliceQuery {
            agent_id: agent,
            session_id: session,
            turn_id: turn,
            base_event_seq: base,
            through_event_seq: through,
        })
        .await
        .expect("slice reads");
    assert_eq!(slice.len(), 2);
    assert!(matches!(
        slice.first().map(|row| &row.event),
        Some(DurableAgentEvent::LoopStarted { .. })
    ));

    let error = backend
        .read_resume_slice(ResumeSliceQuery {
            agent_id: agent,
            session_id: SessionId::new(),
            turn_id: turn,
            base_event_seq: base,
            through_event_seq: through,
        })
        .await
        .expect_err("foreign session fails closed");
    assert!(matches!(error, PostgresError::DurableStateCorrupt { .. }));

    let error = backend
        .read_resume_slice(ResumeSliceQuery {
            agent_id: agent,
            session_id: session,
            turn_id: turn,
            base_event_seq: base,
            through_event_seq: through.checked_add(1).expect("test sequence has room"),
        })
        .await
        .expect_err("missing rows fail closed");
    assert!(matches!(error, PostgresError::DurableStateCorrupt { .. }));

    // Full-replay fallback reads from the ledger start.
    let replay = backend
        .read_events_range(agent, 0, base)
        .await
        .expect("replay range reads");
    assert!(replay.is_empty(), "nothing precedes the first turn");

    // Actual tail corruption is also rejected: the high-water still points to
    // `through`, but the final committed row is gone.
    sqlx::query("DELETE FROM durable_events WHERE agent_id = $1 AND event_seq = $2")
        .bind(agent.as_uuid())
        .bind(i64::try_from(through).expect("test sequence fits bigint"))
        .execute(&raw_pool().await)
        .await
        .expect("tail row deletes");
    let error = backend
        .read_resume_slice(ResumeSliceQuery {
            agent_id: agent,
            session_id: session,
            turn_id: turn,
            base_event_seq: base,
            through_event_seq: through,
        })
        .await
        .expect_err("deleted tail fails closed");
    assert!(matches!(error, PostgresError::DurableStateCorrupt { .. }));
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn agent_view_derives_barrier_usage_and_default_model_updates() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;

    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.status, stratum_postgres::AgentStatus::Idle);
    assert_eq!(view.snapshot_event_seq, 0);
    assert_eq!(view.telemetry_floor_event_seq, 0);
    assert!(view.pending_approvals.is_empty());
    assert_eq!(view.latest_usage, None);
    assert_eq!(view.source_template_name, "alpha");

    let (session, turn) = running_turn(&backend, agent).await;

    // The first user message commits the effective model replacement.
    let mut command = append_command(
        agent,
        session,
        turn,
        DurableAgentEvent::MessageAppended {
            message: ChatMessage::user("hello"),
        },
    );
    command.default_model_update = Some(model_config("upgraded-model"));
    backend
        .append_event(command)
        .await
        .expect("first message commits");
    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.default_model_config, model_config("upgraded-model"));

    // An identical replacement is a no-op.
    let mut command = append_command(
        agent,
        session,
        turn,
        DurableAgentEvent::MessageAppended {
            message: ChatMessage::user("again"),
        },
    );
    command.default_model_update = Some(model_config("upgraded-model"));
    backend.append_event(command).await.expect("no-op commits");
    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.default_model_config, model_config("upgraded-model"));

    // Usage derives from the latest usage-carrying event within the barrier.
    backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::IterationCompleted {
                iteration: 0,
                usage: usage(10),
            },
        ))
        .await
        .expect("iteration commits");
    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.latest_usage, Some(usage(10)));

    backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage: usage(20),
            },
        ))
        .await
        .expect("terminal commits");
    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.status, stratum_postgres::AgentStatus::Finished);
    assert_eq!(view.latest_usage, Some(usage(20)));
    assert_eq!(view.snapshot_event_seq, 5);
    assert_eq!(view.session_id, Some(session));
    assert_eq!(view.current_turn_id, Some(turn));
    // A finished turn admits the next one with the exact recent-turn CAS.
    let (command, _) = begin_command(agent, Some(turn), session);
    backend.begin_turn(command).await.expect("next turn admits");
    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.status, stratum_postgres::AgentStatus::Running);
    assert_eq!(view.latest_usage, None, "new turn has no usage yet");
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn agent_view_derives_telemetry_floor_beyond_the_latest_history_page() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "telemetry-floor").await;
    let (session, turn) = running_turn(&backend, agent).await;

    let assistant_seq = backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::assistant("durable final"),
            },
        ))
        .await
        .expect("assistant final appends")
        .event_seq;
    assert_eq!(assistant_seq, 2);

    let mut newest_seq = assistant_seq;
    for index in 0..55 {
        newest_seq = append_message(
            &backend,
            agent,
            session,
            turn,
            &format!("later user message {index}"),
        )
        .await;
    }

    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.snapshot_event_seq, newest_seq);
    assert_eq!(view.telemetry_floor_event_seq, assistant_seq);

    let latest_page = backend
        .read_history_page(HistoryQuery {
            agent_id: agent,
            through_event_seq: view.snapshot_event_seq,
            before_event_seq: None,
            limit: 50,
        })
        .await
        .expect("latest history page reads");
    assert!(latest_page.has_more);
    assert!(
        latest_page
            .items
            .iter()
            .all(|item| item.event_seq != assistant_seq),
        "the AgentView floor must not depend on the latest page containing the final"
    );

    set_event_version(agent, newest_seq, 2).await;
    let error = backend
        .read_agent_view(agent)
        .await
        .expect_err("a newer unsupported message cannot be skipped");
    assert_incompatible(error, 2);

    set_event_version(agent, newest_seq, 1).await;
    set_event_payload(agent, newest_seq, json!({ "message": { "role": "user" } })).await;
    let error = backend
        .read_agent_view(agent)
        .await
        .expect_err("a newer malformed message cannot be skipped");
    assert_corrupt(error);
}

/// Flips one row's `event_version` in place, simulating a row written by a
/// newer binary; the payload keeps the v1 shape so only the version gate can
/// reject it.
async fn set_event_version(agent_id: AgentId, event_seq: u64, version: i32) {
    let event_seq = i64::try_from(event_seq).expect("test event sequence fits bigint");
    sqlx::query(
        "UPDATE durable_events SET event_version = $3 WHERE agent_id = $1 AND event_seq = $2",
    )
    .bind(agent_id.as_uuid())
    .bind(event_seq)
    .bind(version)
    .execute(&raw_pool().await)
    .await
    .expect("event version flips");
}

async fn set_event_payload(agent_id: AgentId, event_seq: u64, payload: Value) {
    let event_seq = i64::try_from(event_seq).expect("test event sequence fits bigint");
    sqlx::query("UPDATE durable_events SET payload = $3 WHERE agent_id = $1 AND event_seq = $2")
        .bind(agent_id.as_uuid())
        .bind(event_seq)
        .bind(payload)
        .execute(&raw_pool().await)
        .await
        .expect("event payload changes");
}

async fn insert_illegal_companion(
    agent_id: AgentId,
    event_seq: u64,
    turn_id: TurnId,
    retained_from_event_seq: u64,
) {
    let summary = serde_json::to_value(ChatMessage::system(
        "[stratum:transcript-compacted]\nmalformed companion",
    ))
    .expect("summary encodes");
    sqlx::query(
        "INSERT INTO transcript_compactions \
         (agent_id, event_seq, turn_id, compacted_iteration, upto, \
          retained_from_event_seq, summary) \
         VALUES ($1, $2, $3, 1, 1, $4, $5)",
    )
    .bind(agent_id.as_uuid())
    .bind(i64::try_from(event_seq).expect("test event sequence fits bigint"))
    .bind(turn_id.as_uuid())
    .bind(i64::try_from(retained_from_event_seq).expect("test retained sequence fits bigint"))
    .bind(summary)
    .execute(&raw_pool().await)
    .await
    .expect("malformed companion relation inserts");
}

fn assert_incompatible(error: PostgresError, version: i32) {
    assert!(
        matches!(
            error,
            PostgresError::RuntimeIncompatible {
                kind: VersionedKind::EventPayload,
                version: found,
            } if found == version
        ),
        "expected runtime_incompatible for version {version}, got {error:?}"
    );
}

fn assert_corrupt(error: PostgresError) {
    assert!(
        matches!(error, PostgresError::DurableStateCorrupt { .. }),
        "expected durable_state_corrupt, got {error:?}"
    );
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn latest_usage_distinguishes_unsupported_version_from_corrupt_v1() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;
    let usage_seq = backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::IterationCompleted {
                iteration: 0,
                usage: usage(10),
            },
        ))
        .await
        .expect("usage event appends")
        .event_seq;

    set_event_version(agent, usage_seq, 2).await;
    let error = backend
        .read_agent_view(agent)
        .await
        .expect_err("unsupported usage version fails closed");
    assert_incompatible(error, 2);

    set_event_version(agent, usage_seq, 1).await;
    set_event_payload(agent, usage_seq, json!({ "iteration": 0 })).await;
    let error = backend
        .read_agent_view(agent)
        .await
        .expect_err("malformed v1 usage fails closed");
    assert_corrupt(error);
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn approval_and_hook_derivation_fail_closed_on_unsupported_event_versions() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;

    let approval = ApprovalId::new();
    let hook = HookInvocationId::new();
    let mut command = append_command(agent, session, turn, approval_request(approval));
    command.approval_hook_invocation_id = Some(hook);
    let requested_seq = backend
        .append_event(command)
        .await
        .expect("request appends")
        .event_seq;
    let view = backend.read_agent_view(agent).await.expect("view reads");
    assert_eq!(view.pending_approvals.len(), 1);

    // A same-shaped request row at an unsupported version is never decoded as
    // v1: every derivation that reads its payload fails closed.
    set_event_version(agent, requested_seq, 2).await;
    let error = backend
        .read_approval(agent, turn, ApprovalLookup::ByApprovalId(approval))
        .await
        .expect_err("v2 request is incompatible");
    assert_incompatible(error, 2);
    let error = backend
        .read_agent_view(agent)
        .await
        .expect_err("v2 pending derivation is incompatible");
    assert_incompatible(error, 2);
    let error = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("v2 request cannot resolve");
    assert_incompatible(error, 2);
    set_event_version(agent, requested_seq, 1).await;

    // The decision commits at v1.
    let ResolveApprovalOutcome::Resolved { receipt } = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval,
            ApprovalDecision::Approve,
        ))
        .await
        .expect("resolve commits")
    else {
        panic!("first resolve must commit");
    };

    // An unsupported-version decision row is explicit at every derivation,
    // including the pending view's existence-based exclusion.
    set_event_version(agent, receipt.event_seq, 2).await;
    let error = backend
        .read_approval(agent, turn, ApprovalLookup::ByApprovalId(approval))
        .await
        .expect_err("v2 resolution is incompatible");
    assert_incompatible(error, 2);
    let error = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("v2 resolution cannot be re-decoded");
    assert_incompatible(error, 2);
    let error = backend
        .read_agent_view(agent)
        .await
        .expect_err("v2 resolution cannot silently hide a pending approval");
    assert_incompatible(error, 2);
    set_event_version(agent, receipt.event_seq, 1).await;

    // A v1 pending hook invocation is open at its exact address.
    let invocation = HookInvocationId::new();
    let pending_seq = backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::HookInvocationPending {
                invocation_id: invocation,
                point: HookPoint::DecideToolCall,
                iteration: 0,
                call_id: Some(CallId::from("call-1")),
                input_digest: "b".repeat(64).parse::<HookInputDigest>().expect("digest"),
            },
        ))
        .await
        .expect("pending appends")
        .event_seq;
    let lookup = HookInvocationLookup {
        agent_id: agent,
        turn_id: turn,
        point: HookPoint::DecideToolCall,
        iteration: 0,
        call_id: Some(CallId::from("call-1")),
    };
    let open = backend
        .read_open_hook_invocation(lookup.clone())
        .await
        .expect("lookup reads");
    assert_eq!(open, Some(invocation));

    // An unsupported-version pending row is never read as v1.
    set_event_version(agent, pending_seq, 2).await;
    let error = backend
        .read_open_hook_invocation(lookup.clone())
        .await
        .expect_err("v2 pending invocation is incompatible");
    assert_incompatible(error, 2);
    set_event_version(agent, pending_seq, 1).await;

    // A completion at an unsupported version cannot silently consume the
    // pending invocation through NOT EXISTS.
    let completed_seq = backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::HookInvocationCompleted {
                invocation_id: invocation,
                decision: HookDecisionRecord::DecideToolCall(DecideToolCallDecisionRecord::Execute),
            },
        ))
        .await
        .expect("completion appends")
        .event_seq;
    set_event_version(agent, completed_seq, 2).await;
    let error = backend
        .read_open_hook_invocation(lookup)
        .await
        .expect_err("v2 completion cannot silently consume an invocation");
    assert_incompatible(error, 2);
}

#[tokio::test]
#[ignore = "requires the compose Postgres stack"]
async fn approval_and_open_hook_derivations_reject_malformed_v1_payloads() {
    let backend = reset_backend().await;
    let agent = create_agent(&backend, "alpha").await;
    let (session, turn) = running_turn(&backend, agent).await;

    let approval = ApprovalId::new();
    let hook = HookInvocationId::new();
    let mut command = append_command(agent, session, turn, approval_request(approval));
    command.approval_hook_invocation_id = Some(hook);
    let request_seq = backend
        .append_event(command)
        .await
        .expect("approval request appends")
        .event_seq;
    set_event_payload(
        agent,
        request_seq,
        json!({
            "approval_id": approval,
            "hook_invocation_id": hook,
            "call_id": "call-1",
            "tool_name": "echo"
        }),
    )
    .await;
    let error = backend
        .read_approval(agent, turn, ApprovalLookup::ByApprovalId(approval))
        .await
        .expect_err("malformed request lookup fails closed");
    assert_corrupt(error);
    let error = backend
        .resolve_approval(resolve_command(
            agent,
            turn,
            approval,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("malformed request cannot resolve");
    assert_corrupt(error);
    let error = backend
        .read_agent_view(agent)
        .await
        .expect_err("malformed pending request fails closed");
    assert_corrupt(error);

    let invocation = HookInvocationId::new();
    let pending_seq = backend
        .append_event(append_command(
            agent,
            session,
            turn,
            DurableAgentEvent::HookInvocationPending {
                invocation_id: invocation,
                point: HookPoint::DecideToolCall,
                iteration: 0,
                call_id: Some(CallId::from("call-2")),
                input_digest: "b".repeat(64).parse::<HookInputDigest>().expect("digest"),
            },
        ))
        .await
        .expect("pending hook appends")
        .event_seq;
    set_event_payload(
        agent,
        pending_seq,
        json!({
            "invocation_id": invocation,
            "point": "decide_tool_call",
            "iteration": 0,
            "call_id": "call-2",
            "input_digest": "not-a-digest"
        }),
    )
    .await;
    let error = backend
        .read_open_hook_invocation(HookInvocationLookup {
            agent_id: agent,
            turn_id: turn,
            point: HookPoint::DecideToolCall,
            iteration: 0,
            call_id: Some(CallId::from("call-2")),
        })
        .await
        .expect_err("malformed pending hook fails closed");
    assert_corrupt(error);

    let resolved_agent = create_agent(&backend, "malformed-resolution").await;
    let (resolved_session, resolved_turn) = running_turn(&backend, resolved_agent).await;
    let resolved_approval = ApprovalId::new();
    let mut request = append_command(
        resolved_agent,
        resolved_session,
        resolved_turn,
        approval_request(resolved_approval),
    );
    request.approval_hook_invocation_id = Some(HookInvocationId::new());
    backend
        .append_event(request)
        .await
        .expect("approval request appends");
    let ResolveApprovalOutcome::Resolved { receipt } = backend
        .resolve_approval(resolve_command(
            resolved_agent,
            resolved_turn,
            resolved_approval,
            ApprovalDecision::Approve,
        ))
        .await
        .expect("resolution appends")
    else {
        panic!("first resolution must append");
    };
    set_event_payload(
        resolved_agent,
        receipt.event_seq,
        json!({
            "approval_id": resolved_approval,
            "decision": "not-a-decision"
        }),
    )
    .await;
    let error = backend
        .read_agent_view(resolved_agent)
        .await
        .expect_err("malformed resolution cannot silently exclude a request");
    assert_corrupt(error);
    let error = backend
        .resolve_approval(resolve_command(
            resolved_agent,
            resolved_turn,
            resolved_approval,
            ApprovalDecision::Approve,
        ))
        .await
        .expect_err("malformed resolution cannot be treated as idempotent");
    assert_corrupt(error);

    let completed_agent = create_agent(&backend, "malformed-completion").await;
    let (completed_session, completed_turn) = running_turn(&backend, completed_agent).await;
    let completed_invocation = HookInvocationId::new();
    backend
        .append_event(append_command(
            completed_agent,
            completed_session,
            completed_turn,
            DurableAgentEvent::HookInvocationPending {
                invocation_id: completed_invocation,
                point: HookPoint::DecideToolCall,
                iteration: 0,
                call_id: Some(CallId::from("call-1")),
                input_digest: "c".repeat(64).parse::<HookInputDigest>().expect("digest"),
            },
        ))
        .await
        .expect("pending hook appends");
    let completed_approval = ApprovalId::new();
    let mut request = append_command(
        completed_agent,
        completed_session,
        completed_turn,
        approval_request(completed_approval),
    );
    request.approval_hook_invocation_id = Some(completed_invocation);
    backend
        .append_event(request)
        .await
        .expect("approval request appends");
    let completed_seq = backend
        .append_event(append_command(
            completed_agent,
            completed_session,
            completed_turn,
            DurableAgentEvent::HookInvocationCompleted {
                invocation_id: completed_invocation,
                decision: HookDecisionRecord::DecideToolCall(DecideToolCallDecisionRecord::Execute),
            },
        ))
        .await
        .expect("hook completion appends")
        .event_seq;
    set_event_payload(
        completed_agent,
        completed_seq,
        json!({ "invocation_id": completed_invocation }),
    )
    .await;
    let error = backend
        .read_agent_view(completed_agent)
        .await
        .expect_err("malformed completion cannot silently consume a request");
    assert_corrupt(error);
    let error = backend
        .read_open_hook_invocation(HookInvocationLookup {
            agent_id: completed_agent,
            turn_id: completed_turn,
            point: HookPoint::DecideToolCall,
            iteration: 0,
            call_id: Some(CallId::from("call-1")),
        })
        .await
        .expect_err("malformed completion cannot silently close a hook");
    assert_corrupt(error);
}
