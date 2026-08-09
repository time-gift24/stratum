//! Explicit operational timeout policy for provider egress.

use std::time::Duration;

/// Time bounds applied by one LLM provider client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LlmTimeouts {
    connect: Duration,
    request: Duration,
    first_response: Duration,
    stream_idle: Duration,
}

impl LlmTimeouts {
    /// Creates an explicit provider timeout policy.
    #[must_use]
    pub const fn new(
        connect: Duration,
        request: Duration,
        first_response: Duration,
        stream_idle: Duration,
    ) -> Self {
        Self {
            connect,
            request,
            first_response,
            stream_idle,
        }
    }

    /// TCP connect timeout.
    #[must_use]
    pub const fn connect(self) -> Duration {
        self.connect
    }

    /// Total non-streaming request or streaming error-envelope timeout.
    #[must_use]
    pub const fn request(self) -> Duration {
        self.request
    }

    /// Maximum wait for streaming response headers.
    #[must_use]
    pub const fn first_response(self) -> Duration {
        self.first_response
    }

    /// Maximum silence between streaming response body chunks.
    #[must_use]
    pub const fn stream_idle(self) -> Duration {
        self.stream_idle
    }
}
