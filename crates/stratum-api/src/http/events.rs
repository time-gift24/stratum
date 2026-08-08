//! Agent-scoped SSE tail endpoint.
//!
//! Cursor input is `Last-Event-ID` XOR `after_cursor`; any SSE id is an
//! opaque NATS tail cursor. `stream_ready` is sent only after the
//! subscription is established and the server-side buffer is active. A cursor
//! discarded by retention answers 410 before headers; a NATS failure answers
//! 503 while every Postgres-backed command keeps working. Once the stream is
//! established, a bounded per-connection buffer applies: on overflow the
//! connection sends a no-id `stream_reset { reason: "buffer_overflow" }`
//! frame and closes; that frame never touches NATS or Postgres.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use stratum_infra::{AgentTailError, AgentTailStream, TailCursor};
use tracing::{Span, field};
use utoipa::IntoParams;

use super::parse_agent_id;
use crate::error::{ApiError, ErrorKind, ErrorResponse};
use crate::frames::AgentStreamFrameV1;
use crate::state::AppState;

/// Bounded per-connection server-side buffer.
const SSE_BUFFER_CAPACITY: usize = 256;

/// Event stream query parameters.
#[derive(Debug, serde::Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct EventsParams {
    /// Opaque tail cursor to continue from (alternative to `Last-Event-ID`).
    after_cursor: Option<String>,
}

/// One buffered item travelling from the tail pump to the SSE connection.
enum StreamItem {
    /// One retained tail frame with its cursor (becomes the SSE id).
    Frame(TailCursor, bytes::Bytes),
    /// Locally produced overflow reset; never carries an SSE id.
    Reset,
}

/// Subscribes to the Agent-scoped realtime tail.
#[utoipa::path(
    get,
    path = "/v1/agents/{agent_id}/events",
    params(
        ("agent_id" = String, Path, description = "agent identity"),
        EventsParams,
        ("Last-Event-ID" = Option<String>, Header, description = "opaque tail cursor (alternative to after_cursor)"),
    ),
    responses(
        (status = 200, description = "agent event stream of AgentStreamFrameV1 frames",
            content_type = "text/event-stream", body = crate::frames::AgentStreamFrameV1),
        (status = 400, description = "both cursor channels supplied or cursor unparsable", body = ErrorResponse),
        (status = 404, description = "agent not found", body = ErrorResponse),
        (status = 410, description = "cursor is no longer retained", body = ErrorResponse),
        (status = 503, description = "realtime tail unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_events(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
    headers: HeaderMap,
    Query(params): Query<EventsParams>,
) -> Result<Response, ApiError> {
    let agent_id = parse_agent_id(&agent_id)?;
    Span::current().record("agent_id", field::display(agent_id));
    let last_event_id = headers
        .get("Last-Event-ID")
        .and_then(|value| value.to_str().ok());
    let cursor = parse_cursor(last_event_id, params.after_cursor.as_deref())?;

    // Idle Agents are subscribable; the read only proves existence and
    // captures the current Session/Turn for the control frame.
    let agent_state = state
        .pg()
        .read_agent_state(agent_id)
        .await
        .map_err(ApiError::from_postgres)?;
    let tail = state
        .tail()
        .ok_or_else(|| ApiError::new(ErrorKind::RealtimeUnavailable))?;
    let subscription = tail
        .subscribe(&agent_id, cursor)
        .await
        .map_err(|error| match error {
            AgentTailError::CursorExpired { .. } => ApiError::new(ErrorKind::CursorExpired),
            other => ApiError::with_source(ErrorKind::RealtimeUnavailable, other),
        })?;

    // The subscription is established before any header is sent; the pump
    // starts buffering immediately, then `stream_ready` is emitted.
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamItem>(SSE_BUFFER_CAPACITY);
    tokio::spawn(pump_tail(agent_id, subscription, tx));

    let ready = AgentStreamFrameV1::stream_ready(
        agent_id,
        agent_state.session_id,
        agent_state.current_turn_id,
    );
    let ready_event = Event::default().data(frame_data(&ready)?);
    let shutdown = state.shutdown_token();
    let live = stream::unfold(
        (rx, shutdown, agent_id),
        |(mut rx, shutdown, agent_id)| async move {
            tokio::select! {
                () = shutdown.cancelled() => None,
                item = rx.recv() => item.and_then(|item| {
                    let event = match item {
                        StreamItem::Frame(cursor, payload) => {
                            let data = String::from_utf8(payload.to_vec()).ok()?;
                            Some(Event::default().id(cursor.to_string()).data(data))
                        }
                        StreamItem::Reset => {
                            let reset = AgentStreamFrameV1::stream_reset(agent_id);
                            let data = frame_data(&reset).ok()?;
                            Some(Event::default().data(data))
                        }
                    };
                    event.map(|event| (Ok::<Event, Infallible>(event), (rx, shutdown, agent_id)))
                }),
            }
        },
    );
    let events = stream::once(async move { Ok::<Event, Infallible>(ready_event) }).chain(live);
    Ok(Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(Duration::from_secs(15))
                .text("keep-alive"),
        )
        .into_response())
}

/// Serializes a locally produced control frame.
fn frame_data(frame: &AgentStreamFrameV1) -> Result<String, ApiError> {
    frame
        .to_bytes()
        .map_err(|source| ApiError::with_source(ErrorKind::Internal, source))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Pulls the tail into the bounded per-connection buffer; on overflow the
/// reset item is queued and the pump exits (closing the stream after the
/// buffer drains).
async fn pump_tail(
    agent_id: stratum_core::AgentId,
    mut subscription: AgentTailStream,
    tx: tokio::sync::mpsc::Sender<StreamItem>,
) {
    while let Some(item) = subscription.next().await {
        match item {
            Ok((cursor, payload)) => {
                if tx.try_send(StreamItem::Frame(cursor, payload)).is_err() {
                    // Bounded buffer overflow: deliver the local reset once
                    // the client drains, then close.
                    if tx.send(StreamItem::Reset).await.is_err() {
                        tracing::debug!(agent_id = %agent_id, "sse connection closed before reset delivery");
                    }
                    return;
                }
            }
            Err(error) => {
                tracing::warn!(agent_id = %agent_id, error = %error, "agent tail delivery ended");
                return;
            }
        }
    }
}

/// Parses the cursor input: exactly one of `Last-Event-ID` or `after_cursor`.
fn parse_cursor(
    last_event_id: Option<&str>,
    after_cursor: Option<&str>,
) -> Result<Option<TailCursor>, ApiError> {
    match (last_event_id, after_cursor) {
        (Some(_), Some(_)) => Err(ApiError::new(ErrorKind::InvalidCursor)),
        (Some(value), None) | (None, Some(value)) => value
            .parse::<TailCursor>()
            .map(Some)
            .map_err(|_| ApiError::new(ErrorKind::InvalidCursor)),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_accepts_exactly_one_decimal_source() {
        assert_eq!(parse_cursor(None, None).expect("no cursor"), None);
        let cursor = parse_cursor(Some("42"), None)
            .expect("header cursor parses")
            .expect("cursor present");
        assert_eq!(cursor.to_string(), "42");
        let cursor = parse_cursor(None, Some("7"))
            .expect("query cursor parses")
            .expect("cursor present");
        assert_eq!(cursor.to_string(), "7");
    }

    #[test]
    fn cursor_rejects_dual_sources_and_garbage() {
        for input in [
            (Some("1"), Some("2")),
            (Some("abc"), None),
            (None, Some("")),
            (None, Some("-3")),
            (Some("18446744073709551616"), None),
        ] {
            let error = parse_cursor(input.0, input.1).expect_err("cursor rejected");
            assert_eq!(error.kind(), ErrorKind::InvalidCursor);
        }
    }
}
