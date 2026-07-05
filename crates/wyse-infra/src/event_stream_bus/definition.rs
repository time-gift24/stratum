//! Event stream bus public definitions.

use std::{future::Future, pin::Pin};

use futures_core::Stream;
use wyse_core::{RunId, StreamEnvelope};

use super::EventStreamBusError;

/// Stream of runtime event envelopes.
pub type EventStream =
    Pin<Box<dyn Stream<Item = Result<StreamEnvelope, EventStreamBusError>> + Send + 'static>>;

/// Publishes and subscribes to runtime event streams.
pub trait EventStreamBus: Send + Sync {
    /// Publishes one complete stream envelope.
    fn publish(
        &self,
        envelope: StreamEnvelope,
    ) -> impl Future<Output = Result<(), EventStreamBusError>> + Send;

    /// Subscribes to live events for one run.
    fn subscribe_run(
        &self,
        run_id: RunId,
    ) -> impl Future<Output = Result<EventStream, EventStreamBusError>> + Send;
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
