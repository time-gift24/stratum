//! Scoped output ports for foundational agent-loop events.

mod definition;
mod error;
mod filesystem;
mod scoped;

pub use definition::{DurableEventSink, TelemetryEventSink};
pub use error::{DurableEventSinkError, FilesystemEventSinkError};
pub use filesystem::{EVENTS_FILE_NAME, FilesystemDurableEventSink, read_events};
pub use scoped::ScopedAgentEventSink;
