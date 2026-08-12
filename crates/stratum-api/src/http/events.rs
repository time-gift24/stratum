//! AgentRuntime-scoped SSE tail endpoint.
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

use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures_util::{StreamExt, stream};
use stratum_infra::{AgentRuntimeTailCursor, AgentRuntimeTailError, AgentRuntimeTailStream};
use tracing::{Span, field};
use utoipa::IntoParams;

use super::parse_agent_runtime_id;
use crate::error::{ApiError, ErrorKind, ErrorResponse};
use crate::frames::AgentRuntimeStreamFrameV1;
use crate::state::AppState;

/// Bounded per-connection server-side buffer.
const SSE_BUFFER_CAPACITY: usize = 256;

/// Event stream query parameters.
#[derive(Debug, serde::Deserialize, IntoParams)]
#[serde(deny_unknown_fields)]
#[into_params(parameter_in = Query)]
pub(crate) struct EventsParams {
    /// Opaque tail cursor to continue from (alternative to `Last-Event-ID`).
    after_cursor: Option<String>,
}

/// One buffered item travelling from the tail pump to the SSE connection.
enum StreamItem {
    /// One retained tail frame with its cursor (becomes the SSE id).
    Frame(AgentRuntimeTailCursor, bytes::Bytes),
    /// Locally produced overflow reset; never carries an SSE id.
    Reset,
}

/// Subscribes to the AgentRuntime-scoped realtime tail.
#[utoipa::path(
    get,
    path = "/v1/agent-runtimes/{agent_runtime_id}/events",
    params(
        ("agent_runtime_id" = String, Path, description = "agent runtime identity"),
        EventsParams,
        ("Last-Event-ID" = Option<String>, Header, description = "opaque tail cursor (alternative to after_cursor)"),
    ),
    responses(
        (status = 200, description = "agent runtime event stream of AgentRuntimeStreamFrameV1 frames",
            content_type = "text/event-stream", body = crate::frames::AgentRuntimeStreamFrameV1),
        (status = 400, description = "both cursor channels supplied or cursor unparsable", body = ErrorResponse),
        (status = 404, description = "agent runtime not found", body = ErrorResponse),
        (status = 410, description = "cursor is no longer retained", body = ErrorResponse),
        (status = 503, description = "realtime tail unavailable", body = ErrorResponse),
    )
)]
pub(crate) async fn get_events(
    State(state): State<Arc<AppState>>,
    Path(agent_runtime_id): Path<String>,
    headers: HeaderMap,
    params: Result<Query<EventsParams>, QueryRejection>,
) -> Result<Response, ApiError> {
    let params = events_query(params)?;
    let agent_runtime_id = parse_agent_runtime_id(&agent_runtime_id)?;
    Span::current().record("agent_runtime_id", field::display(agent_runtime_id));
    let last_event_id = last_event_id(&headers)?;
    let cursor = parse_cursor(last_event_id, params.after_cursor.as_deref())?;

    // Idle AgentRuntimes are subscribable; the read only proves existence and
    // captures the current Session/Turn for the control frame.
    let runtime_state = state
        .pg()
        .read_agent_runtime_state(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    Span::current().record("agent_id", field::display(runtime_state.agent_id));
    let tail = state
        .tail()
        .ok_or_else(|| ApiError::new(ErrorKind::RealtimeUnavailable))?;
    let subscription =
        tail.subscribe(&agent_runtime_id, cursor)
            .await
            .map_err(|error| match error {
                AgentRuntimeTailError::CursorExpired { .. } => {
                    ApiError::new(ErrorKind::CursorExpired)
                }
                other => ApiError::with_source(ErrorKind::RealtimeUnavailable, other),
            })?;

    // The subscription is established before any header is sent; the pump
    // starts buffering immediately, then `stream_ready` is emitted.
    let (tx, rx) = tokio::sync::mpsc::channel::<StreamItem>(SSE_BUFFER_CAPACITY);
    state.spawn_runtime_task(pump_tail(
        agent_runtime_id,
        subscription,
        tx,
        state.shutdown_token(),
    ));

    let ready = AgentRuntimeStreamFrameV1::stream_ready(
        agent_runtime_id,
        runtime_state.agent_id,
        runtime_state.session_id,
        runtime_state.current_turn_id,
    );
    let ready_event = Event::default().data(frame_data(&ready)?);
    let shutdown = state.shutdown_token();
    let live = stream::unfold(
        (rx, shutdown, agent_runtime_id, runtime_state.agent_id),
        |(mut rx, shutdown, agent_runtime_id, agent_id)| async move {
            tokio::select! {
                () = shutdown.cancelled() => None,
                item = rx.recv() => item.and_then(|item| {
                    let event = match item {
                        StreamItem::Frame(cursor, payload) => {
                            let data = String::from_utf8(payload.to_vec()).ok()?;
                            Some(Event::default().id(cursor.to_string()).data(data))
                        }
                        StreamItem::Reset => {
                            let reset = AgentRuntimeStreamFrameV1::stream_reset(
                                agent_runtime_id,
                                agent_id,
                            );
                            let data = frame_data(&reset).ok()?;
                            Some(Event::default().data(data))
                        }
                    };
                    event.map(|event| {
                        (
                            Ok::<Event, Infallible>(event),
                            (rx, shutdown, agent_runtime_id, agent_id),
                        )
                    })
                }),
            }
        },
    );
    let events = stream::once(async move { Ok::<Event, Infallible>(ready_event) }).chain(live);
    Ok(Sse::new(events)
        .keep_alive(
            KeepAlive::new()
                .interval(state.sse_keep_alive())
                .text("keep-alive"),
        )
        .into_response())
}

fn events_query(
    params: Result<Query<EventsParams>, QueryRejection>,
) -> Result<EventsParams, ApiError> {
    params
        .map(|Query(params)| params)
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))
}

fn last_event_id(headers: &HeaderMap) -> Result<Option<&str>, ApiError> {
    headers
        .get("Last-Event-ID")
        .map(|value| {
            value
                .to_str()
                .map_err(|_| ApiError::new(ErrorKind::InvalidCursor))
        })
        .transpose()
}

/// Serializes a locally produced control frame.
fn frame_data(frame: &AgentRuntimeStreamFrameV1) -> Result<String, ApiError> {
    frame
        .to_bytes()
        .map_err(|source| ApiError::with_source(ErrorKind::Internal, source))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Pulls the tail into the bounded per-connection buffer; on overflow the
/// reset item is queued and the pump exits (closing the stream after the
/// buffer drains). The pump also exits when the client disconnects or the
/// process shuts down, so a quiet tail never pins the JetStream consumer.
/// Every `select!` branch is cancellation-safe.
async fn pump_tail(
    agent_runtime_id: stratum_core::AgentRuntimeId,
    mut subscription: AgentRuntimeTailStream,
    tx: tokio::sync::mpsc::Sender<StreamItem>,
    shutdown: tokio_util::sync::CancellationToken,
) {
    loop {
        let item = tokio::select! {
            () = shutdown.cancelled() => return,
            () = tx.closed() => return,
            item = subscription.next() => item,
        };
        match item {
            Some(Ok((cursor, payload))) => {
                if tx.try_send(StreamItem::Frame(cursor, payload)).is_err() {
                    // Bounded buffer overflow: deliver the local reset once
                    // the client drains, then close.
                    let sent = tokio::select! {
                        () = shutdown.cancelled() => return,
                        () = tx.closed() => return,
                        result = tx.send(StreamItem::Reset) => result,
                    };
                    if sent.is_err() {
                        tracing::debug!(
                            agent_runtime_id = %agent_runtime_id,
                            "sse connection closed before reset delivery"
                        );
                    }
                    return;
                }
            }
            Some(Err(error)) => {
                tracing::warn!(agent_runtime_id = %agent_runtime_id, error = %error, "agent runtime tail delivery ended");
                return;
            }
            None => return,
        }
    }
}

/// Parses the cursor input: exactly one of `Last-Event-ID` or `after_cursor`.
fn parse_cursor(
    last_event_id: Option<&str>,
    after_cursor: Option<&str>,
) -> Result<Option<AgentRuntimeTailCursor>, ApiError> {
    match (last_event_id, after_cursor) {
        (Some(_), Some(_)) => Err(ApiError::new(ErrorKind::InvalidCursor)),
        (Some(value), None) | (None, Some(value)) => value
            .parse::<AgentRuntimeTailCursor>()
            .map(Some)
            .map_err(|_| ApiError::new(ErrorKind::InvalidCursor)),
        (None, None) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_accepts_exactly_one_opaque_source() {
        assert_eq!(parse_cursor(None, None).expect("no cursor"), None);
        let agent_runtime_id = stratum_core::AgentRuntimeId::new();
        let header = format!("v1.{agent_runtime_id}.123.42");
        let query = format!("v1.{agent_runtime_id}.123.7");
        let cursor = parse_cursor(Some(&header), None)
            .expect("header cursor parses")
            .expect("cursor present");
        assert_eq!(cursor.to_string(), header);
        let cursor = parse_cursor(None, Some(&query))
            .expect("query cursor parses")
            .expect("cursor present");
        assert_eq!(cursor.to_string(), query);
    }

    #[test]
    fn cursor_rejects_dual_sources_and_garbage() {
        for input in [
            (Some("one"), Some("two")),
            (Some("abc"), None),
            (None, Some("")),
            (None, Some("-3")),
            (Some("18446744073709551616"), None),
        ] {
            let error = parse_cursor(input.0, input.1).expect_err("cursor rejected");
            assert_eq!(error.kind(), ErrorKind::InvalidCursor);
        }
    }

    #[test]
    fn unknown_query_and_non_utf8_header_fail_closed() {
        let uri: http::Uri = "/v1/agent-runtimes/id/events?replay=all"
            .parse()
            .expect("test URI parses");
        let query = Query::<EventsParams>::try_from_uri(&uri);
        let error = events_query(query).expect_err("unknown query is rejected");
        assert_eq!(error.kind(), ErrorKind::InvalidRequest);

        let mut headers = HeaderMap::new();
        headers.insert(
            "Last-Event-ID",
            http::HeaderValue::from_bytes(&[0xff]).expect("opaque header value is valid"),
        );
        let error = last_event_id(&headers).expect_err("non-UTF-8 cursor is rejected");
        assert_eq!(error.kind(), ErrorKind::InvalidCursor);
    }

    #[tokio::test]
    async fn quiet_tail_pump_stops_when_the_client_closes() {
        let subscription = Box::pin(stream::pending()) as AgentRuntimeTailStream;
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(rx);

        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            pump_tail(
                stratum_core::AgentRuntimeId::new(),
                subscription,
                tx,
                tokio_util::sync::CancellationToken::new(),
            ),
        )
        .await
        .expect("closed client stops the pump");
    }

    #[tokio::test]
    async fn full_tail_buffer_does_not_block_shutdown() {
        let agent_runtime_id = stratum_core::AgentRuntimeId::new();
        let cursor = format!("v1.{agent_runtime_id}.1.1")
            .parse::<AgentRuntimeTailCursor>()
            .expect("test cursor parses");
        let items = vec![
            Ok((cursor, bytes::Bytes::from_static(b"one"))),
            Ok((cursor, bytes::Bytes::from_static(b"two"))),
        ];
        let subscription = Box::pin(stream::iter(items)) as AgentRuntimeTailStream;
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let shutdown = tokio_util::sync::CancellationToken::new();
        let task_shutdown = shutdown.clone();
        let task = tokio::spawn(pump_tail(agent_runtime_id, subscription, tx, task_shutdown));
        tokio::task::yield_now().await;
        assert!(!task.is_finished(), "reset waits behind the full buffer");

        shutdown.cancel();
        tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("shutdown releases the full-buffer pump")
            .expect("pump task joins");
    }
}
