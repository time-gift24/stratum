//! Output port contracts for foundational agent-loop events.

mod definition;
mod error;

pub use definition::{DurableEventSink, TelemetryEventSink};
pub use error::DurableEventSinkError;
