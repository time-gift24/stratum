//! Agent store persistence contract for Stratum runtimes.

mod definition;
mod error;
mod state;

pub use definition::AgentStore;
pub use error::StoreError;
pub use state::{AGENT_STATE_VERSION, AgentState, AgentStatus, MAX_HISTORY_PAGE_SIZE};
