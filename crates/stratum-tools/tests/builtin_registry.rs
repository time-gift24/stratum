use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use stratum_core::{CallId, DangerLevel, ToolKind, ToolName, ToolSpec};
use stratum_filesystem::{Filesystem, LocalFilesystem, LocalFilesystemConfig, VirtualPath};
use stratum_tools::{
    ApplyPatchTool, BuiltinToolRegistry, Tool, ToolError, ToolInput, ToolOutput,
    ToolPermissionMode, ToolRegistry,
};
use tokio_util::sync::CancellationToken;

struct TestTool {
    spec: ToolSpec,
}

impl TestTool {
    fn new(name: &str) -> Self {
        Self {
            spec: ToolSpec::builder()
                .name(name)
                .description("registry test tool")
                .input_schema(json!({"type": "object"}))
                .build(),
        }
    }
}

#[async_trait]
impl Tool for TestTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn validate(&self, input: &ToolInput) -> Result<(), ToolError> {
        if input.arguments.is_object() {
            Ok(())
        } else {
            Err(ToolError::InvalidArgument {
                name: "arguments",
                reason: "must be an object".into(),
            })
        }
    }

    async fn call(
        &self,
        input: ToolInput,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        self.validate(&input)?;
        if cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }
        Ok(ToolOutput::new(input.arguments))
    }
}

async fn test_filesystem(name: &str) -> (Arc<LocalFilesystem>, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "stratum-tools-apply-patch-{name}-{}",
        std::process::id()
    ));
    let _ = tokio::fs::remove_dir_all(&root).await;
    tokio::fs::create_dir_all(&root).await.expect("create root");
    let filesystem = Arc::new(
        LocalFilesystem::new(LocalFilesystemConfig {
            root: root.clone(),
            max_file_bytes: Some(4096),
        })
        .expect("filesystem is valid"),
    );
    (filesystem, root)
}

fn apply_patch_registry(filesystem: Arc<LocalFilesystem>) -> BuiltinToolRegistry {
    let mut registry = BuiltinToolRegistry::default();
    registry
        .register(
            Arc::new(ApplyPatchTool::new(filesystem)),
            ToolKind::Write,
            DangerLevel::High,
        )
        .expect("apply patch should register");
    registry
}

#[tokio::test]
async fn apply_patch_validation_is_side_effect_free_and_matches_execution() {
    let (filesystem, root) = test_filesystem("validation").await;
    let registry = apply_patch_registry(filesystem);
    let name = ToolName::from("apply_patch");
    let cases = [
        json!({}),
        json!({"operation": {"type": "rename_file", "path": "notes.txt"}}),
        json!({"operation": {"type": "delete_file", "path": "../secret"}}),
        json!({"operation": {"type": "create_file", "path": "notes.txt"}}),
    ];

    for arguments in cases {
        let input = ToolInput::new(CallId::from("call-invalid"), arguments);
        let validation_error = registry
            .validate(&name, &input)
            .expect_err("invalid input should fail validation");
        let call_error = registry
            .call(&name, input, &CancellationToken::new())
            .await
            .expect_err("invalid input should fail execution");
        assert_eq!(validation_error.to_string(), call_error.to_string());
    }

    assert!(
        tokio::fs::read_dir(&root)
            .await
            .expect("validation root should remain readable")
            .next_entry()
            .await
            .expect("reading validation root should succeed")
            .is_none(),
        "validation must not perform filesystem work"
    );
    tokio::fs::remove_dir_all(root)
        .await
        .expect("validation root is removed");
}

#[tokio::test]
async fn apply_patch_schema_requires_diff_only_for_create_and_update() {
    let (filesystem, root) = test_filesystem("schema").await;
    let registry = apply_patch_registry(filesystem);
    let spec = registry
        .specs()
        .into_iter()
        .find(|spec| spec.name == ToolName::from("apply_patch"))
        .expect("apply patch spec should be registered");
    let operation_schema = &spec.input_schema["properties"]["operation"];

    assert_eq!(operation_schema["required"], json!(["type", "path"]));
    assert_eq!(
        operation_schema["allOf"],
        json!([{
            "if": {
                "properties": {
                    "type": {"enum": ["create_file", "update_file"]}
                },
                "required": ["type"]
            },
            "then": {"required": ["diff"]}
        }])
    );

    tokio::fs::remove_dir_all(root)
        .await
        .expect("schema root is removed");
}

#[tokio::test]
async fn apply_patch_creates_updates_and_deletes_a_file() {
    let (filesystem, root) = test_filesystem("lifecycle").await;
    let registry = apply_patch_registry(Arc::clone(&filesystem));
    let name = ToolName::from("apply_patch");
    let cancellation = CancellationToken::new();

    let created = registry
        .call(
            &name,
            ToolInput::new(
                CallId::from("call-create"),
                json!({
                    "operation": {
                        "type": "create_file",
                        "path": "notes.txt",
                        "diff": "@@\n+one\n+two\n"
                    }
                }),
            ),
            &cancellation,
        )
        .await
        .expect("create should run");
    assert_eq!(created.result["output"], "created notes.txt");

    let updated = registry
        .call(
            &name,
            ToolInput::new(
                CallId::from("call-update"),
                json!({
                    "operation": {
                        "type": "update_file",
                        "path": "notes.txt",
                        "diff": "@@\n one\n-two\n+deux\n"
                    }
                }),
            ),
            &cancellation,
        )
        .await
        .expect("update should run");
    assert_eq!(updated.result["output"], "updated notes.txt");

    let path = VirtualPath::try_from("/notes.txt").expect("path is valid");
    assert_eq!(
        filesystem.read_file(&path).await.expect("read file"),
        b"one\ndeux\n"
    );

    let deleted = registry
        .call(
            &name,
            ToolInput::new(
                CallId::from("call-delete"),
                json!({"operation": {"type": "delete_file", "path": "notes.txt"}}),
            ),
            &cancellation,
        )
        .await
        .expect("delete should run");
    assert_eq!(deleted.result["output"], "deleted notes.txt");
    assert!(matches!(
        filesystem
            .read_file(&path)
            .await
            .expect_err("file should be gone"),
        stratum_filesystem::FilesystemError::NotFound { .. }
    ));

    tokio::fs::remove_dir_all(root)
        .await
        .expect("lifecycle root is removed");
}

#[tokio::test]
async fn apply_patch_returns_actionable_failure_output_without_mutating_files() {
    let (filesystem, root) = test_filesystem("failure").await;
    let path = VirtualPath::try_from("/notes.txt").expect("path is valid");
    filesystem
        .write_file(&path, b"one\ntwo\n".to_vec())
        .await
        .expect("seed file");
    let registry = apply_patch_registry(Arc::clone(&filesystem));

    let conflict = registry
        .call(
            &ToolName::from("apply_patch"),
            ToolInput::new(
                CallId::from("call-conflict"),
                json!({
                    "operation": {
                        "type": "update_file",
                        "path": "notes.txt",
                        "diff": "@@\n missing\n-two\n+deux\n"
                    }
                }),
            ),
            &CancellationToken::new(),
        )
        .await
        .expect("conflict remains a model-visible result");
    assert_eq!(conflict.result["status"], "failed");
    assert_eq!(
        conflict.result["output"],
        "patch context did not match notes.txt"
    );
    assert_eq!(
        filesystem.read_file(&path).await.expect("read original"),
        b"one\ntwo\n"
    );

    let exists = registry
        .call(
            &ToolName::from("apply_patch"),
            ToolInput::new(
                CallId::from("call-exists"),
                json!({
                    "operation": {
                        "type": "create_file",
                        "path": "notes.txt",
                        "diff": "@@\n+replacement\n"
                    }
                }),
            ),
            &CancellationToken::new(),
        )
        .await
        .expect("existing file remains a model-visible result");
    assert_eq!(exists.result["status"], "failed");
    assert_eq!(
        exists.result["output"],
        "file already exists at path 'notes.txt'"
    );

    tokio::fs::remove_dir_all(root)
        .await
        .expect("failure root is removed");
}

#[tokio::test]
async fn cancelled_apply_patch_preserves_existing_content() {
    let (filesystem, root) = test_filesystem("cancelled").await;
    let path = VirtualPath::try_from("/notes.txt").expect("path is valid");
    filesystem
        .write_file(&path, b"original\n".to_vec())
        .await
        .expect("seed file");
    let registry = apply_patch_registry(Arc::clone(&filesystem));
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = registry
        .call(
            &ToolName::from("apply_patch"),
            ToolInput::new(
                CallId::from("call-cancelled"),
                json!({
                    "operation": {
                        "type": "update_file",
                        "path": "notes.txt",
                        "diff": "@@\n-original\n+changed\n"
                    }
                }),
            ),
            &cancellation,
        )
        .await
        .expect_err("cancelled patch should not run");

    assert!(matches!(error, ToolError::Cancelled));
    assert_eq!(
        filesystem.read_file(&path).await.expect("read original"),
        b"original\n"
    );
    tokio::fs::remove_dir_all(root)
        .await
        .expect("cancelled root is removed");
}

#[tokio::test]
async fn registered_test_tool_executes_through_the_registry() {
    let name = ToolName::from("test_tool");
    let mut registry = BuiltinToolRegistry::default();
    registry
        .register(
            Arc::new(TestTool::new(name.as_str())),
            ToolKind::Read,
            DangerLevel::Low,
        )
        .expect("test tool should register");

    let output = registry
        .call(
            &name,
            ToolInput::new(CallId::from("call-test-tool"), json!({"message": "hello"})),
            &CancellationToken::new(),
        )
        .await
        .expect("test tool should execute");
    assert_eq!(output.result, json!({"message": "hello"}));
}

#[test]
fn permission_modes_apply_the_declared_matrix() {
    let cases = [
        (
            ToolPermissionMode::Allow,
            ToolKind::Write,
            DangerLevel::High,
            false,
        ),
        (
            ToolPermissionMode::PartialAllow,
            ToolKind::Read,
            DangerLevel::Low,
            false,
        ),
        (
            ToolPermissionMode::PartialAllow,
            ToolKind::Read,
            DangerLevel::Medium,
            true,
        ),
        (
            ToolPermissionMode::PartialAllow,
            ToolKind::Write,
            DangerLevel::Low,
            true,
        ),
        (
            ToolPermissionMode::RequireApproval,
            ToolKind::Read,
            DangerLevel::Low,
            true,
        ),
    ];

    for (mode, kind, danger_level, expects_approval) in cases {
        let name = ToolName::from("test_tool");
        let mut registry = BuiltinToolRegistry::new(mode);
        registry
            .register(Arc::new(TestTool::new(name.as_str())), kind, danger_level)
            .expect("test tool should register");
        assert_eq!(
            registry
                .authorization(&name)
                .expect("test tool is registered")
                .is_some(),
            expects_approval
        );
    }
}

#[tokio::test]
async fn duplicate_and_missing_tools_have_typed_errors() {
    let name = ToolName::from("test_tool");
    let mut registry = BuiltinToolRegistry::default();
    registry
        .register(
            Arc::new(TestTool::new(name.as_str())),
            ToolKind::Read,
            DangerLevel::Low,
        )
        .expect("first test tool should register");
    let duplicate = registry
        .register(
            Arc::new(TestTool::new(name.as_str())),
            ToolKind::Read,
            DangerLevel::Low,
        )
        .expect_err("duplicate registration should fail");
    assert!(matches!(
        duplicate,
        ToolError::DuplicateTool { name: ref duplicate_name } if duplicate_name == &name
    ));

    let missing = ToolName::from("missing");
    let error = registry
        .call(
            &missing,
            ToolInput::new(CallId::from("call-missing"), json!({})),
            &CancellationToken::new(),
        )
        .await
        .expect_err("missing tool should fail");
    assert!(matches!(
        error,
        ToolError::ToolNotFound { ref name } if name == &missing
    ));
}

#[test]
fn fingerprint_includes_authorization_policy() {
    let name = ToolName::from("test_tool");
    let mut allowed = BuiltinToolRegistry::new(ToolPermissionMode::Allow);
    allowed
        .register(
            Arc::new(TestTool::new(name.as_str())),
            ToolKind::Read,
            DangerLevel::Low,
        )
        .expect("test tool should register");
    let first = allowed.fingerprint().expect("fingerprint computes");
    let second = allowed.fingerprint().expect("fingerprint recomputes");

    let mut approval = BuiltinToolRegistry::new(ToolPermissionMode::RequireApproval);
    approval
        .register(
            Arc::new(TestTool::new(name.as_str())),
            ToolKind::Read,
            DangerLevel::Low,
        )
        .expect("test tool should register");

    assert_eq!(first, second);
    assert_ne!(first, approval.fingerprint().expect("fingerprint computes"));
}
