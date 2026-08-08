//! Infrastructure primitives for Stratum runtimes.
//!
//! The retained surface is narrow: the kernel sink contracts
//! ([`DurableEventSink`], [`TelemetryEventSink`]) and the concrete Agent-scoped
//! NATS tail transport ([`NatsAgentTail`]).

pub mod agent_event_sink;
pub mod agent_tail;

pub use agent_event_sink::{DurableEventSink, DurableEventSinkError, TelemetryEventSink};
pub use agent_tail::{
    AgentTailConfig, AgentTailError, AgentTailStream, NatsAgentTail, TailCursor,
    TailCursorParseError,
};
