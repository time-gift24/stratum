//! Event stream bus store persistence.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use stratum_core::{AgentEvent, ReplayStart, RuntimeEvent, SessionId, StreamEnvelope};
use stratum_store::{AgentStatus, AgentStore};
use tokio::time::timeout;

use crate::{EventStream, EventStreamBus, EventStreamBusError};

const COMMITTED_FORWARD_GRACE: Duration = Duration::from_secs(1);

/// Persists terminal agent state before forwarding committed events to an event stream bus.
pub struct StoreEventStreamBus {
    store: Arc<dyn AgentStore>,
    inner: Arc<dyn EventStreamBus>,
}

impl StoreEventStreamBus {
    /// Creates a store-backed event stream bus decorator.
    #[must_use]
    pub fn new(store: Arc<dyn AgentStore>, inner: Arc<dyn EventStreamBus>) -> Self {
        Self { store, inner }
    }

    async fn forward_committed(&self, envelope: StreamEnvelope) {
        match timeout(COMMITTED_FORWARD_GRACE, self.inner.publish(envelope)).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(source = %error, "committed agent event was not retained");
            }
            Err(_) => {
                tracing::warn!(
                    grace_millis = COMMITTED_FORWARD_GRACE.as_millis(),
                    "committed agent event forwarding timed out"
                );
            }
        }
    }
}

#[async_trait]
impl EventStreamBus for StoreEventStreamBus {
    async fn publish(&self, envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
        match &envelope.event {
            RuntimeEvent::Agent {
                event: AgentEvent::Message { .. } | AgentEvent::Started,
                ..
            } => {
                self.forward_committed(envelope).await;
                Ok(())
            }
            RuntimeEvent::Agent {
                turn_id,
                event: AgentEvent::IterationCompleted { iteration, usage },
                ..
            } => {
                self.store
                    .complete_iteration(envelope.session_id, *turn_id, *iteration, *usage)
                    .await
                    .map_err(EventStreamBusError::persistence)?;
                self.forward_committed(envelope).await;
                Ok(())
            }
            RuntimeEvent::Agent { turn_id, event, .. } => {
                let (status, usage) = match event {
                    AgentEvent::Finished { usage, .. } => (AgentStatus::Finished, *usage),
                    AgentEvent::Failed { usage, .. } => (AgentStatus::Failed, *usage),
                    AgentEvent::Cancelled { usage } => (AgentStatus::Cancelled, *usage),
                    _ => return self.inner.publish(envelope).await,
                };
                self.store
                    .update_state(status, Some(envelope.session_id), Some(*turn_id), usage)
                    .await
                    .map_err(EventStreamBusError::persistence)?;
                self.forward_committed(envelope).await;
                Ok(())
            }
            _ => self.inner.publish(envelope).await,
        }
    }

    async fn subscribe_session(
        &self,
        session_id: SessionId,
        replay_start: ReplayStart,
    ) -> Result<EventStream, EventStreamBusError> {
        self.inner.subscribe_session(session_id, replay_start).await
    }
}
