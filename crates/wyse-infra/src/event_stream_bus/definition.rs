//! Event stream bus public definitions.

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use wyse_core::{AgentId, EventRecord, ReplayStart, StreamEnvelope};

use super::EventStreamBusError;

/// Stream of runtime event records.
pub type EventStream =
    Pin<Box<dyn Stream<Item = Result<EventRecord, EventStreamBusError>> + Send + 'static>>;

/// Publishes and subscribes to runtime event streams.
#[async_trait]
pub trait EventStreamBus: Send + Sync {
    /// Publishes one complete stream envelope.
    async fn publish(&self, envelope: StreamEnvelope) -> Result<(), EventStreamBusError>;

    /// Subscribes to one agent's retained and live events from the requested position.
    async fn subscribe_agent(
        &self,
        agent_id: AgentId,
        replay_start: ReplayStart,
    ) -> Result<EventStream, EventStreamBusError>;
}

/// Configuration for the NATS event stream bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatsEventStreamBusConfig {
    /// NATS server URL.
    pub url: String,
    /// JetStream stream name.
    pub stream_name: String,
    /// Subject prefix before `<run_id>.<event_type>`.
    pub subject_prefix: String,
    /// Number of stream replicas.
    pub replicas: usize,
}

impl Default for NatsEventStreamBusConfig {
    fn default() -> Self {
        Self {
            url: "nats://localhost:4222".to_owned(),
            stream_name: "WYSE_EVENTS".to_owned(),
            subject_prefix: "wyse.events".to_owned(),
            replicas: 1,
        }
    }
}
