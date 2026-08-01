//! Tests for the unified schema validation boundary (OpenSpec `tool-input-validation`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use serde_json::json;
use stratum_core::{CallId, DangerLevel, ToolKind, ToolName, ToolSpec};
use stratum_filesystem::{LocalFilesystem, LocalFilesystemConfig};
use stratum_tools::{
    BuiltinToolRegistry, EchoTool, SearchTextTool, Tool, ToolError, ToolInput, ToolOutput,
    ToolRegistry,
};
use tokio_util::sync::CancellationToken;

/// Tool that delegates semantic validation and counts how often it is reached.
struct CountingTool {
    spec: ToolSpec,
    delegate: Arc<dyn Tool>,
    validate_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for CountingTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn validate(&self, input: &ToolInput) -> Result<(), ToolError> {
        self.validate_calls.fetch_add(1, Ordering::SeqCst);
        self.delegate.validate(input)
    }

    async fn call(
        &self,
        input: ToolInput,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        self.delegate.call(input, cancellation).await
    }
}

fn counting_tool(
    name: &str,
    input_schema: serde_json::Value,
    delegate: Arc<dyn Tool>,
) -> (Arc<CountingTool>, Arc<AtomicUsize>) {
    let validate_calls = Arc::new(AtomicUsize::new(0));
    let tool = Arc::new(CountingTool {
        spec: ToolSpec::builder()
            .name(name)
            .description("counting test tool")
            .input_schema(input_schema)
            .build(),
        delegate,
        validate_calls: Arc::clone(&validate_calls),
    });
    (tool, validate_calls)
}

fn counting_registry(
    name: &str,
    input_schema: serde_json::Value,
    delegate: Arc<dyn Tool>,
) -> (BuiltinToolRegistry, Arc<AtomicUsize>) {
    let (tool, validate_calls) = counting_tool(name, input_schema, delegate);
    let mut registry = BuiltinToolRegistry::default();
    registry
        .register(tool, ToolKind::Read, DangerLevel::Low)
        .expect("test tool should register");
    (registry, validate_calls)
}

#[tokio::test]
async fn schema_rejects_type_error_that_custom_validate_would_miss() {
    let echo = Arc::new(EchoTool::new());
    let (registry, validate_calls) = counting_registry(
        "counting_echo",
        json!({
            "type": "object",
            "required": ["message"],
            "properties": {"message": {"type": "string"}}
        }),
        echo.clone(),
    );
    let input = ToolInput::new(CallId::from("call-type"), json!({"message": 42}));
    assert!(
        echo.validate(&input).is_ok(),
        "echo's own validate only checks for an object and misses the wrong field type"
    );

    let error = registry
        .validate(&ToolName::from("counting_echo"), &input)
        .expect_err("schema must reject the wrong field type");

    assert!(
        matches!(
            error,
            ToolError::InvalidArgument {
                name: "arguments",
                ..
            }
        ),
        "schema rejection must be a typed InvalidArgument, got {error}"
    );
    assert!(
        error.to_string().contains("/message"),
        "schema error must name the failing field path, got {error}"
    );
    assert_eq!(
        validate_calls.load(Ordering::SeqCst),
        0,
        "custom validate must not run when the schema rejects the input"
    );
}

#[tokio::test]
async fn schema_rejects_missing_required_field_without_calling_custom_validate() {
    let (registry, validate_calls) = counting_registry(
        "counting_echo",
        json!({
            "type": "object",
            "required": ["message"],
            "properties": {"message": {"type": "string"}}
        }),
        Arc::new(EchoTool::new()),
    );

    let error = registry
        .validate(
            &ToolName::from("counting_echo"),
            &ToolInput::new(CallId::from("call-required"), json!({})),
        )
        .expect_err("schema must reject a missing required field");

    assert!(matches!(error, ToolError::InvalidArgument { .. }));
    assert!(
        error.to_string().contains("message"),
        "schema error must mention the missing field, got {error}"
    );
    assert_eq!(validate_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn schema_rejects_constraint_violation_without_calling_custom_validate() {
    let (registry, validate_calls) = counting_registry(
        "counting_echo",
        json!({
            "type": "object",
            "required": ["message"],
            "properties": {"message": {"type": "string", "minLength": 1}}
        }),
        Arc::new(EchoTool::new()),
    );
    let input = ToolInput::new(CallId::from("call-constraint"), json!({"message": ""}));

    let validate_error = registry
        .validate(&ToolName::from("counting_echo"), &input)
        .expect_err("schema must reject the minLength violation");
    let call_error = registry
        .call(
            &ToolName::from("counting_echo"),
            input,
            &CancellationToken::new(),
        )
        .await
        .expect_err("direct calls must hit the same schema boundary");

    assert!(matches!(validate_error, ToolError::InvalidArgument { .. }));
    assert_eq!(validate_error.to_string(), call_error.to_string());
    assert_eq!(validate_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn custom_validate_still_runs_after_schema_passes() {
    let root = std::env::temp_dir().join(format!(
        "stratum-tools-schema-validation-{}",
        std::process::id()
    ));
    tokio::fs::create_dir_all(&root).await.expect("create root");
    let filesystem = Arc::new(
        LocalFilesystem::new(LocalFilesystemConfig {
            root: root.clone(),
            max_file_bytes: Some(4096),
        })
        .expect("filesystem is valid"),
    );
    let (registry, validate_calls) = counting_registry(
        "counting_search",
        json!({"type": "object"}),
        Arc::new(SearchTextTool::new(filesystem)),
    );

    let error = registry
        .validate(
            &ToolName::from("counting_search"),
            &ToolInput::new(
                CallId::from("call-custom"),
                json!({"path": "src", "query": ""}),
            ),
        )
        .expect_err("custom validate must still reject semantically invalid input");

    assert_eq!(
        error.to_string(),
        "invalid argument query: must not be empty"
    );
    assert_eq!(
        validate_calls.load(Ordering::SeqCst),
        1,
        "schema-valid input must reach the tool's custom validate"
    );

    let _ = tokio::fs::remove_dir_all(root).await;
}

#[tokio::test]
async fn schema_valid_input_executes_through_registry() {
    let (registry, validate_calls) = counting_registry(
        "counting_echo",
        json!({
            "type": "object",
            "required": ["message"],
            "properties": {"message": {"type": "string"}}
        }),
        Arc::new(EchoTool::new()),
    );

    let input = ToolInput::new(CallId::from("call-valid"), json!({"message": "hello"}));
    registry
        .validate(&ToolName::from("counting_echo"), &input)
        .expect("schema-valid input should pass validation");

    let output = registry
        .call(
            &ToolName::from("counting_echo"),
            input,
            &CancellationToken::new(),
        )
        .await
        .expect("schema-valid input should execute");

    assert_eq!(output.result, json!({"message": "hello"}));
    assert_eq!(validate_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_input_schema_is_rejected_at_registration() {
    let (tool, _) = counting_tool(
        "broken_schema",
        json!({"type": "not_a_real_type"}),
        Arc::new(EchoTool::new()),
    );
    let mut registry = BuiltinToolRegistry::default();

    let error = registry
        .register(tool, ToolKind::Read, DangerLevel::Low)
        .expect_err("an uncompilable input schema must fail registration");

    assert!(
        matches!(
            error,
            ToolError::InvalidInputSchema { ref name, .. } if name == &ToolName::from("broken_schema")
        ),
        "registration failure must be a typed InvalidInputSchema, got {error}"
    );
    assert!(
        registry.get(&ToolName::from("broken_schema")).is_none(),
        "a tool with an invalid schema must not enter the registry"
    );
    assert!(
        registry.specs().is_empty(),
        "rejected tools must not appear in provider-visible specs"
    );
}
