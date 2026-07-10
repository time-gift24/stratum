//! NATS JetStream event stream bus implementation.

use async_nats::jetstream::{
    self,
    consumer::{DeliverPolicy, push::OrderedConfig},
};
use async_trait::async_trait;
use bytes::Bytes;
use futures_util::StreamExt;
use wyse_core::{
    AgentId, EventCursor, EventRecord, ReplayStart, RunId, RuntimeEvent, StreamEnvelope,
};

use super::{EventStream, EventStreamBus, EventStreamBusError, NatsEventStreamBusConfig};

#[derive(Clone)]
pub(crate) struct NatsEventStreamBus {
    jetstream: jetstream::Context,
    config: NatsEventStreamBusConfig,
}

impl NatsEventStreamBus {
    pub(crate) async fn new(config: NatsEventStreamBusConfig) -> Result<Self, EventStreamBusError> {
        let client = async_nats::connect(&config.url)
            .await
            .map_err(EventStreamBusError::nats)?;
        let jetstream = jetstream::new(client.clone());

        jetstream
            .get_or_create_stream(jetstream::stream::Config {
                name: config.stream_name.clone(),
                subjects: vec![format!("{}.>", config.subject_prefix)],
                storage: jetstream::stream::StorageType::File,
                num_replicas: config.replicas,
                ..Default::default()
            })
            .await
            .map_err(EventStreamBusError::nats)?;

        Ok(Self { jetstream, config })
    }

    fn subject_for(&self, envelope: &StreamEnvelope) -> Result<String, EventStreamBusError> {
        subject_for(&self.config.subject_prefix, envelope)
    }

    fn subscribe_subject(&self, agent_id: AgentId) -> String {
        subscribe_subject(&self.config.subject_prefix, agent_id)
    }

    async fn validate_cursor(
        &self,
        agent_id: AgentId,
        cursor: EventCursor,
    ) -> Result<(), EventStreamBusError> {
        let stream = self
            .jetstream
            .get_stream_no_info(&self.config.stream_name)
            .await
            .map_err(EventStreamBusError::nats)?;
        match stream.get_raw_message(cursor.transport_sequence()).await {
            Ok(message)
                if message
                    .subject
                    .as_str()
                    .starts_with(&format!("{}.{agent_id}.", self.config.subject_prefix)) =>
            {
                Ok(())
            }
            Ok(_) => Err(EventStreamBusError::CursorExpired { cursor }),
            Err(error)
                if error.kind()
                    == async_nats::jetstream::stream::RawMessageErrorKind::NoMessageFound =>
            {
                Err(EventStreamBusError::CursorExpired { cursor })
            }
            Err(error) => Err(EventStreamBusError::nats(error)),
        }
    }
}

#[async_trait]
impl EventStreamBus for NatsEventStreamBus {
    async fn publish(&self, envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
        let subject = self.subject_for(&envelope)?;
        let payload = serde_json::to_vec(&envelope).map_err(EventStreamBusError::Serialize)?;

        self.jetstream
            .publish(subject, Bytes::from(payload))
            .await
            .map_err(EventStreamBusError::nats)?
            .await
            .map_err(EventStreamBusError::nats)?;

        Ok(())
    }

    async fn subscribe_agent(
        &self,
        agent_id: AgentId,
        replay_start: ReplayStart,
    ) -> Result<EventStream, EventStreamBusError> {
        if let ReplayStart::After(cursor) = replay_start {
            self.validate_cursor(agent_id, cursor).await?;
        }
        let subscription_id = RunId::new();
        let deliver_subject = format!("_INBOX.wyse.events.{agent_id}.{subscription_id}");
        let consumer = self
            .jetstream
            .create_consumer_on_stream(
                OrderedConfig {
                    deliver_subject,
                    filter_subject: self.subscribe_subject(agent_id),
                    deliver_policy: deliver_policy(replay_start)?,
                    ..Default::default()
                },
                &self.config.stream_name,
            )
            .await
            .map_err(EventStreamBusError::nats)?;
        let messages = consumer
            .messages()
            .await
            .map_err(EventStreamBusError::nats)?;

        Ok(Box::pin(messages.map(|message| {
            let message = message.map_err(EventStreamBusError::nats)?;
            let cursor = EventCursor::from_transport_sequence(
                message
                    .info()
                    .map_err(|source| EventStreamBusError::Nats { source })?
                    .stream_sequence,
            );
            let envelope = serde_json::from_slice::<StreamEnvelope>(&message.message.payload)
                .map_err(EventStreamBusError::Deserialize)?;
            Ok(EventRecord { cursor, envelope })
        })))
    }
}

fn subject_for(prefix: &str, envelope: &StreamEnvelope) -> Result<String, EventStreamBusError> {
    let RuntimeEvent::Agent { agent_id, event } = &envelope.event else {
        return Err(EventStreamBusError::MissingAgentScope);
    };
    Ok(format!("{prefix}.{agent_id}.{}", event.event_type()))
}

fn subscribe_subject(prefix: &str, agent_id: AgentId) -> String {
    format!("{prefix}.{agent_id}.>")
}

fn deliver_policy(replay_start: ReplayStart) -> Result<DeliverPolicy, EventStreamBusError> {
    match replay_start {
        ReplayStart::All => Ok(DeliverPolicy::All),
        ReplayStart::New => Ok(DeliverPolicy::New),
        ReplayStart::After(cursor) => cursor
            .transport_sequence()
            .checked_add(1)
            .map(|start_sequence| DeliverPolicy::ByStartSequence { start_sequence })
            .ok_or(EventStreamBusError::CursorExpired { cursor }),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use wyse_core::{AgentEvent, AgentId, EventCursor, EventSource, ReplayStart, RuntimeEvent};

    use super::*;

    fn envelope(agent_id: AgentId) -> StreamEnvelope {
        StreamEnvelope {
            run_id: RunId::new(),
            timestamp: Utc::now(),
            source: EventSource::Run,
            event: RuntimeEvent::Agent {
                agent_id,
                event: AgentEvent::Started,
            },
            metadata: BTreeMap::new(),
        }
    }

    #[test]
    fn subject_for_uses_agent_id_and_agent_event_type() {
        let agent_id = AgentId::new();
        let subject = subject_for("wyse.events", &envelope(agent_id)).expect("agent subject");

        assert_eq!(subject, format!("wyse.events.{agent_id}.started"));
    }

    #[test]
    fn subscribe_subject_uses_agent_wildcard() {
        let agent_id = AgentId::new();
        let subject = subscribe_subject("wyse.events", agent_id);

        assert_eq!(subject, format!("wyse.events.{agent_id}.>"));
    }

    #[test]
    fn replay_start_maps_to_jetstream_delivery_policy() {
        let cursor = EventCursor::from_transport_sequence(41);

        assert_eq!(
            deliver_policy(ReplayStart::All).expect("all"),
            DeliverPolicy::All
        );
        assert_eq!(
            deliver_policy(ReplayStart::New).expect("new"),
            DeliverPolicy::New
        );
        assert_eq!(
            deliver_policy(ReplayStart::After(cursor)).expect("after"),
            DeliverPolicy::ByStartSequence { start_sequence: 42 }
        );
    }
}
