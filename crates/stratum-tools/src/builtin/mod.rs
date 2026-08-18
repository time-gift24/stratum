//! Builtin tool implementations.

mod apply_patch;
mod shell;

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
use stratum_core::{DangerLevel, ToolKind, ToolName, ToolSpec};
use tokio_util::sync::CancellationToken;

use crate::schema_validation;
use crate::{Tool, ToolError, ToolInput, ToolOutput, ToolPermissionMode, ToolRegistry};

pub use apply_patch::ApplyPatchTool;
pub use shell::ShellTool;

/// Registry backed by builtin in-memory tools.
struct RegisteredTool {
    tool: Arc<dyn Tool>,
    tool_kind: ToolKind,
    danger_level: DangerLevel,
    input_schema: jsonschema::Validator,
}

pub struct BuiltinToolRegistry {
    tools: BTreeMap<ToolName, RegisteredTool>,
    permission_mode: ToolPermissionMode,
}

impl BuiltinToolRegistry {
    /// Creates a builtin registry with the requested permission behavior.
    #[must_use]
    pub fn new(permission_mode: ToolPermissionMode) -> Self {
        Self {
            tools: BTreeMap::new(),
            permission_mode,
        }
    }
}

impl Default for BuiltinToolRegistry {
    fn default() -> Self {
        Self::new(ToolPermissionMode::Allow)
    }
}

#[async_trait]
impl ToolRegistry for BuiltinToolRegistry {
    fn register(
        &mut self,
        tool: Arc<dyn Tool>,
        tool_kind: ToolKind,
        danger_level: DangerLevel,
    ) -> Result<(), ToolError> {
        let name = tool.spec().name.clone();
        if self.tools.contains_key(&name) {
            return Err(ToolError::DuplicateTool { name });
        }
        let input_schema =
            schema_validation::compile_input_schema(&name, &tool.spec().input_schema)?;

        self.tools.insert(
            name,
            RegisteredTool {
                tool,
                tool_kind,
                danger_level,
                input_schema,
            },
        );
        Ok(())
    }

    fn authorization(&self, name: &ToolName) -> Result<Option<(ToolKind, DangerLevel)>, ToolError> {
        let registered = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::ToolNotFound { name: name.clone() })?;
        let allowed = match self.permission_mode {
            ToolPermissionMode::Allow => true,
            ToolPermissionMode::PartialAllow => {
                registered.tool_kind == ToolKind::Read
                    && registered.danger_level == DangerLevel::Low
            }
            ToolPermissionMode::RequireApproval => false,
        };
        Ok((!allowed).then_some((registered.tool_kind, registered.danger_level)))
    }

    fn validate(&self, name: &ToolName, input: &ToolInput) -> Result<(), ToolError> {
        let registered = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::ToolNotFound { name: name.clone() })?;
        schema_validation::validate_against_schema(&registered.input_schema, input)?;
        registered.tool.validate(input)
    }

    fn get(&self, name: &ToolName) -> Option<Arc<dyn Tool>> {
        self.tools
            .get(name)
            .map(|registered| Arc::clone(&registered.tool))
    }

    fn specs(&self) -> Vec<ToolSpec> {
        self.tools
            .values()
            .map(|registered| registered.tool.spec().clone())
            .collect()
    }

    async fn call(
        &self,
        name: &ToolName,
        input: ToolInput,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let registered = self
            .tools
            .get(name)
            .ok_or_else(|| ToolError::ToolNotFound { name: name.clone() })?;
        schema_validation::validate_against_schema(&registered.input_schema, &input)?;
        let tool = Arc::clone(&registered.tool);

        tool.call(input, cancellation).await
    }
}
