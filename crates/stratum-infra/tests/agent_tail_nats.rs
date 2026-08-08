//! Integration tests for the Agent-scoped NATS tail transport against a real
//! JetStream broker (see `docker-compose.test.yml` and the crate `Makefile`).

use std::{error::Error, time::Duration};

use bytes::Bytes;
use futures_util::StreamExt;
use stratum_core::AgentId;
use stratum_infra::{AgentTailConfig, AgentTailError, AgentTailStream, NatsAgentTail, TailCursor};
use tokio::time::{Instant, sleep, timeout};

const DEFAULT_NATS_URL: &str = "nats://127.0.0.1:44227";
const ORDER_STREAM: &str = "AGENT_TAIL_TEST_ORDER";
const NEW_ONLY_STREAM: &str = "AGENT_TAIL_TEST_NEW_ONLY";
const RESUME_STREAM: &str = "AGENT_TAIL_TEST_RESUME";
const EXPIRY_STREAM: &str = "AGENT_TAIL_TEST_EXPIRY";
const FUTURE_STREAM: &str = "AGENT_TAIL_TEST_FUTURE";
const EMPTY_STREAM: &str = "AGENT_TAIL_TEST_EMPTY";
const ISOLATION_STREAM: &str = "AGENT_TAIL_TEST_ISOLATION";
const RESTART_STREAM: &str = "AGENT_TAIL_TEST_RESTART";
const NO_DELIVERY_GRACE: Duration = Duration::from_millis(500);
const RECEIVE_TIMEOUT: Duration = Duration::from_secs(5);

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_publish_subscribe_preserves_order() -> Result<(), Box<dyn Error>> {
    let tail = connect(&test_config(ORDER_STREAM, "events.agent.test.order")).await?;
    let agent_id = AgentId::new();
    let mut stream = tail.subscribe(&agent_id, None).await?;

    let payloads = [
        Bytes::from_static(b"frame-1"),
        Bytes::from_static(b"frame-2"),
    ];
    let mut cursors = Vec::with_capacity(payloads.len());
    for payload in &payloads {
        cursors.push(tail.publish(&agent_id, payload.clone()).await?);
    }

    for (expected_payload, expected_cursor) in payloads.iter().zip(cursors.iter()) {
        let (cursor, payload) = receive(&mut stream).await?;
        assert_eq!(&payload, expected_payload);
        assert_eq!(cursor, *expected_cursor);
    }
    assert!(cursors.windows(2).all(|pair| pair[0] < pair[1]));
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_no_cursor_subscription_receives_only_new_frames() -> Result<(), Box<dyn Error>>
{
    let tail = connect(&test_config(NEW_ONLY_STREAM, "events.agent.test.newonly")).await?;
    let agent_id = AgentId::new();

    tail.publish(&agent_id, Bytes::from_static(b"history-1"))
        .await?;
    tail.publish(&agent_id, Bytes::from_static(b"history-2"))
        .await?;

    let mut stream = tail.subscribe(&agent_id, None).await?;
    tail.publish(&agent_id, Bytes::from_static(b"live-1"))
        .await?;

    let (_, payload) = receive(&mut stream).await?;
    assert_eq!(payload, Bytes::from_static(b"live-1"));
    assert_no_delivery(&mut stream).await;
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_cursor_subscription_resumes_after_cursor() -> Result<(), Box<dyn Error>> {
    let tail = connect(&test_config(RESUME_STREAM, "events.agent.test.resume")).await?;
    let agent_id = AgentId::new();

    let first = tail
        .publish(&agent_id, Bytes::from_static(b"frame-1"))
        .await?;
    tail.publish(&agent_id, Bytes::from_static(b"frame-2"))
        .await?;
    tail.publish(&agent_id, Bytes::from_static(b"frame-3"))
        .await?;

    let mut stream = tail.subscribe(&agent_id, Some(first)).await?;

    let (_, payload) = receive(&mut stream).await?;
    assert_eq!(payload, Bytes::from_static(b"frame-2"));
    let (_, payload) = receive(&mut stream).await?;
    assert_eq!(payload, Bytes::from_static(b"frame-3"));
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_reports_expired_cursor_after_retention_eviction() -> Result<(), Box<dyn Error>>
{
    let config = AgentTailConfig {
        max_messages: 4,
        ..test_config(EXPIRY_STREAM, "events.agent.test.expiry")
    };
    reset_stream(&config).await?;
    let tail = connect(&config).await?;
    let agent_id = AgentId::new();

    let evicted = tail
        .publish(&agent_id, Bytes::from_static(b"frame-1"))
        .await?;
    for index in 0..8 {
        tail.publish(&agent_id, Bytes::from(format!("evictor-{index}")))
            .await?;
    }

    let result = tail.subscribe(&agent_id, Some(evicted)).await;
    assert!(
        matches!(result, Err(AgentTailError::CursorExpired { cursor }) if cursor == evicted),
        "evicted cursor must fail with the typed CursorExpired error"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_reports_future_cursor_as_expired() -> Result<(), Box<dyn Error>> {
    let config = test_config(FUTURE_STREAM, "events.agent.test.future");
    reset_stream(&config).await?;
    let tail = connect(&config).await?;
    let agent_id = AgentId::new();

    tail.publish(&agent_id, Bytes::from_static(b"frame-1"))
        .await?;

    // A cursor ahead of the tail (forged, or from a recreated stream) must
    // expire instead of silently waiting for future messages.
    let forged: TailCursor = "999999".parse()?;
    let result = tail.subscribe(&agent_id, Some(forged)).await;
    assert!(
        matches!(result, Err(AgentTailError::CursorExpired { cursor }) if cursor == forged),
        "future cursor must fail with the typed CursorExpired error"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_reports_any_cursor_as_expired_on_empty_stream() -> Result<(), Box<dyn Error>> {
    let config = test_config(EMPTY_STREAM, "events.agent.test.empty");
    reset_stream(&config).await?;
    let tail = connect(&config).await?;
    let agent_id = AgentId::new();

    // Nothing was ever published: no cursor can be retained.
    let cursor: TailCursor = "0".parse()?;
    let result = tail.subscribe(&agent_id, Some(cursor)).await;
    assert!(
        matches!(result, Err(AgentTailError::CursorExpired { cursor: expired }) if expired == cursor),
        "an empty stream must expire every cursor"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_isolates_frames_per_agent() -> Result<(), Box<dyn Error>> {
    let tail = connect(&test_config(
        ISOLATION_STREAM,
        "events.agent.test.isolation",
    ))
    .await?;
    let agent_a = AgentId::new();
    let agent_b = AgentId::new();

    let mut stream_a = tail.subscribe(&agent_a, None).await?;
    tail.publish(&agent_b, Bytes::from_static(b"agent-b-frame"))
        .await?;

    assert_no_delivery(&mut stream_a).await;

    tail.publish(&agent_a, Bytes::from_static(b"agent-a-frame"))
        .await?;
    let (_, payload) = receive(&mut stream_a).await?;
    assert_eq!(payload, Bytes::from_static(b"agent-a-frame"));
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_seed_frames_before_restart() -> Result<(), Box<dyn Error>> {
    let tail = connect(&test_config(RESTART_STREAM, "events.agent.test.restart")).await?;
    let agent_id = restart_agent_id();

    tail.publish(&agent_id, Bytes::from_static(b"pre-restart-1"))
        .await?;
    tail.publish(&agent_id, Bytes::from_static(b"pre-restart-2"))
        .await?;
    Ok(())
}

#[tokio::test]
#[ignore = "requires NATS JetStream"]
async fn agent_tail_restart_does_not_redeliver_history_to_new_subscription()
-> Result<(), Box<dyn Error>> {
    let tail = connect(&test_config(RESTART_STREAM, "events.agent.test.restart")).await?;
    let agent_id = restart_agent_id();

    let mut stream = tail.subscribe(&agent_id, None).await?;
    assert_no_delivery(&mut stream).await;

    tail.publish(&agent_id, Bytes::from_static(b"post-restart"))
        .await?;
    let (_, payload) = receive(&mut stream).await?;
    assert_eq!(payload, Bytes::from_static(b"post-restart"));
    Ok(())
}

fn nats_url() -> String {
    std::env::var("STRATUM_INFRA_TEST_NATS_URL").unwrap_or_else(|_| DEFAULT_NATS_URL.to_owned())
}

fn test_config(stream_name: &str, subject_prefix: &str) -> AgentTailConfig {
    AgentTailConfig {
        url: nats_url(),
        stream_name: stream_name.to_owned(),
        subject_prefix: subject_prefix.to_owned(),
        replicas: 1,
        max_age: Duration::from_secs(300),
        max_bytes: 16 * 1024 * 1024,
        max_messages: 1_000,
    }
}

/// Fixed identity shared by the seed/restart test pair; the seed phase runs
/// before the broker restart (see the crate `Makefile`), and retained frames
/// from earlier runs are never delivered to a no-cursor subscription anyway.
fn restart_agent_id() -> AgentId {
    "018f3c2a-7b1d-7e4f-9a2b-3c4d5e6f7a8b"
        .parse()
        .expect("fixed restart agent id is a valid uuid")
}

async fn connect(config: &AgentTailConfig) -> Result<NatsAgentTail, Box<dyn Error>> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match NatsAgentTail::connect(config.clone()).await {
            Ok(tail) => return Ok(tail),
            Err(error) if Instant::now() < deadline => {
                sleep(Duration::from_millis(200)).await;
                drop(error);
            }
            Err(error) => return Err(Box::new(error)),
        }
    }
}

async fn reset_stream(config: &AgentTailConfig) -> Result<(), Box<dyn Error>> {
    let client = async_nats::connect(&config.url).await?;
    let jetstream = async_nats::jetstream::new(client);
    let _ = jetstream.delete_stream(&config.stream_name).await;
    Ok(())
}

async fn receive(stream: &mut AgentTailStream) -> Result<(TailCursor, Bytes), Box<dyn Error>> {
    let item = timeout(RECEIVE_TIMEOUT, stream.next()).await?;
    Ok(item.expect("tail stream ended before the expected frame")?)
}

async fn assert_no_delivery(stream: &mut AgentTailStream) {
    let result = timeout(NO_DELIVERY_GRACE, stream.next()).await;
    assert!(
        result.is_err(),
        "subscription delivered frames it must not deliver: {result:?}"
    );
}
