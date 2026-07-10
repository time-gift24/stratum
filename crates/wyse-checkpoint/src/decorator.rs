//! Event stream bus checkpoint persistence.

use std::sync::Arc;

use async_trait::async_trait;
use wyse_core::{AgentEvent, AgentId, ReplayStart, RuntimeEvent, StreamEnvelope, TokenUsage};
use wyse_infra::{EventStream, EventStreamBus, EventStreamBusError};

use crate::{AgentCheckpoint, AgentStatus};

/// Persists complete agent messages and state before forwarding them to an event stream bus.
pub struct CheckpointEventStreamBus {
    checkpoint: Arc<dyn AgentCheckpoint>,
    inner: Arc<dyn EventStreamBus>,
}

impl CheckpointEventStreamBus {
    /// Creates a checkpointing event stream bus decorator.
    #[must_use]
    pub fn new(checkpoint: Arc<dyn AgentCheckpoint>, inner: Arc<dyn EventStreamBus>) -> Self {
        Self { checkpoint, inner }
    }

    async fn forward_committed(&self, envelope: StreamEnvelope) {
        if let Err(error) = self.inner.publish(envelope).await {
            tracing::warn!(source = %error, "committed agent event was not retained");
        }
    }
}

#[async_trait]
impl EventStreamBus for CheckpointEventStreamBus {
    async fn publish(&self, envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
        match &envelope.event {
            RuntimeEvent::Agent {
                event: AgentEvent::Message { .. },
                ..
            } => {
                let committed = self
                    .checkpoint
                    .append_message(envelope)
                    .await
                    .map_err(EventStreamBusError::persistence)?;
                self.forward_committed(committed).await;
                Ok(())
            }
            RuntimeEvent::Agent {
                event: AgentEvent::Started { turn_id },
                ..
            } => {
                self.checkpoint
                    .update_state(
                        AgentStatus::Running,
                        Some(envelope.run_id),
                        Some(*turn_id),
                        TokenUsage::default(),
                    )
                    .await
                    .map_err(EventStreamBusError::persistence)?;
                self.forward_committed(envelope).await;
                Ok(())
            }
            RuntimeEvent::Agent { event, .. } => {
                let (status, usage) = match event {
                    AgentEvent::Finished { usage, .. } => (AgentStatus::Finished, *usage),
                    AgentEvent::Failed { usage, .. } => (AgentStatus::Failed, *usage),
                    AgentEvent::Cancelled { usage } => (AgentStatus::Cancelled, *usage),
                    _ => return self.inner.publish(envelope).await,
                };
                let state = self
                    .checkpoint
                    .load_agent()
                    .await
                    .map_err(EventStreamBusError::persistence)?;
                self.checkpoint
                    .update_state(status, Some(envelope.run_id), state.turn_id, usage)
                    .await
                    .map_err(EventStreamBusError::persistence)?;
                self.forward_committed(envelope).await;
                Ok(())
            }
            _ => self.inner.publish(envelope).await,
        }
    }

    async fn subscribe_agent(
        &self,
        agent_id: AgentId,
        replay_start: ReplayStart,
    ) -> Result<EventStream, EventStreamBusError> {
        self.inner.subscribe_agent(agent_id, replay_start).await
    }
}
