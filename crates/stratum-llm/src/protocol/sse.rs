//! Shared server-sent event parsing helpers.
use std::io;

use crate::LlmError;

/// Hard safety bound for one incomplete SSE event. A healthy long response
/// may contain any number of events; only an individual undelimited event is
/// bounded.
const MAX_SSE_EVENT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Default)]
pub(crate) struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    pub(crate) fn push(&mut self, mut chunk: &[u8]) -> Vec<Result<SseEvent, LlmError>> {
        let mut events = Vec::new();

        while !chunk.is_empty() {
            let Some(available) = MAX_SSE_EVENT_BYTES.checked_sub(self.buffer.len()) else {
                self.push_size_error(&mut events);
                return events;
            };
            if available == 0 {
                self.push_size_error(&mut events);
                return events;
            }
            let take = available.min(chunk.len());
            self.buffer.extend_from_slice(&chunk[..take]);
            chunk = &chunk[take..];

            while let Some((event_end, delimiter_len)) = event_delimiter(&self.buffer) {
                let Some(drain_end) = event_end.checked_add(delimiter_len) else {
                    self.push_size_error(&mut events);
                    return events;
                };
                let event = self.buffer[..event_end].to_vec();
                self.buffer.drain(..drain_end);

                match parse_sse_event(event) {
                    Ok(Some(event)) => events.push(Ok(event)),
                    Ok(None) => {}
                    Err(error) => {
                        self.buffer.clear();
                        events.push(Err(error));
                        return events;
                    }
                }
            }

            if self.buffer.len() == MAX_SSE_EVENT_BYTES {
                self.push_size_error(&mut events);
                return events;
            }
        }

        events
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }

    fn push_size_error(&mut self, events: &mut Vec<Result<SseEvent, LlmError>>) {
        self.buffer.clear();
        events.push(Err(LlmError::InvalidProviderPayload(
            "sse event exceeds size limit",
        )));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SseEvent {
    Data(String),
    Done,
}

fn event_delimiter(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|position| (position, 2));
    let crlf = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| (position, 4));

    match (lf, crlf) {
        (Some(lf), Some(crlf)) => Some(lf.min(crlf)),
        (Some(lf), None) => Some(lf),
        (None, Some(crlf)) => Some(crlf),
        (None, None) => None,
    }
}

fn parse_sse_event(event: Vec<u8>) -> Result<Option<SseEvent>, LlmError> {
    let text = String::from_utf8(event).map_err(LlmError::stream)?;
    let mut data_lines = Vec::new();

    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.strip_prefix(' ').unwrap_or(data).to_owned());
        }
    }

    if data_lines.is_empty() {
        return Ok(None);
    }

    let data = data_lines.join("\n");
    if data == "[DONE]" {
        return Ok(Some(SseEvent::Done));
    }

    Ok(Some(SseEvent::Data(data)))
}

pub(crate) fn stream_eof_error(message: &'static str) -> LlmError {
    LlmError::stream(io::Error::new(io::ErrorKind::UnexpectedEof, message))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undelimited_event_is_rejected_across_multiple_chunks_at_the_hard_cap() {
        let mut parser = SseParser::default();
        let first = vec![b'a'; MAX_SSE_EVENT_BYTES / 2];
        let second = vec![b'b'; MAX_SSE_EVENT_BYTES / 2];

        assert!(parser.push(&first).is_empty());
        let events = parser.push(&second);

        assert!(matches!(
            events.as_slice(),
            [Err(LlmError::InvalidProviderPayload(
                "sse event exceeds size limit"
            ))]
        ));
        assert!(!parser.has_pending());
    }

    #[test]
    fn total_stream_length_is_not_capped_when_each_event_is_delimited() {
        let mut parser = SseParser::default();

        for _ in 0..2048 {
            let events = parser.push(b"data: ok\n\n");
            assert!(matches!(
                events.as_slice(),
                [Ok(SseEvent::Data(data))] if data == "ok"
            ));
        }
        assert!(!parser.has_pending());
    }
}
