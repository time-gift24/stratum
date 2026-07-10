// The shared fixture also exposes fault-injection hooks used by filesystem_checkpoint.
#[allow(dead_code)]
mod support;

use std::{collections::BTreeMap, sync::Arc};

use chrono::Utc;
use futures_util::StreamExt;
use support::MemoryCasFilesystem;
use wyse_checkpoint::{AgentCheckpoint, FilesystemAgentCheckpoint};
use wyse_core::{AgentId, ChatMessage, EventSource, HistoryQuery, ReplayStart, RunId, TurnId};
use wyse_filesystem::VirtualPath;
use wyse_infra::{EventStreamBus, event_stream_bus::InMemoryEventStreamBus};

async fn initialized_checkpoint(agent_id: AgentId) -> FilesystemAgentCheckpoint {
    let filesystem = Arc::new(MemoryCasFilesystem::default());
    let root = VirtualPath::try_from("/agents/recovery").expect("valid root");
    let checkpoint = FilesystemAgentCheckpoint::new(filesystem, root);
    checkpoint
        .initialize(agent_id, "recovery".to_owned())
        .await
        .expect("initialize checkpoint");
    checkpoint
}

async fn checkpoint_and_publish(
    checkpoint: &FilesystemAgentCheckpoint,
    bus: &InMemoryEventStreamBus,
    text: &str,
) {
    let envelope = checkpoint
        .append_message(
            RunId::new(),
            TurnId::new(),
            Utc::now(),
            EventSource::Run,
            ChatMessage::user(text),
            BTreeMap::new(),
        )
        .await
        .expect("append message");
    bus.publish(envelope).await.expect("publish message");
}

#[tokio::test]
async fn consumer_first_recovery_delivers_buffered_message_after_fixed_barrier() {
    let agent_id = AgentId::new();
    let checkpoint = initialized_checkpoint(agent_id).await;
    let bus = InMemoryEventStreamBus::default();
    checkpoint_and_publish(&checkpoint, &bus, "one").await;

    let mut live = bus
        .subscribe_agent(agent_id, ReplayStart::New)
        .await
        .expect("subscribe");
    let first_page = checkpoint
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 1,
        })
        .await
        .expect("history page");
    let barrier = first_page.through_seq;

    checkpoint_and_publish(&checkpoint, &bus, "two").await;
    let buffered = live.next().await.expect("buffered").expect("record");
    let buffered_seq = buffered
        .envelope
        .event
        .business_seq()
        .expect("complete message sequence");

    assert_eq!(barrier, 1);
    assert_eq!(first_page.events[0].event.business_seq(), Some(1));
    assert!(buffered_seq > barrier);
}

#[tokio::test]
async fn consumer_first_recovery_classifies_buffered_message_inside_barrier_as_duplicate() {
    let agent_id = AgentId::new();
    let checkpoint = initialized_checkpoint(agent_id).await;
    let bus = InMemoryEventStreamBus::default();
    checkpoint_and_publish(&checkpoint, &bus, "one").await;
    let mut live = bus
        .subscribe_agent(agent_id, ReplayStart::New)
        .await
        .expect("subscribe");

    checkpoint_and_publish(&checkpoint, &bus, "two").await;
    let buffered = live.next().await.expect("buffered").expect("record");
    let page = checkpoint
        .history_page(HistoryQuery {
            after_seq: 0,
            through_seq: None,
            limit: 256,
        })
        .await
        .expect("history page");
    let buffered_seq = buffered
        .envelope
        .event
        .business_seq()
        .expect("complete message sequence");

    assert_eq!(page.through_seq, 2);
    assert!(buffered_seq <= page.through_seq);
}
