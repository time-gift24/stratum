//! Unified JSON Schema validation boundary for tool input.
//!
//! Every registered tool compiles its [`ToolSpec::input_schema`] once at registration time and
//! the registry validates `ToolInput.arguments` against the compiled schema before any
//! tool-specific semantic validation runs.

use serde_json::Value;
use stratum_core::ToolName;

use crate::{ToolError, ToolInput};

/// Compiles a tool input schema for reuse across validation calls.
///
/// The schema is checked against its JSON Schema meta-schema first, so documents that are not
/// valid schemas are rejected here instead of failing unpredictably at validation time.
///
/// # Errors
///
/// Returns [`ToolError::InvalidInputSchema`] when the schema is not a valid JSON Schema document
/// or cannot be compiled.
pub(crate) fn compile_input_schema(
    name: &ToolName,
    schema: &Value,
) -> Result<jsonschema::Validator, ToolError> {
    jsonschema::meta::validate(schema).map_err(|error| ToolError::InvalidInputSchema {
        name: name.clone(),
        reason: error.to_string(),
    })?;
    jsonschema::validator_for(schema).map_err(|error| ToolError::InvalidInputSchema {
        name: name.clone(),
        reason: error.to_string(),
    })
}

/// Validates tool input against a compiled input schema.
///
/// # Errors
///
/// Returns [`ToolError::InvalidArgument`] naming the failing instance path when the input does
/// not satisfy the schema.
pub(crate) fn validate_against_schema(
    validator: &jsonschema::Validator,
    input: &ToolInput,
) -> Result<(), ToolError> {
    let error = match validator.validate(&input.arguments) {
        Ok(()) => return Ok(()),
        Err(error) => error,
    };
    let instance_path = error.instance_path().to_string();
    // The default `Display` embeds the full failing instance, which can carry
    // entire file contents or credentials back to the model and into logs.
    // Mask it and keep only the instance path plus the violation category.
    let category = error.masked().to_string();
    let reason = if instance_path.is_empty() {
        category
    } else {
        format!("{instance_path}: {category}")
    };
    Err(ToolError::InvalidArgument {
        name: "arguments",
        reason: reason.into(),
    })
}
