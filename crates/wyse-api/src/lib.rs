//! HTTP API host for persisted Wyse agents.

mod api;
mod error;
mod host;

pub use error::HostError;
pub use host::{HostState, HostedAgent};
