//! Public tool traits and execution types.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use stratum_core::{CallId, DangerLevel, ToolKind, ToolName, ToolSetFingerprint, ToolSpec};
use tokio_util::sync::CancellationToken;

use crate::ToolError;

/// Registry-wide tool permission behavior.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolPermissionMode {
    #[default]
    Allow,
    PartialAllow,
    RequireApproval,
}

/// Input passed to one runtime tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolInput {
    /// Provider call identity.
    pub call_id: CallId,
    /// Parsed tool arguments.
    pub arguments: Value,
}

impl ToolInput {
    /// Creates tool input from a provider call id and parsed arguments.
    #[must_use]
    pub const fn new(call_id: CallId, arguments: Value) -> Self {
        Self { call_id, arguments }
    }
}

/// Output returned by one runtime tool call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ToolOutput {
    /// Tool result payload.
    pub result: Value,
}

impl ToolOutput {
    /// Creates tool output from a result payload.
    #[must_use]
    pub const fn new(result: Value) -> Self {
        Self { result }
    }
}

/// Runtime tool implementation.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Returns the provider-visible tool specification.
    fn spec(&self) -> &ToolSpec;

    /// Validates every deterministic input condition before execution is authorized.
    ///
    /// Validation is synchronous and side-effect free. Callers use it before approval and before
    /// recording that execution started; [`Tool::call`] must still reject the same invalid input
    /// when invoked directly.
    ///
    /// # Errors
    ///
    /// Returns a tool error when the input cannot be executed as supplied.
    fn validate(&self, input: &ToolInput) -> Result<(), ToolError>;

    /// Returns the runtime implementation identity included in turn snapshots.
    fn implementation_id(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Executes the tool.
    ///
    /// Cancellation is cooperative. Implementations should stop before starting new
    /// external work, but operations already issued are not guaranteed to be rolled back.
    ///
    /// # Errors
    ///
    /// Returns a tool error when execution fails.
    async fn call(
        &self,
        input: ToolInput,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError>;
}

/// Registry of pre-populated runtime tools.
#[async_trait]
pub trait ToolRegistry: Send + Sync {
    /// Registers a tool by its provider-visible name.
    ///
    /// # Errors
    ///
    /// Returns an error when another tool with the same name is already registered.
    fn register(
        &mut self,
        tool: Arc<dyn Tool>,
        tool_kind: ToolKind,
        danger_level: DangerLevel,
    ) -> Result<(), ToolError>;

    /// Returns approval metadata for a registered tool, or `None` when it is allowed.
    ///
    /// # Errors
    ///
    /// Returns an error when the tool is not registered.
    fn authorization(&self, name: &ToolName) -> Result<Option<(ToolKind, DangerLevel)>, ToolError>;

    /// Validates input for a registered tool without starting external work.
    ///
    /// # Errors
    ///
    /// Returns an error when the tool is missing or the input is invalid.
    fn validate(&self, name: &ToolName, input: &ToolInput) -> Result<(), ToolError>;

    /// Returns a registered tool by name.
    fn get(&self, name: &ToolName) -> Option<Arc<dyn Tool>>;

    /// Returns provider-visible specs for all registered tools.
    fn specs(&self) -> Vec<ToolSpec>;

    /// Computes the exact ordered tool-set fingerprint used by resumable turns.
    ///
    /// # Errors
    ///
    /// Returns an error if registry metadata is inconsistent or cannot be encoded.
    fn fingerprint(&self) -> Result<ToolSetFingerprint, ToolError> {
        #[derive(Serialize)]
        struct FingerprintEntry {
            spec: ToolSpec,
            authorization: Option<(ToolKind, DangerLevel)>,
            implementation_id: &'static str,
        }

        let entries = self
            .specs()
            .into_iter()
            .map(|spec| {
                let authorization = self.authorization(&spec.name)?;
                let implementation_id = self
                    .get(&spec.name)
                    .ok_or_else(|| ToolError::ToolNotFound {
                        name: spec.name.clone(),
                    })?
                    .implementation_id();
                Ok(FingerprintEntry {
                    spec,
                    authorization,
                    implementation_id,
                })
            })
            .collect::<Result<Vec<_>, ToolError>>()?;
        let encoded = serde_json::to_vec(&entries)
            .map_err(|source| ToolError::FingerprintEncoding { source })?;
        let digest = Sha256::digest(encoded);
        let canonical = digest
            .iter()
            .fold(String::with_capacity(64), |mut output, byte| {
                use std::fmt::Write as _;
                write!(output, "{byte:02x}").expect("writing to a string cannot fail");
                output
            });
        Ok(canonical
            .parse()
            .expect("sha-256 output is a canonical fingerprint"))
    }

    /// Executes a registered tool by name.
    ///
    /// Cancellation is cooperative. Implementations should stop before starting new
    /// external work, but operations already issued are not guaranteed to be rolled back.
    ///
    /// # Errors
    ///
    /// Returns an error when the tool is missing or execution fails.
    async fn call(
        &self,
        name: &ToolName,
        input: ToolInput,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BuiltinToolRegistry;
    use serde_json::json;
    struct CancellationAwareTool {
        spec: ToolSpec,
    }

    #[async_trait]
    impl Tool for CancellationAwareTool {
        fn spec(&self) -> &ToolSpec {
            &self.spec
        }

        fn validate(&self, _input: &ToolInput) -> Result<(), ToolError> {
            Ok(())
        }

        async fn call(
            &self,
            _input: ToolInput,
            cancellation: &CancellationToken,
        ) -> Result<ToolOutput, ToolError> {
            assert!(cancellation.is_cancelled());
            Ok(ToolOutput::new(json!({"cancelled": true})))
        }
    }

    #[tokio::test]
    async fn cancellation_token_reaches_tool() {
        let tool = Arc::new(CancellationAwareTool {
            spec: ToolSpec::builder()
                .name("cancellation_aware")
                .description("observes cancellation")
                .input_schema(json!({"type": "object"}))
                .build(),
        });
        let name = tool.spec().name.clone();
        let mut registry = BuiltinToolRegistry::default();
        registry
            .register(tool, ToolKind::Read, DangerLevel::Low)
            .expect("test tool should register");
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let output = registry
            .call(
                &name,
                ToolInput::new(CallId::new("call-1"), json!({})),
                &cancellation,
            )
            .await
            .expect("test tool should return output");

        assert_eq!(output.result, json!({"cancelled": true}));
    }
}
