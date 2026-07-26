mod support;

use std::sync::Arc;

use stratum_core::{
    AgentEvent, AgentId, AgentRuntimeContext, AgentVersionId, ChatMessage, ChatRole, HistoryQuery,
    ModelConfig, ModelId, NewAgentMessage, NodeId, RuntimeEvent, SessionId, StreamEnvelope,
    TokenUsage, ToolSetFingerprint, TurnId, TurnRuntimeSnapshot, WorkflowVersionId,
};
use stratum_filesystem::{Entry, FILESYSTEM_CAS_RETRIES, VirtualPath};
use stratum_store::{
    AGENT_STATE_VERSION, AgentState, AgentStatus, AgentStore, FilesystemAgentStore, StoreError,
};
use support::MemoryCasFilesystem;

fn test_model_config() -> ModelConfig {
    ModelConfig::new(
        ModelId::new("openai", "test-model").expect("static model is valid"),
        serde_json::Map::new(),
    )
}

fn legacy_agent_state_without_model_config(agent_id: AgentId) -> serde_json::Value {
    let mut state = serde_json::to_value(AgentState::new(agent_id, "a".to_owned()))
        .expect("serialize legacy state");
    let state = state.as_object_mut().expect("state object");
    state.insert(
        "state_version".to_owned(),
        serde_json::Value::from(AGENT_STATE_VERSION - 1),
    );
    state.remove("model_config");
    serde_json::Value::Object(state.clone())
}

fn message_envelope(agent_id: AgentId, session_id: SessionId, turn_id: TurnId) -> NewAgentMessage {
    NewAgentMessage::new(
        &AgentRuntimeContext::direct(session_id),
        agent_id,
        turn_id,
        ChatMessage::user("message"),
    )
}

fn sequenced_message_envelope(
    agent_id: AgentId,
    session_id: SessionId,
    turn_id: TurnId,
    seq: u64,
) -> StreamEnvelope {
    message_envelope(agent_id, session_id, turn_id).into_envelope(seq)
}

fn test_runtime_snapshot(state: &AgentState) -> TurnRuntimeSnapshot {
    TurnRuntimeSnapshot::new(
        state.agent_version_id,
        test_model_config(),
        "0000000000000000000000000000000000000000000000000000000000000000"
            .parse::<ToolSetFingerprint>()
            .expect("test fingerprint parses"),
        state.skill_set_version_id,
        state.extension_set_version_id,
        state.hook_handler_versions.clone(),
    )
}

fn json_entry<T: serde::Serialize>(value: &T) -> Entry {
    Entry::new(serde_json::to_vec(value).expect("serialize fixture entry"))
}

fn event_sequences(events: &[StreamEnvelope]) -> Vec<u64> {
    events
        .iter()
        .map(|event| event.message_seq().expect("message sequence"))
        .collect()
}

async fn append_messages(store: &FilesystemAgentStore, count: usize) {
    let state = store.load_agent().await.expect("load agent");
    let agent_id = state.agent_id;
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    for index in 0..count {
        let appended = store
            .append_message(message_envelope(agent_id, session_id, turn_id))
            .await
            .expect("append message");
        let expected_seq =
            state.last_seq + u64::try_from(index).expect("message index fits u64") + 1;
        assert_eq!(appended.message_seq(), Some(expected_seq));
    }
}

#[tokio::test]
async fn initialize_and_append_create_exact_files_and_advance_last_seq() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();

    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let first = store
        .append_message(message_envelope(agent_id, SessionId::new(), TurnId::new()))
        .await
        .expect("append");

    assert_eq!(first.message_seq(), Some(1));
    assert!(filesystem.exists("/agents/a/agent.json"));
    assert!(filesystem.exists("/agents/a/messages/1.json"));
    let stored: StreamEnvelope = serde_json::from_slice(
        filesystem
            .entry("/agents/a/messages/1.json")
            .expect("stored message")
            .contents(),
    )
    .expect("decode stored message");
    assert_eq!(stored.message_seq(), Some(1));
    assert_eq!(stored, first);
    assert_eq!(store.load_agent().await.expect("state").last_seq, 1);
}

#[tokio::test]
async fn unsupported_beta_state_is_rejected_without_migration() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let legacy = legacy_agent_state_without_model_config(agent_id);
    filesystem.insert_entry("/agents/a/agent.json", json_entry(&legacy));

    let error = store
        .load_agent()
        .await
        .expect_err("old beta state must be rejected");

    assert!(matches!(
        error,
        StoreError::UnsupportedStateVersion { version }
            if version == AGENT_STATE_VERSION - 1
    ));
}

#[tokio::test]
async fn start_turn_reconciles_previous_frontier_before_changing_identity() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let old_session_id = SessionId::new();
    let old_turn_id = TurnId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize configured state");
    store
        .update_state(
            AgentStatus::Running,
            Some(old_session_id),
            Some(old_turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("store old identity");
    store
        .update_state(
            AgentStatus::Finished,
            Some(old_session_id),
            Some(old_turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("finish old identity");
    let old_frontier = sequenced_message_envelope(agent_id, old_session_id, old_turn_id, 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&old_frontier));

    let new_session_id = SessionId::new();
    let new_turn_id = TurnId::new();
    let current = store.load_agent().await.expect("load current state");
    let started = store
        .start_turn(
            &AgentRuntimeContext::direct(new_session_id),
            new_turn_id,
            test_runtime_snapshot(&current),
        )
        .await
        .expect("start turn after frontier reconciliation");

    assert_eq!(started.last_seq, 1);
    assert_eq!(started.session_id, Some(new_session_id));
    assert_eq!(started.turn_id, Some(new_turn_id));
    let page = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: Some(1),
            limit: 1,
        })
        .await
        .expect("old frontier remains recoverable");
    assert_eq!(page.events, [old_frontier]);
}

#[tokio::test]
async fn workflow_location_and_runtime_snapshot_survive_store_restart() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root.clone());
    let agent_id = AgentId::new();
    let state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let context = AgentRuntimeContext::workflow_node(
        session_id,
        WorkflowVersionId::new(),
        NodeId::from("agent-node"),
    );
    let snapshot = test_runtime_snapshot(&state);

    store
        .start_turn(&context, turn_id, snapshot.clone())
        .await
        .expect("start turn");
    let restarted = FilesystemAgentStore::new(filesystem, root);
    let restored = restarted.load_agent().await.expect("load after restart");

    assert_eq!(restored.session_id, Some(session_id));
    assert_eq!(restored.turn_id, Some(turn_id));
    assert_eq!(restored.location, Some(context.location));
    assert_eq!(restored.turn_runtime_snapshot, Some(snapshot));
}

#[tokio::test]
async fn start_turn_fails_closed_on_unavailable_pinned_component() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let mut snapshot = test_runtime_snapshot(&state);
    snapshot.agent_version_id = AgentVersionId::new();
    let before = filesystem
        .entry("/agents/a/agent.json")
        .expect("agent state")
        .contents()
        .to_vec();

    let error = store
        .start_turn(
            &AgentRuntimeContext::direct(SessionId::new()),
            TurnId::new(),
            snapshot,
        )
        .await
        .expect_err("mismatched component must fail closed");

    assert!(matches!(
        error,
        StoreError::RuntimeSnapshotMismatch {
            component: "agent_version"
        }
    ));
    assert_eq!(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent state")
            .contents(),
        before
    );
}

#[tokio::test]
async fn complete_iteration_advances_and_persists_usage() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem, root);
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let usage = TokenUsage {
        input_tokens: 2,
        output_tokens: 3,
        total_tokens: 5,
    };
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    store
        .update_state(
            AgentStatus::Running,
            Some(session_id),
            Some(turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("start run");

    let completed = store
        .complete_iteration(session_id, turn_id, 0, usage)
        .await
        .expect("complete iteration");

    assert_eq!(completed.next_iteration, 1);
    assert_eq!(completed.usage, usage);
    let persisted = store.load_agent().await.expect("load persisted state");
    assert_eq!(persisted.next_iteration, 1);
    assert_eq!(persisted.usage, usage);
}

#[tokio::test]
async fn running_state_rejects_a_different_run_without_writing() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    store
        .update_state(
            AgentStatus::Running,
            Some(session_id),
            Some(turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("start run");
    let before = filesystem
        .entry("/agents/a/agent.json")
        .expect("agent state")
        .contents()
        .to_vec();

    let error = store
        .update_state(
            AgentStatus::Running,
            Some(SessionId::new()),
            Some(TurnId::new()),
            TokenUsage::default(),
        )
        .await
        .expect_err("a second run must not replace the persisted running turn");

    assert!(matches!(error, StoreError::RunningSessionConflict { .. }));
    assert_eq!(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent state")
            .contents(),
        before
    );
}

#[tokio::test]
async fn complete_iteration_rejects_non_running_state_without_writing() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let before = filesystem
        .entry("/agents/a/agent.json")
        .expect("agent state")
        .contents()
        .to_vec();

    let error = store
        .complete_iteration(SessionId::new(), TurnId::new(), 0, TokenUsage::default())
        .await
        .expect_err("idle state is not running");

    assert!(matches!(
        error,
        StoreError::AgentNotRunning {
            actual: AgentStatus::Idle
        }
    ));
    assert_eq!(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent state")
            .contents(),
        before
    );
}

#[tokio::test]
async fn complete_iteration_rejects_session_mismatch_without_writing() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let other_session_id = SessionId::new();
    let turn_id = TurnId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    store
        .update_state(
            AgentStatus::Running,
            Some(session_id),
            Some(turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("start operation");
    let before = filesystem
        .entry("/agents/a/agent.json")
        .expect("agent state")
        .contents()
        .to_vec();

    let error = store
        .complete_iteration(other_session_id, turn_id, 0, TokenUsage::default())
        .await
        .expect_err("session mismatch");

    assert!(matches!(
        error,
        StoreError::SessionMismatch { expected, actual }
            if expected == session_id && actual == other_session_id
    ));
    assert_eq!(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent state")
            .contents(),
        before
    );
}

#[tokio::test]
async fn complete_iteration_rejects_turn_mismatch_without_writing() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let other_turn_id = TurnId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    store
        .update_state(
            AgentStatus::Running,
            Some(session_id),
            Some(turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("start run");
    let before = filesystem
        .entry("/agents/a/agent.json")
        .expect("agent state")
        .contents()
        .to_vec();

    let error = store
        .complete_iteration(session_id, other_turn_id, 0, TokenUsage::default())
        .await
        .expect_err("turn mismatch");

    assert!(matches!(
        error,
        StoreError::TurnMismatch { expected, actual }
            if expected == turn_id && actual == other_turn_id
    ));
    assert_eq!(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent state")
            .contents(),
        before
    );
}

#[tokio::test]
async fn complete_iteration_rejects_iteration_mismatch_without_writing() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    store
        .update_state(
            AgentStatus::Running,
            Some(session_id),
            Some(turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("start run");
    let before = filesystem
        .entry("/agents/a/agent.json")
        .expect("agent state")
        .contents()
        .to_vec();

    let error = store
        .complete_iteration(session_id, turn_id, 1, TokenUsage::default())
        .await
        .expect_err("iteration mismatch");

    assert!(matches!(
        error,
        StoreError::IterationMismatch {
            expected: 0,
            actual: 1
        }
    ));
    assert_eq!(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent state")
            .contents(),
        before
    );
}

#[tokio::test]
async fn complete_iteration_rejects_iteration_overflow_without_writing() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let mut state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    state.status = AgentStatus::Running;
    state.session_id = Some(session_id);
    state.turn_id = Some(turn_id);
    state.next_iteration = u64::MAX;
    filesystem.insert_entry("/agents/a/agent.json", json_entry(&state));
    let before = filesystem
        .entry("/agents/a/agent.json")
        .expect("agent state")
        .contents()
        .to_vec();

    let error = store
        .complete_iteration(session_id, turn_id, u64::MAX, TokenUsage::default())
        .await
        .expect_err("iteration overflow");

    assert!(matches!(error, StoreError::IterationOverflow));
    assert_eq!(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent state")
            .contents(),
        before
    );
}

#[tokio::test]
async fn start_turn_resets_iteration_and_terminal_updates_preserve_it() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem, root);
    let agent_id = AgentId::new();
    let old_session_id = SessionId::new();
    let old_turn_id = TurnId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    store
        .update_state(
            AgentStatus::Running,
            Some(old_session_id),
            Some(old_turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("start old run");
    store
        .complete_iteration(old_session_id, old_turn_id, 0, TokenUsage::default())
        .await
        .expect("complete old iteration");
    store
        .update_state(
            AgentStatus::Finished,
            Some(old_session_id),
            Some(old_turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("finish old run");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let current = store.load_agent().await.expect("load current state");

    let started = store
        .start_turn(
            &AgentRuntimeContext::direct(session_id),
            turn_id,
            test_runtime_snapshot(&current),
        )
        .await
        .expect("start new turn");
    assert_eq!(started.next_iteration, 0);
    store
        .complete_iteration(session_id, turn_id, 0, TokenUsage::default())
        .await
        .expect("complete new iteration");

    for status in [
        AgentStatus::Finished,
        AgentStatus::Failed,
        AgentStatus::Cancelled,
    ] {
        let terminal = store
            .update_state(
                status,
                Some(session_id),
                Some(turn_id),
                TokenUsage::default(),
            )
            .await
            .expect("store terminal state");
        assert_eq!(terminal.next_iteration, 1);
    }
}

#[tokio::test]
async fn append_rejects_a_system_message_before_writing() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let mut envelope = message_envelope(agent_id, SessionId::new(), TurnId::new());
    envelope.message = ChatMessage::system("system prompt");

    let error = store
        .append_message(envelope)
        .await
        .expect_err("system message role");

    assert!(matches!(
        error,
        StoreError::InvalidMessageRole {
            role: ChatRole::System
        }
    ));
    assert_eq!(store.load_agent().await.expect("state").last_seq, 0);
    assert!(!filesystem.exists("/agents/a/messages/1.json"));
}

#[tokio::test]
async fn load_rejects_a_committed_system_message() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let mut state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    state.last_seq = 1;
    filesystem.insert_entry("/agents/a/agent.json", json_entry(&state));
    let mut envelope = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    let RuntimeEvent::Agent {
        event: AgentEvent::Message { message, .. },
        ..
    } = &mut envelope.event
    else {
        panic!("message fixture");
    };
    *message = ChatMessage::system("system prompt");
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&envelope));

    let error = store
        .load_agent()
        .await
        .expect_err("committed system message role");

    assert!(matches!(
        error,
        StoreError::InvalidMessageRole {
            role: ChatRole::System
        }
    ));
}

#[tokio::test]
async fn load_rejects_an_uncommitted_system_frontier_without_advancing_state() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let mut envelope = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    let RuntimeEvent::Agent {
        event: AgentEvent::Message { message, .. },
        ..
    } = &mut envelope.event
    else {
        panic!("message fixture");
    };
    *message = ChatMessage::system("system prompt");
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&envelope));

    let error = store
        .load_agent()
        .await
        .expect_err("uncommitted system frontier role");

    assert!(matches!(
        error,
        StoreError::InvalidMessageRole {
            role: ChatRole::System
        }
    ));
    let persisted: AgentState = serde_json::from_slice(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent entry")
            .contents(),
    )
    .expect("agent state");
    assert_eq!(persisted.last_seq, 0);
}

#[tokio::test]
async fn append_rejects_a_message_for_a_different_agent() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let other_agent_id = AgentId::new();

    let error = store
        .append_message(message_envelope(
            other_agent_id,
            SessionId::new(),
            TurnId::new(),
        ))
        .await
        .expect_err("agent mismatch");

    assert!(matches!(
        error,
        StoreError::AgentMismatch { expected, actual }
            if expected == agent_id && actual == other_agent_id
    ));
    assert_eq!(store.load_agent().await.expect("state").last_seq, 0);
    assert!(!filesystem.exists("/agents/a/messages/1.json"));
}

#[tokio::test]
async fn load_reconciles_one_valid_frontier_without_rewriting_it() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    assert_eq!(state.last_seq, 0);
    let envelope = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&envelope));
    let message_version = filesystem
        .entry_version("/agents/a/messages/1.json")
        .expect("message version");

    let reconciled = store.load_agent().await.expect("reconcile frontier");

    assert_eq!(reconciled.last_seq, 1);
    assert_eq!(
        filesystem.entry_version("/agents/a/messages/1.json"),
        Some(message_version)
    );
}

#[tokio::test]
async fn load_rejects_a_discontiguous_second_message_without_a_frontier() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let second = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 2);
    filesystem.insert_entry("/agents/a/messages/2.json", json_entry(&second));

    let error = store
        .load_agent()
        .await
        .expect_err("discontiguous extra message");

    assert!(matches!(
        error,
        StoreError::MessageBeyondFrontier {
            seq: 2,
            frontier: 1
        }
    ));
}

#[tokio::test]
async fn load_rejects_a_third_message_beyond_the_single_frontier() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let first = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    let third = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 3);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&first));
    filesystem.insert_entry("/agents/a/messages/3.json", json_entry(&third));

    let error = store
        .load_agent()
        .await
        .expect_err("message beyond single frontier");

    assert!(matches!(
        error,
        StoreError::MessageBeyondFrontier {
            seq: 3,
            frontier: 1
        }
    ));
}

#[tokio::test]
async fn load_rejects_noncanonical_message_filenames() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let first = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    filesystem.insert_entry("/agents/a/messages/01.json", json_entry(&first));

    let error = store.load_agent().await.expect_err("noncanonical filename");

    assert!(matches!(
        error,
        StoreError::InvalidMessageFilename { file_name } if file_name == "01.json"
    ));
}

#[tokio::test]
async fn append_retry_returns_an_identical_uncommitted_frontier_without_duplication() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let requested = message_envelope(agent_id, session_id, turn_id);
    let envelope = requested.clone().into_envelope(1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&envelope));
    let message_version = filesystem
        .entry_version("/agents/a/messages/1.json")
        .expect("message version");

    let appended = store.append_message(requested).await.expect("retry append");

    assert_eq!(appended, envelope);
    assert_eq!(store.load_agent().await.expect("state").last_seq, 1);
    assert!(!filesystem.exists("/agents/a/messages/2.json"));
    assert_eq!(
        filesystem.entry_version("/agents/a/messages/1.json"),
        Some(message_version)
    );
}

#[tokio::test]
async fn append_reconciles_a_different_frontier_then_retries_at_the_next_sequence() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let frontier = sequenced_message_envelope(agent_id, session_id, turn_id, 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&frontier));
    let frontier_version = filesystem
        .entry_version("/agents/a/messages/1.json")
        .expect("frontier version");
    let mut requested = message_envelope(agent_id, session_id, turn_id);
    requested
        .metadata
        .insert("request".to_owned(), serde_json::json!(true));

    let appended = store
        .append_message(requested)
        .await
        .expect("append after frontier");

    assert_eq!(appended.message_seq(), Some(2));
    assert_eq!(store.load_agent().await.expect("state").last_seq, 2);
    assert!(filesystem.exists("/agents/a/messages/2.json"));
    assert_eq!(
        filesystem.entry_version("/agents/a/messages/1.json"),
        Some(frontier_version)
    );
}

#[tokio::test]
async fn append_rejects_an_uncommitted_frontier_from_a_different_session() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let frontier_session_id = SessionId::new();
    let turn_id = TurnId::new();
    let frontier = sequenced_message_envelope(agent_id, frontier_session_id, turn_id, 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&frontier));
    let requested_session_id = SessionId::new();

    let error = store
        .append_message(message_envelope(agent_id, requested_session_id, turn_id))
        .await
        .expect_err("session mismatch");

    assert!(matches!(
        error,
        StoreError::SessionMismatch { expected, actual }
            if expected == requested_session_id && actual == frontier_session_id
    ));
    assert_eq!(store.load_agent().await.expect("reconcile").last_seq, 1);
}

#[tokio::test]
async fn append_rejects_discontiguous_message_before_advancing_frontier() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let second = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 2);
    filesystem.insert_entry("/agents/a/messages/2.json", json_entry(&second));

    let error = store
        .append_message(message_envelope(agent_id, SessionId::new(), TurnId::new()))
        .await
        .expect_err("discontiguous message");

    assert!(matches!(
        error,
        StoreError::MessageBeyondFrontier {
            seq: 2,
            frontier: 1
        }
    ));
    let state: AgentState = serde_json::from_slice(
        filesystem
            .entry("/agents/a/agent.json")
            .expect("agent entry")
            .contents(),
    )
    .expect("agent state");
    assert_eq!(state.last_seq, 0);
    assert!(!filesystem.exists("/agents/a/messages/1.json"));
}

#[tokio::test]
async fn load_rejects_missing_committed_message() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let mut state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    state.last_seq = 2;
    filesystem.insert_entry("/agents/a/agent.json", json_entry(&state));
    let first = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&first));
    filesystem.remove_entry("/agents/a/messages/2.json");

    let error = store.load_agent().await.expect_err("missing message");

    assert!(matches!(
        error,
        StoreError::MissingCommittedMessage { seq: 2 }
    ));
}

#[tokio::test]
async fn load_rejects_message_filename_body_sequence_mismatch() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let mut state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    state.last_seq = 2;
    filesystem.insert_entry("/agents/a/agent.json", json_entry(&state));
    let first = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    let mismatched = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 3);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&first));
    filesystem.insert_entry("/agents/a/messages/2.json", json_entry(&mismatched));

    let error = store.load_agent().await.expect_err("sequence mismatch");

    assert!(matches!(
        error,
        StoreError::MessageSequenceMismatch {
            path_seq: 2,
            event_seq: 3
        }
    ));
}

#[tokio::test]
async fn load_rejects_message_for_a_different_agent() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let other_agent_id = AgentId::new();
    let frontier = sequenced_message_envelope(other_agent_id, SessionId::new(), TurnId::new(), 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&frontier));

    let error = store.load_agent().await.expect_err("agent mismatch");

    assert!(matches!(
        error,
        StoreError::AgentMismatch { expected, actual }
            if expected == agent_id && actual == other_agent_id
    ));
}

#[tokio::test]
async fn load_rejects_unknown_message_json_fields() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let envelope = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    let mut value = serde_json::to_value(envelope).expect("serialize envelope");
    value
        .as_object_mut()
        .expect("envelope object")
        .insert("owner_id".to_owned(), serde_json::json!("unexpected"));
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&value));

    let error = store.load_agent().await.expect_err("unknown field");

    assert!(matches!(error, StoreError::DecodeMessage(_)));
}

#[tokio::test]
async fn load_rejects_legacy_run_and_source_message_shape() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let envelope = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    let mut value = serde_json::to_value(envelope).expect("serialize envelope");
    value["run_id"] = serde_json::json!(SessionId::new());
    value["source"] = serde_json::json!({"type": "run"});
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&value));

    let error = store.load_agent().await.expect_err("legacy envelope");

    assert!(matches!(error, StoreError::DecodeMessage(_)));
}

#[tokio::test]
async fn state_update_retry_preserves_concurrently_advanced_last_seq() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let mut advanced_state: AgentState = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    advanced_state.last_seq = 1;
    let first = sequenced_message_envelope(agent_id, SessionId::new(), TurnId::new(), 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&first));
    filesystem.fail_next_version_write();

    let update = tokio::spawn({
        let store = store.clone();
        async move {
            store
                .update_state(
                    AgentStatus::Finished,
                    Some(SessionId::new()),
                    Some(TurnId::new()),
                    TokenUsage::default(),
                )
                .await
        }
    });
    while filesystem.version_write_failure_pending() {
        tokio::task::yield_now().await;
    }
    filesystem.insert_entry("/agents/a/agent.json", json_entry(&advanced_state));
    let updated = update
        .await
        .expect("state update task")
        .expect("state update retries");

    assert_eq!(updated.status, AgentStatus::Finished);
    assert_eq!(updated.last_seq, 1);
}

#[tokio::test]
async fn terminal_update_reconciles_the_previous_run_frontier_before_a_new_identity() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let old_session_id = SessionId::new();
    let old_turn_id = TurnId::new();
    store
        .update_state(
            AgentStatus::Running,
            Some(old_session_id),
            Some(old_turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("store old identity");
    let old_frontier = sequenced_message_envelope(agent_id, old_session_id, old_turn_id, 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&old_frontier));
    store
        .update_state(
            AgentStatus::Finished,
            Some(old_session_id),
            Some(old_turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("finish old identity after reconciliation");
    let new_session_id = SessionId::new();
    let new_turn_id = TurnId::new();

    let updated = store
        .update_state(
            AgentStatus::Running,
            Some(new_session_id),
            Some(new_turn_id),
            TokenUsage::default(),
        )
        .await
        .expect("store new identity after reconciliation");

    assert_eq!(updated.last_seq, 1);
    assert_eq!(updated.session_id, Some(new_session_id));
    assert_eq!(updated.turn_id, Some(new_turn_id));
    let page = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: Some(1),
            limit: 1,
        })
        .await
        .expect("old frontier is committed and loadable");
    assert_eq!(page.events, [old_frontier]);
    let appended = store
        .append_message(message_envelope(agent_id, new_session_id, new_turn_id))
        .await
        .expect("append new-run message");
    assert_eq!(appended.message_seq(), Some(2));
}

#[tokio::test]
async fn load_retries_when_frontier_cas_observes_a_later_valid_state() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let mut latest_state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let first = sequenced_message_envelope(agent_id, session_id, turn_id, 1);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&first));
    filesystem.pause_next_version_write();

    let load = tokio::spawn({
        let store = store.clone();
        async move { store.load_agent().await }
    });
    filesystem.wait_for_version_write_pause().await;
    let second = sequenced_message_envelope(agent_id, session_id, turn_id, 2);
    filesystem.insert_entry("/agents/a/messages/2.json", json_entry(&second));
    latest_state.last_seq = 2;
    filesystem.insert_entry("/agents/a/agent.json", json_entry(&latest_state));
    filesystem.resume_version_write();

    let loaded = load
        .await
        .expect("load task")
        .expect("load retries after state advance");
    let page = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: Some(2),
            limit: 2,
        })
        .await
        .expect("latest valid history");

    assert_eq!(loaded.last_seq, 2);
    assert_eq!(event_sequences(&page.events), [1, 2]);
}

#[tokio::test]
async fn pagination_keeps_the_first_page_barrier_after_a_later_append() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem, root);
    store
        .initialize_with_model_config(AgentId::new(), "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    append_messages(&store, 3).await;

    let first = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 2,
        })
        .await
        .expect("first page");
    append_messages(&store, 1).await;
    let second = store
        .history_page(HistoryQuery {
            after_seq: first.next_front_seq,
            through_seq: Some(first.through_seq),
            limit: 2,
        })
        .await
        .expect("second page");

    assert_eq!(first.through_seq, 3);
    assert_eq!(event_sequences(&first.events), [1, 2]);
    assert!(first.has_more);
    assert_eq!(second.through_seq, 3);
    assert_eq!(event_sequences(&second.events), [3]);
    assert!(!second.has_more);
}

#[tokio::test]
async fn pagination_rejects_zero_and_oversized_limits() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem, root);
    store
        .initialize_with_model_config(AgentId::new(), "a".to_owned(), test_model_config())
        .await
        .expect("initialize");

    for limit in [0, 257] {
        let error = store
            .history_page(HistoryQuery {
                after_seq: 0,
                through_seq: None,
                limit,
            })
            .await
            .expect_err("invalid limit");
        assert!(matches!(
            error,
            StoreError::InvalidHistoryLimit { actual, maximum: 256 }
                if actual == limit
        ));
    }
}

#[tokio::test]
async fn pagination_rejects_front_beyond_barrier() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem, root);
    store
        .initialize_with_model_config(AgentId::new(), "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    append_messages(&store, 2).await;

    let error = store
        .history_page(HistoryQuery {
            after_seq: 2,
            through_seq: Some(1),
            limit: 1,
        })
        .await
        .expect_err("front beyond barrier");

    assert!(matches!(
        error,
        StoreError::InvalidHistoryRange {
            after_seq: 2,
            through_seq: 1
        }
    ));
}

#[tokio::test]
async fn pagination_rejects_barrier_beyond_last_committed_sequence() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem, root);
    store
        .initialize_with_model_config(AgentId::new(), "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    append_messages(&store, 2).await;

    let error = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: Some(3),
            limit: 1,
        })
        .await
        .expect_err("barrier beyond last");

    assert!(matches!(
        error,
        StoreError::HistoryBarrierBeyondLast {
            through_seq: 3,
            last_seq: 2
        }
    ));
}

#[tokio::test]
async fn pagination_rejects_missing_path_inside_committed_range() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    store
        .initialize_with_model_config(AgentId::new(), "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    append_messages(&store, 2).await;
    filesystem.remove_entry("/agents/a/messages/2.json");

    let error = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 2,
        })
        .await
        .expect_err("missing committed path");

    assert!(matches!(
        error,
        StoreError::MissingCommittedMessage { seq: 2 }
    ));
}

#[tokio::test]
async fn pagination_reads_nine_and_ten_in_numeric_order() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem, root);
    store
        .initialize_with_model_config(AgentId::new(), "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    append_messages(&store, 10).await;

    let page = store
        .history_page(HistoryQuery {
            after_seq: 8,
            through_seq: Some(10),
            limit: 2,
        })
        .await
        .expect("numeric page");

    assert_eq!(event_sequences(&page.events), [9, 10]);
}

#[tokio::test]
async fn pagination_reads_only_the_requested_message_paths() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    store
        .initialize_with_model_config(AgentId::new(), "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    append_messages(&store, 5).await;
    filesystem.reset_read_counts();

    let page = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: Some(5),
            limit: 1,
        })
        .await
        .expect("single-message page");

    assert_eq!(event_sequences(&page.events), [1]);
    assert_eq!(filesystem.read_count("/agents/a/agent.json"), 1);
    assert_eq!(filesystem.read_count("/agents/a/messages/1.json"), 1);
    for seq in 2..=5 {
        assert_eq!(
            filesystem.read_count(&format!("/agents/a/messages/{seq}.json")),
            0
        );
    }
    assert_eq!(filesystem.list_count(), 0);
}

#[tokio::test]
async fn append_reads_only_state_and_the_constant_size_frontier() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    append_messages(&store, 10).await;
    filesystem.reset_read_counts();

    store
        .append_message(message_envelope(agent_id, SessionId::new(), TurnId::new()))
        .await
        .expect("append after long history");

    assert_eq!(filesystem.list_count(), 0);
    for seq in 1..=10 {
        assert_eq!(
            filesystem.read_count(&format!("/agents/a/messages/{seq}.json")),
            0
        );
    }
    assert_eq!(filesystem.read_count("/agents/a/messages/11.json"), 1);
    assert_eq!(filesystem.read_count("/agents/a/messages/12.json"), 1);
}

#[tokio::test]
async fn append_stops_after_the_filesystem_cas_retry_limit() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    filesystem.fail_absent_writes();

    let error = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        store.append_message(message_envelope(agent_id, SessionId::new(), TurnId::new())),
    )
    .await
    .expect("append terminates")
    .expect_err("append retry exhaustion");

    assert!(matches!(error, StoreError::CasRetriesExhausted));
    assert_eq!(filesystem.absent_write_attempts(), FILESYSTEM_CAS_RETRIES);
}

#[tokio::test]
async fn append_retries_when_one_beyond_belongs_to_an_advanced_state() {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/a").expect("valid root");
    let store = FilesystemAgentStore::new(filesystem.clone(), root);
    let agent_id = AgentId::new();
    let mut advanced_state = store
        .initialize_with_model_config(agent_id, "a".to_owned(), test_model_config())
        .await
        .expect("initialize");
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let requested = message_envelope(agent_id, session_id, turn_id);
    filesystem.pause_next_read("/agents/a/messages/2.json");

    let append = tokio::spawn({
        let store = store.clone();
        async move { store.append_message(requested).await }
    });
    filesystem.wait_for_read_pause().await;
    let committed = sequenced_message_envelope(agent_id, session_id, turn_id, 1);
    let frontier = sequenced_message_envelope(agent_id, session_id, turn_id, 2);
    filesystem.insert_entry("/agents/a/messages/1.json", json_entry(&committed));
    filesystem.insert_entry("/agents/a/messages/2.json", json_entry(&frontier));
    advanced_state.last_seq = 1;
    filesystem.insert_entry("/agents/a/agent.json", json_entry(&advanced_state));
    filesystem.resume_read();

    let appended = append
        .await
        .expect("append task")
        .expect("stale append retries");

    assert_eq!(appended.message_seq(), Some(3));
    assert_eq!(store.load_agent().await.expect("state").last_seq, 3);
    assert!(filesystem.exists("/agents/a/messages/3.json"));
}
