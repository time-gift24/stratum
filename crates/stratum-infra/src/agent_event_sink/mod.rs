//! Scoped output ports for foundational agent-loop events.

mod definition;
mod error;
mod filesystem;
mod scoped;

pub use definition::{DurableEventSink, TelemetryEventSink};
pub use error::{DurableEventSinkError, FilesystemEventSinkError};
pub use filesystem::{
    COMPACT_INDEX_FILE_NAME, CompactionCheckpoint, EVENTS_FILE_NAME, FilesystemDurableEventSink,
    read_events, read_events_from_checkpoint,
};
pub use scoped::ScopedAgentEventSink;
