//! Structured output request types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum StructuredOutput {
    JsonObject,
    JsonSchema {
        name: String,
        schema: Value,
        strict: bool,
    },
}
