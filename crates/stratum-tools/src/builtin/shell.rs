//! Builtin one-shot shell tool.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use stratum_core::ToolSpec;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{Tool, ToolError, ToolInput, ToolOutput};

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const OUTPUT_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);

/// Executes each command in a fresh, non-persistent Bash process.
pub struct ShellTool {
    working_directory: PathBuf,
    spec: ToolSpec,
}

impl ShellTool {
    /// Creates a shell tool rooted at the supplied default working directory.
    #[must_use]
    pub fn new(working_directory: PathBuf) -> Self {
        Self {
            working_directory,
            spec: ToolSpec::builder()
                .name("shell")
                .description(
                    "executes a command in a fresh, non-persistent Bash process and returns its complete stdout, stderr, and exit status",
                )
                .input_schema(json!({
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["command"],
                    "properties": {
                        "command": {
                            "type": "string",
                            "minLength": 1
                        },
                        "workdir": { "type": "string" },
                        "timeout_ms": {
                            "type": "integer",
                            "minimum": 1
                        }
                    }
                }))
                .build(),
        }
    }
}

#[async_trait]
impl Tool for ShellTool {
    fn spec(&self) -> &ToolSpec {
        &self.spec
    }

    fn validate(&self, input: &ToolInput) -> Result<(), ToolError> {
        parse_input(input.arguments.clone()).map(|_| ())
    }

    async fn call(
        &self,
        input: ToolInput,
        cancellation: &CancellationToken,
    ) -> Result<ToolOutput, ToolError> {
        let input = parse_input(input.arguments)?;
        if cancellation.is_cancelled() {
            return Err(ToolError::Cancelled);
        }

        let workdir = resolve_workdir(&self.working_directory, input.workdir.as_deref());
        let mut command = Command::new("bash");
        command
            .arg("-c")
            .arg(input.command)
            .current_dir(workdir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        #[cfg(unix)]
        command.process_group(0);

        let mut child = command
            .spawn()
            .map_err(|source| process_error("spawn", source))?;
        #[cfg(unix)]
        let process_group = child.id().ok_or_else(|| {
            process_error(
                "read process id",
                io::Error::other("spawned shell has no process id"),
            )
        })?;
        #[cfg(unix)]
        let mut process_group_guard = ProcessGroupGuard::new(process_group);
        let stdout = child.stdout.take().ok_or_else(|| {
            process_error(
                "open stdout pipe",
                io::Error::other("spawned shell stdout pipe is unavailable"),
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            process_error(
                "open stderr pipe",
                io::Error::other("spawned shell stderr pipe is unavailable"),
            )
        })?;
        let timeout = Duration::from_millis(input.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS));
        let mut stdout = tokio::spawn(read_all(stdout));
        let mut stderr = tokio::spawn(read_all(stderr));
        let outcome = wait_for_exit(
            &mut child,
            timeout,
            cancellation,
            #[cfg(unix)]
            process_group,
        );
        let outcome = outcome.await?;
        #[cfg(unix)]
        process_group_guard.disarm();
        let (stdout, stderr) = collect_output(&mut stdout, &mut stderr).await?;
        if outcome.cancelled {
            return Err(ToolError::Cancelled);
        }

        Ok(ToolOutput::new(json!({
            "stdout": String::from_utf8_lossy(&stdout),
            "stderr": String::from_utf8_lossy(&stderr),
            "exit_code": outcome.status.code(),
            "signal": exit_signal(&outcome.status),
            "timed_out": outcome.timed_out,
        })))
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShellInput {
    command: String,
    workdir: Option<PathBuf>,
    timeout_ms: Option<u64>,
}

fn parse_input(arguments: serde_json::Value) -> Result<ShellInput, ToolError> {
    let input: ShellInput =
        serde_json::from_value(arguments).map_err(|source| ToolError::InvalidInput { source })?;
    if input.command.is_empty() {
        return Err(ToolError::InvalidArgument {
            name: "command",
            reason: "must not be empty".into(),
        });
    }
    if input.timeout_ms == Some(0) {
        return Err(ToolError::InvalidArgument {
            name: "timeout_ms",
            reason: "must be greater than zero".into(),
        });
    }
    Ok(input)
}

fn resolve_workdir(default: &Path, requested: Option<&Path>) -> PathBuf {
    match requested {
        Some(path) if path.is_absolute() => path.to_path_buf(),
        Some(path) => default.join(path),
        None => default.to_path_buf(),
    }
}

#[derive(Debug)]
struct ProcessOutcome {
    status: ExitStatus,
    timed_out: bool,
    cancelled: bool,
}

async fn wait_for_exit(
    child: &mut Child,
    timeout: Duration,
    cancellation: &CancellationToken,
    #[cfg(unix)] process_group: u32,
) -> Result<ProcessOutcome, ToolError> {
    enum Reason {
        Exited(ExitStatus),
        TimedOut,
        Cancelled,
    }

    let reason = tokio::select! {
        biased;
        status = child.wait() => Reason::Exited(
            status.map_err(|source| process_error("wait", source))?
        ),
        () = cancellation.cancelled() => Reason::Cancelled,
        () = tokio::time::sleep(timeout) => Reason::TimedOut,
    };

    match reason {
        Reason::Exited(status) => {
            terminate_remaining_processes(
                child,
                #[cfg(unix)]
                process_group,
            )
            .await?;
            Ok(ProcessOutcome {
                status,
                timed_out: false,
                cancelled: false,
            })
        }
        Reason::TimedOut | Reason::Cancelled => {
            terminate_processes(
                child,
                #[cfg(unix)]
                process_group,
            )
            .await?;
            let status = child
                .wait()
                .await
                .map_err(|source| process_error("wait after terminate", source))?;
            Ok(ProcessOutcome {
                status,
                timed_out: matches!(reason, Reason::TimedOut),
                cancelled: matches!(reason, Reason::Cancelled),
            })
        }
    }
}

async fn collect_output(
    stdout: &mut JoinHandle<Result<Vec<u8>, io::Error>>,
    stderr: &mut JoinHandle<Result<Vec<u8>, io::Error>>,
) -> Result<(Vec<u8>, Vec<u8>), ToolError> {
    let collected = tokio::time::timeout(OUTPUT_DRAIN_TIMEOUT, async {
        let stdout = (&mut *stdout).await;
        let stderr = (&mut *stderr).await;
        (stdout, stderr)
    })
    .await;
    let (stdout, stderr) = match collected {
        Ok(output) => output,
        Err(_) => {
            stdout.abort();
            stderr.abort();
            // Joining aborted readers is best-effort cleanup; the stable
            // process error below is the model-visible outcome.
            drop((&mut *stdout).await);
            drop((&mut *stderr).await);
            return Err(process_error(
                "drain output",
                io::Error::new(io::ErrorKind::TimedOut, "shell output did not close"),
            ));
        }
    };
    let stdout = stdout
        .map_err(|source| process_error("join stdout reader", io::Error::other(source)))?
        .map_err(|source| process_error("read stdout", source))?;
    let stderr = stderr
        .map_err(|source| process_error("join stderr reader", io::Error::other(source)))?
        .map_err(|source| process_error("read stderr", source))?;
    Ok((stdout, stderr))
}

#[cfg(unix)]
async fn terminate_remaining_processes(
    _child: &mut Child,
    process_group: u32,
) -> Result<(), ToolError> {
    kill_process_group(process_group)
        .map_err(|source| process_error("terminate remaining process group", source))
}

#[cfg(not(unix))]
async fn terminate_remaining_processes(_child: &mut Child) -> Result<(), ToolError> {
    Ok(())
}

#[cfg(unix)]
async fn terminate_processes(_child: &mut Child, process_group: u32) -> Result<(), ToolError> {
    kill_process_group(process_group)
        .map_err(|source| process_error("terminate process group", source))
}

#[cfg(not(unix))]
async fn terminate_processes(child: &mut Child) -> Result<(), ToolError> {
    child
        .kill()
        .await
        .map_err(|source| process_error("terminate", source))
}

#[cfg(unix)]
fn kill_process_group(process_group: u32) -> Result<(), io::Error> {
    let process_group = i32::try_from(process_group)
        .map_err(|_| io::Error::other("shell process id exceeds platform range"))?;
    // SAFETY: `process_group` comes from the PID returned by the freshly
    // spawned child, and negating it addresses only that isolated process
    // group. `kill` does not retain the pointer-free integer arguments.
    if unsafe { libc::kill(-process_group, libc::SIGKILL) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ESRCH) {
        Ok(())
    } else {
        Err(error)
    }
}

#[cfg(unix)]
struct ProcessGroupGuard(Option<u32>);

#[cfg(unix)]
impl ProcessGroupGuard {
    const fn new(process_group: u32) -> Self {
        Self(Some(process_group))
    }

    fn disarm(&mut self) {
        self.0 = None;
    }
}

#[cfg(unix)]
impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        // Best-effort cleanup for a dropped Tool future; synchronous Drop
        // cannot surface a termination failure to the caller.
        if let Some(process_group) = self.0 {
            drop(kill_process_group(process_group));
        }
    }
}

async fn read_all(mut reader: impl AsyncRead + Unpin) -> Result<Vec<u8>, io::Error> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes).await?;
    Ok(bytes)
}

#[cfg(unix)]
fn exit_signal(status: &ExitStatus) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;

    status.signal()
}

#[cfg(not(unix))]
const fn exit_signal(_status: &ExitStatus) -> Option<i32> {
    None
}

fn process_error(operation: &'static str, source: io::Error) -> ToolError {
    ToolError::Process { operation, source }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::json;
    use stratum_core::CallId;

    use super::*;

    static TEST_DIRECTORY_ID: AtomicU64 = AtomicU64::new(0);

    fn input(arguments: serde_json::Value) -> ToolInput {
        ToolInput::new(CallId::from("call-1"), arguments)
    }

    async fn test_directory(name: &str) -> PathBuf {
        let id = TEST_DIRECTORY_ID.fetch_add(1, Ordering::Relaxed);
        let root =
            std::env::temp_dir().join(format!("stratum-shell-{name}-{}-{id}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&root).await;
        tokio::fs::create_dir_all(&root)
            .await
            .expect("test directory is created");
        root.canonicalize().expect("test directory canonicalizes")
    }

    #[test]
    fn validation_rejects_empty_commands_and_unknown_fields() {
        let tool = ShellTool::new(PathBuf::from("."));

        assert!(matches!(
            tool.validate(&input(json!({"command": ""}))),
            Err(ToolError::InvalidArgument {
                name: "command",
                ..
            })
        ));
        assert!(matches!(
            tool.validate(&input(json!({"command": "pwd", "persistent": true}))),
            Err(ToolError::InvalidInput { .. })
        ));
    }

    #[tokio::test]
    async fn command_returns_complete_streams_status_and_workdir() {
        let root = test_directory("output").await;
        tokio::fs::create_dir(root.join("nested"))
            .await
            .expect("nested directory is created");
        let tool = ShellTool::new(root.clone());

        let output = tool
            .call(
                input(json!({
                    "command": "pwd; printf diagnostic >&2; exit 7",
                    "workdir": "nested"
                })),
                &CancellationToken::new(),
            )
            .await
            .expect("non-zero exit is a normal tool result");

        assert_eq!(
            output.result["stdout"],
            json!(format!("{}/nested\n", root.display()))
        );
        assert_eq!(output.result["stderr"], "diagnostic");
        assert_eq!(output.result["exit_code"], 7);
        assert_eq!(output.result["signal"], serde_json::Value::Null);
        assert_eq!(output.result["timed_out"], false);
        tokio::fs::remove_dir_all(root)
            .await
            .expect("test directory is removed");
    }

    #[tokio::test]
    async fn calls_do_not_retain_shell_state() {
        let root = test_directory("fresh").await;
        tokio::fs::create_dir(root.join("nested"))
            .await
            .expect("nested directory is created");
        let tool = ShellTool::new(root.clone());

        tool.call(
            input(json!({
                "command": "export STRATUM_SHELL_STATE=retained; cd nested"
            })),
            &CancellationToken::new(),
        )
        .await
        .expect("first shell call succeeds");
        let output = tool
            .call(
                input(json!({
                    "command": "printf '%s\\n' \"${STRATUM_SHELL_STATE-unset}\"; pwd"
                })),
                &CancellationToken::new(),
            )
            .await
            .expect("second shell call succeeds");

        assert_eq!(
            output.result["stdout"],
            json!(format!("unset\n{}\n", root.display()))
        );
        tokio::fs::remove_dir_all(root)
            .await
            .expect("test directory is removed");
    }

    #[tokio::test]
    async fn pre_cancelled_call_does_not_start_a_process() {
        let root = test_directory("pre-cancel").await;
        let tool = ShellTool::new(root.clone());
        let cancellation = CancellationToken::new();
        cancellation.cancel();

        let result = tool
            .call(
                input(json!({"command": "touch should-not-exist"})),
                &cancellation,
            )
            .await;

        assert!(matches!(result, Err(ToolError::Cancelled)));
        assert!(!root.join("should-not-exist").exists());
        tokio::fs::remove_dir_all(root)
            .await
            .expect("test directory is removed");
    }

    #[tokio::test]
    async fn timeout_kills_the_process_and_preserves_output() {
        let root = test_directory("timeout").await;
        let tool = ShellTool::new(root.clone());

        let output = tool
            .call(
                input(json!({
                    "command": "printf before-timeout; sleep 10",
                    "timeout_ms": 150
                })),
                &CancellationToken::new(),
            )
            .await
            .expect("timeout is a normal tool result");

        assert_eq!(output.result["stdout"], "before-timeout");
        assert_eq!(output.result["exit_code"], serde_json::Value::Null);
        assert_eq!(output.result["timed_out"], true);
        tokio::fs::remove_dir_all(root)
            .await
            .expect("test directory is removed");
    }

    #[tokio::test]
    async fn cancellation_kills_a_started_process() {
        let root = test_directory("cancel").await;
        let tool = ShellTool::new(root.clone());
        let cancellation = CancellationToken::new();
        let trigger = cancellation.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            trigger.cancel();
        });

        let result = tool
            .call(input(json!({"command": "sleep 10"})), &cancellation)
            .await;

        assert!(matches!(result, Err(ToolError::Cancelled)));
        tokio::fs::remove_dir_all(root)
            .await
            .expect("test directory is removed");
    }
}
