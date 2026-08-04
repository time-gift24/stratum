// The shared fixture also exposes fault-injection hooks used by filesystem_store.
#[allow(dead_code)]
mod support;

use std::{collections::HashSet, sync::Arc};

use futures_util::StreamExt;
use stratum_core::{
    AgentId, AgentRuntimeContext, ChatMessage, HistoryQuery, ModelConfig, ModelId, NewAgentMessage,
    ReplayStart, RuntimeEvent, SessionId, StreamEnvelope, TurnId,
};
use stratum_filesystem::VirtualPath;
use stratum_infra::{
    EventStreamBus, FilesystemAgentStore, event_stream_bus::InMemoryEventStreamBus,
};
use stratum_store::AgentStore;
use support::MemoryCasFilesystem;

fn test_model_config() -> ModelConfig {
    ModelConfig::new(
        ModelId::new("openai", "test-model").expect("static model is valid"),
        serde_json::Map::new(),
    )
}

async fn initialized_store(agent_id: AgentId) -> Arc<FilesystemAgentStore> {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/recovery").expect("valid root");
    let store = Arc::new(FilesystemAgentStore::new(filesystem, root));
    store
        .initialize_with_model_config(agent_id, "recovery".to_owned(), test_model_config())
        .await
        .expect("initialize store");
    store
}

fn unsequenced_message(session_id: SessionId, agent_id: AgentId, text: &str) -> NewAgentMessage {
    NewAgentMessage::new(
        &AgentRuntimeContext::direct(session_id),
        agent_id,
        TurnId::new(),
        ChatMessage::user(text),
    )
}

async fn commit_and_publish(
    store: &FilesystemAgentStore,
    retained: &InMemoryEventStreamBus,
    message: NewAgentMessage,
) {
    let committed = store.append_message(message).await.expect("commit message");
    retained.publish(committed).await.expect("publish message");
}

fn visible_message_sequences(
    history: &[StreamEnvelope],
    buffered: &[StreamEnvelope],
    through_seq: u64,
) -> Vec<u64> {
    history
        .iter()
        .chain(
            buffered
                .iter()
                .filter(|envelope| envelope.message_seq().is_some_and(|seq| seq > through_seq)),
        )
        .map(|envelope| envelope.message_seq().expect("complete message sequence"))
        .collect()
}

fn agent_message_key(envelope: &StreamEnvelope) -> (AgentId, u64) {
    let RuntimeEvent::Agent { agent_id, .. } = envelope.event else {
        panic!("recovery fixture should contain an Agent event");
    };
    (
        agent_id,
        envelope
            .message_seq()
            .expect("recovery fixture should contain a committed message"),
    )
}

#[tokio::test]
async fn consumer_first_recovery_delivers_buffered_message_after_fixed_barrier() {
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let store = initialized_store(agent_id).await;
    let retained = Arc::new(InMemoryEventStreamBus::default());
    commit_and_publish(
        &store,
        &retained,
        unsequenced_message(session_id, agent_id, "one"),
    )
    .await;

    let mut live = retained
        .subscribe_session(session_id, ReplayStart::New)
        .await
        .expect("subscribe");
    let first_page = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 1,
        })
        .await
        .expect("history page");
    let barrier = first_page.through_seq;

    commit_and_publish(
        &store,
        &retained,
        unsequenced_message(session_id, agent_id, "two"),
    )
    .await;
    let buffered = live.next().await.expect("buffered").expect("record");
    let buffered_seq = buffered
        .envelope
        .message_seq()
        .expect("complete message sequence");
    let visible_sequences = visible_message_sequences(
        &first_page.events,
        std::slice::from_ref(&buffered.envelope),
        barrier,
    );

    assert_eq!(barrier, 1);
    assert_eq!(first_page.events[0].message_seq(), Some(1));
    assert!(buffered_seq > barrier);
    assert_eq!(visible_sequences, [1, 2]);
}

#[tokio::test]
async fn consumer_first_recovery_classifies_buffered_message_inside_barrier_as_duplicate() {
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let store = initialized_store(agent_id).await;
    let retained = Arc::new(InMemoryEventStreamBus::default());
    commit_and_publish(
        &store,
        &retained,
        unsequenced_message(session_id, agent_id, "one"),
    )
    .await;
    let mut live = retained
        .subscribe_session(session_id, ReplayStart::New)
        .await
        .expect("subscribe");

    commit_and_publish(
        &store,
        &retained,
        unsequenced_message(session_id, agent_id, "two"),
    )
    .await;
    let buffered = live.next().await.expect("buffered").expect("record");
    let page = store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 256,
        })
        .await
        .expect("history page");
    let buffered_seq = buffered
        .envelope
        .message_seq()
        .expect("complete message sequence");
    let visible_sequences = visible_message_sequences(
        &page.events,
        std::slice::from_ref(&buffered.envelope),
        page.through_seq,
    );

    assert_eq!(page.through_seq, 2);
    assert!(buffered_seq <= page.through_seq);
    assert_eq!(visible_sequences, [1, 2]);
}

#[tokio::test]
async fn multi_agent_session_recovery_uses_agent_scoped_message_barriers() {
    let first_agent_id = AgentId::new();
    let second_agent_id = AgentId::new();
    let session_id = SessionId::new();
    let first_store = initialized_store(first_agent_id).await;
    let second_store = initialized_store(second_agent_id).await;
    let retained = Arc::new(InMemoryEventStreamBus::default());

    commit_and_publish(
        &first_store,
        &retained,
        unsequenced_message(session_id, first_agent_id, "first agent one"),
    )
    .await;
    commit_and_publish(
        &second_store,
        &retained,
        unsequenced_message(session_id, second_agent_id, "second agent one"),
    )
    .await;

    let mut live = retained
        .subscribe_session(session_id, ReplayStart::New)
        .await
        .expect("subscribe");
    let first_history = first_store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 256,
        })
        .await
        .expect("first history page");
    let second_history = second_store
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 256,
        })
        .await
        .expect("second history page");

    commit_and_publish(
        &first_store,
        &retained,
        unsequenced_message(session_id, first_agent_id, "first agent two"),
    )
    .await;
    commit_and_publish(
        &second_store,
        &retained,
        unsequenced_message(session_id, second_agent_id, "second agent two"),
    )
    .await;
    let buffered = vec![
        live.next()
            .await
            .expect("first buffered")
            .expect("record")
            .envelope,
        live.next()
            .await
            .expect("second buffered")
            .expect("record")
            .envelope,
    ];

    let mut visible = first_history
        .events
        .iter()
        .chain(&second_history.events)
        .map(agent_message_key)
        .collect::<HashSet<_>>();
    for envelope in &buffered {
        let (agent_id, message_seq) = agent_message_key(envelope);
        let barrier = if agent_id == first_agent_id {
            first_history.through_seq
        } else if agent_id == second_agent_id {
            second_history.through_seq
        } else {
            panic!("unexpected Agent in Session stream");
        };
        if message_seq > barrier {
            visible.insert((agent_id, message_seq));
        }
    }

    assert_eq!(first_history.through_seq, 1);
    assert_eq!(second_history.through_seq, 1);
    assert_eq!(visible.len(), 4);
    assert!(visible.contains(&(first_agent_id, 1)));
    assert!(visible.contains(&(first_agent_id, 2)));
    assert!(visible.contains(&(second_agent_id, 1)));
    assert!(visible.contains(&(second_agent_id, 2)));
}
