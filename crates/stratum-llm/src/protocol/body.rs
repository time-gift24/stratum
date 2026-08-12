//! Bounded HTTP response-body collection shared by provider adapters.

use std::error::Error;
use std::time::Duration;

use bytes::{Bytes, BytesMut};
use futures_core::Stream;
use futures_util::StreamExt;

use crate::LlmError;

/// Successful non-streaming responses may contain a sizeable completion but
/// must never grow without bound in memory.
const MAX_SUCCESS_BODY_BYTES: usize = 8 * 1024 * 1024;
/// Provider error payloads only need a small structured error envelope.
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;

/// Semantic class of a response body, selecting its fixed safety bound and
/// stable overflow error.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ResponseBodyKind {
    Success,
    Error,
}

impl ResponseBodyKind {
    const fn max_bytes(self) -> usize {
        match self {
            Self::Success => MAX_SUCCESS_BODY_BYTES,
            Self::Error => MAX_ERROR_BODY_BYTES,
        }
    }

    const fn overflow_message(self) -> &'static str {
        match self {
            Self::Success => "provider response body exceeds size limit",
            Self::Error => "provider error body exceeds size limit",
        }
    }
}

/// Reads one reqwest body with a per-chunk silence bound and a fixed memory
/// cap. The partial body is dropped on timeout, transport failure, or
/// overflow and is never included in the returned error.
pub(crate) async fn read_response_body(
    response: reqwest::Response,
    idle_timeout: Duration,
    kind: ResponseBodyKind,
) -> Result<Bytes, LlmError> {
    read_body_stream(response.bytes_stream(), idle_timeout, kind).await
}

async fn read_body_stream<S, E>(
    chunks: S,
    idle_timeout: Duration,
    kind: ResponseBodyKind,
) -> Result<Bytes, LlmError>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: Error + Send + Sync + 'static,
{
    let max_bytes = kind.max_bytes();
    let mut body = BytesMut::with_capacity(max_bytes.min(8 * 1024));
    futures_util::pin_mut!(chunks);
    loop {
        let next = tokio::time::timeout(idle_timeout, chunks.next())
            .await
            .map_err(LlmError::transport)?;
        let Some(chunk) = next else {
            return Ok(body.freeze());
        };
        let chunk = chunk.map_err(LlmError::transport)?;
        let Some(next_len) = body.len().checked_add(chunk.len()) else {
            return Err(LlmError::InvalidProviderPayload(kind.overflow_message()));
        };
        if next_len > max_bytes {
            return Err(LlmError::InvalidProviderPayload(kind.overflow_message()));
        }
        body.extend_from_slice(&chunk);
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::time::Duration;

    use bytes::Bytes;
    use futures_util::stream;

    use super::*;

    #[tokio::test(start_paused = true)]
    async fn stalled_error_body_fails_at_the_configured_idle_bound() {
        let idle_timeout = Duration::from_secs(7);
        let chunks = stream::pending::<Result<Bytes, io::Error>>();
        let started = tokio::time::Instant::now();

        let result = read_body_stream(chunks, idle_timeout, ResponseBodyKind::Error).await;

        assert!(matches!(result, Err(LlmError::Transport(_))));
        assert!(started.elapsed() >= idle_timeout);
    }

    #[tokio::test]
    async fn multi_chunk_error_body_is_rejected_before_aggregation_exceeds_the_cap() {
        let chunk_len = MAX_ERROR_BODY_BYTES / 2 + 1;
        let chunks = stream::iter([
            Ok::<_, io::Error>(Bytes::from(vec![b'a'; chunk_len])),
            Ok(Bytes::from(vec![b'b'; chunk_len])),
        ]);

        let result =
            read_body_stream(chunks, Duration::from_secs(1), ResponseBodyKind::Error).await;

        assert!(matches!(
            result,
            Err(LlmError::InvalidProviderPayload(
                "provider error body exceeds size limit"
            ))
        ));
    }

    #[tokio::test]
    async fn non_streaming_success_body_has_a_hard_cap() {
        let chunks = stream::iter([
            Ok::<_, io::Error>(Bytes::from(vec![b'a'; MAX_SUCCESS_BODY_BYTES])),
            Ok(Bytes::from_static(b"b")),
        ]);

        let result =
            read_body_stream(chunks, Duration::from_secs(1), ResponseBodyKind::Success).await;

        assert!(matches!(
            result,
            Err(LlmError::InvalidProviderPayload(
                "provider response body exceeds size limit"
            ))
        ));
    }
}
