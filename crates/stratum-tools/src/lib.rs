//! Runtime tool abstractions and builtin tools for Stratum.

pub mod builtin;
pub mod definition;
pub mod error;
mod schema_validation;

pub use builtin::{ApplyPatchTool, BuiltinToolRegistry, ShellTool};
pub use definition::{Tool, ToolInput, ToolOutput, ToolPermissionMode, ToolRegistry};
pub use error::ToolError;
