//! Infrastructure primitives for Stratum runtimes.
//!
//! The retained surface is narrow: the kernel sink contracts
//! ([`DurableEventSink`], [`TelemetryEventSink`]) and the concrete
//! AgentRuntime-scoped NATS tail transport ([`NatsAgentRuntimeTail`]).

pub mod agent_event_sink;
pub mod agent_runtime_tail;

pub use agent_event_sink::{DurableEventSink, DurableEventSinkError, TelemetryEventSink};
pub use agent_runtime_tail::{
    AgentRuntimeTailConfig, AgentRuntimeTailCursor, AgentRuntimeTailCursorParseError,
    AgentRuntimeTailError, AgentRuntimeTailStream, NatsAgentRuntimeTail,
};
