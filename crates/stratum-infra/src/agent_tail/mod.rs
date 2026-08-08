//! Narrow concrete Agent-scoped NATS tail transport.
//!
//! The tail publishes and subscribes short-retained per-agent frame streams on
//! one JetStream stream with discard-old limits retention. It is the only
//! `async-nats` boundary in the workspace: payloads are opaque bytes owned by
//! the caller (`AgentStreamFrameV1` serialization lives in `stratum-api`), and
//! JetStream guarantees per-subject ordering only. The tail is not a durable
//! history and never replays across restarts; the Postgres durable ledger is
//! the recovery truth.

mod config;
mod cursor;
mod error;
mod subject;

use std::pin::Pin;

use async_nats::jetstream::{
    self,
    consumer::{DeliverPolicy, push::OrderedConfig},
};
use bytes::Bytes;
use futures_core::Stream;
use futures_util::{StreamExt, future};
use stratum_core::AgentId;

pub use config::AgentTailConfig;
pub use cursor::TailCursor;
pub use error::{AgentTailError, TailCursorParseError};

/// Ordered stream of `(cursor, payload)` items from one agent's tail.
///
/// The first error item terminates the stream; undeliverable conditions
/// surface as typed [`AgentTailError`] values, never panics.
pub type AgentTailStream =
    Pin<Box<dyn Stream<Item = Result<(TailCursor, Bytes), AgentTailError>> + Send + 'static>>;

/// Concrete Agent-scoped NATS tail transport over one JetStream stream.
#[derive(Clone)]
pub struct NatsAgentTail {
    jetstream: jetstream::Context,
    config: AgentTailConfig,
}

impl NatsAgentTail {
    /// Connects to NATS and creates or updates the tail stream with
    /// discard-old limits retention and the configured finite limits.
    ///
    /// # Errors
    ///
    /// Returns [`AgentTailError::InvalidConfig`] if a retention limit or
    /// replica count is invalid, or [`AgentTailError::Nats`] if the broker
    /// cannot be reached or rejects the stream configuration.
    pub async fn connect(config: AgentTailConfig) -> Result<Self, AgentTailError> {
        config.validate()?;
        let client = async_nats::connect(&config.url)
            .await
            .map_err(AgentTailError::nats)?;
        let jetstream = jetstream::new(client);

        jetstream
            .create_or_update_stream(config.stream_config())
            .await
            .map_err(AgentTailError::nats)?;

        Ok(Self { jetstream, config })
    }

    /// Publishes one opaque frame to one agent's tail and returns its cursor.
    ///
    /// The payload is opaque bytes; the caller owns `AgentStreamFrameV1`
    /// serialization. Durable product and telemetry frames share the same
    /// per-agent tail.
    ///
    /// # Errors
    ///
    /// Returns [`AgentTailError::Nats`] if the publish or its acknowledgement
    /// fails. A publish failure after a Postgres commit never rolls back the
    /// commit; callers log it once and rely on Postgres recovery.
    pub async fn publish(
        &self,
        agent_id: &AgentId,
        payload: Bytes,
    ) -> Result<TailCursor, AgentTailError> {
        let subject = subject::agent_subject(&self.config.subject_prefix, agent_id);
        let result = self
            .jetstream
            .publish(subject, payload)
            .await
            .map_err(AgentTailError::nats)?
            .await
            .map_err(AgentTailError::nats)
            .map(|ack| TailCursor::from_transport_sequence(ack.sequence));

        // No logging here: the typed error travels to the handling boundary
        // (the stratum-api dispatcher), which logs it exactly once.
        result
    }

    /// Subscribes to one agent's tail.
    ///
    /// Without a cursor only frames published after the subscription is
    /// established are delivered ([`DeliverPolicy::New`]); retained history is
    /// never replayed. With a cursor the tail resumes after that position while
    /// it is still retained.
    ///
    /// # Errors
    ///
    /// Returns [`AgentTailError::CursorExpired`] before any delivery if the
    /// requested cursor position was discarded by retention (the API maps this
    /// to HTTP 410 before the SSE stream starts), or [`AgentTailError::Nats`]
    /// if the subscription cannot be created.
    pub async fn subscribe(
        &self,
        agent_id: &AgentId,
        after: Option<TailCursor>,
    ) -> Result<AgentTailStream, AgentTailError> {
        if let Some(cursor) = after {
            self.ensure_retained(cursor).await?;
        }
        let deliver_subject = self.jetstream.client().new_inbox();
        let consumer = self
            .jetstream
            .create_consumer_on_stream(
                OrderedConfig {
                    deliver_subject,
                    filter_subject: subject::agent_subject(&self.config.subject_prefix, agent_id),
                    deliver_policy: deliver_policy(after),
                    ..Default::default()
                },
                &self.config.stream_name,
            )
            .await
            .map_err(AgentTailError::nats)?;
        let messages = consumer.messages().await.map_err(AgentTailError::nats)?;

        let items = messages.scan(false, move |terminated, message| {
            if *terminated {
                return future::ready(None);
            }
            let item = message.map_err(AgentTailError::nats).and_then(|message| {
                let sequence = message
                    .info()
                    .map_err(AgentTailError::nats)?
                    .stream_sequence;
                Ok((
                    TailCursor::from_transport_sequence(sequence),
                    message.message.payload,
                ))
            });
            if item.is_err() {
                // Terminate on the first error; the typed error travels to the
                // handling boundary (stratum-api), which logs it exactly once.
                *terminated = true;
            }
            future::ready(Some(item))
        });

        Ok(Box::pin(items) as AgentTailStream)
    }

    async fn ensure_retained(&self, cursor: TailCursor) -> Result<(), AgentTailError> {
        let stream = self
            .jetstream
            .get_stream(&self.config.stream_name)
            .await
            .map_err(AgentTailError::nats)?;
        let state = &stream.cached_info().state;
        check_retained(state.first_sequence, state.last_sequence, cursor)
    }
}

fn deliver_policy(after: Option<TailCursor>) -> DeliverPolicy {
    after
        .and_then(|cursor| {
            cursor
                .transport_sequence()
                .checked_add(1)
                .map(|start_sequence| DeliverPolicy::ByStartSequence { start_sequence })
        })
        // No cursor starts at the current tail; a cursor at u64::MAX cannot be
        // advanced and no retained message can follow it, so only new messages
        // satisfy it.
        .unwrap_or(DeliverPolicy::New)
}

/// A cursor is retained only while it addresses a position inside the live
/// tail. An empty stream (`last_sequence == 0`, no message published yet)
/// retains nothing, so every cursor is expired. A cursor ahead of
/// `last_sequence` (forged, or issued by a since-recreated stream) would
/// otherwise silently wait for future messages and skip the current tail, so
/// it is expired as well; both cases force the caller's cold bootstrap.
fn check_retained(
    first_sequence: u64,
    last_sequence: u64,
    cursor: TailCursor,
) -> Result<(), AgentTailError> {
    let sequence = cursor.transport_sequence();
    let earliest_valid = first_sequence.saturating_sub(1);
    if last_sequence == 0 || sequence > last_sequence || sequence < earliest_valid {
        Err(AgentTailError::CursorExpired { cursor })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_cursor_delivers_only_new_messages() {
        assert_eq!(deliver_policy(None), DeliverPolicy::New);
    }

    #[test]
    fn cursor_resumes_after_its_transport_sequence() {
        assert_eq!(
            deliver_policy(Some(TailCursor::from_transport_sequence(41))),
            DeliverPolicy::ByStartSequence { start_sequence: 42 }
        );
    }

    #[test]
    fn unadvanceable_cursor_falls_back_to_new_only() {
        assert_eq!(
            deliver_policy(Some(TailCursor::from_transport_sequence(u64::MAX))),
            DeliverPolicy::New
        );
    }

    #[test]
    fn cursor_at_or_after_earliest_retained_position_is_valid() {
        let cursor = TailCursor::from_transport_sequence(9);

        assert!(check_retained(10, 50, cursor).is_ok());
        assert!(check_retained(10, 50, TailCursor::from_transport_sequence(50)).is_ok());
    }

    #[test]
    fn cursor_before_earliest_retained_position_is_expired() {
        let cursor = TailCursor::from_transport_sequence(8);

        assert!(matches!(
            check_retained(10, 50, cursor),
            Err(AgentTailError::CursorExpired { cursor: expired }) if expired == cursor
        ));
    }

    #[test]
    fn cursor_beyond_last_sequence_is_expired() {
        let cursor = TailCursor::from_transport_sequence(51);

        assert!(matches!(
            check_retained(10, 50, cursor),
            Err(AgentTailError::CursorExpired { cursor: expired }) if expired == cursor
        ));
        // A forged far-future cursor on a fresh stream is expired as well.
        let forged = TailCursor::from_transport_sequence(u64::MAX);
        assert!(matches!(
            check_retained(1, 3, forged),
            Err(AgentTailError::CursorExpired { cursor: expired }) if expired == forged
        ));
    }

    #[test]
    fn empty_stream_expires_every_cursor() {
        for sequence in [0, 1, 42] {
            let cursor = TailCursor::from_transport_sequence(sequence);
            assert!(matches!(
                check_retained(0, 0, cursor),
                Err(AgentTailError::CursorExpired { cursor: expired }) if expired == cursor
            ));
        }
    }
}
