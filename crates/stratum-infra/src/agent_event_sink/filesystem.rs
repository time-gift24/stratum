//! Filesystem durable event backend for one agent-loop run.
//!
//! Layout: `<root>/<run_id>/events.jsonl`, one JSON event per line. Every
//! append writes the full line, fsyncs the file, and fsyncs the run
//! directory before acknowledging, so an acknowledged event survives a
//! crash. [`read_events`] replays the log and tolerates a truncated tail
//! line left by a crash mid-append.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use stratum_core::DurableAgentEvent;
use tokio::sync::Mutex;

use super::{DurableEventSink, DurableEventSinkError, FilesystemEventSinkError};

/// Name of the JSONL event log inside each run directory.
pub const EVENTS_FILE_NAME: &str = "events.jsonl";

/// Durable event sink appending one JSON line per event to a run directory.
///
/// Appends are serialized internally, so concurrent writers never interleave
/// partial lines. The run directory is created lazily on the first append.
pub struct FilesystemDurableEventSink {
    run_dir: PathBuf,
    append_lock: Mutex<()>,
}

impl FilesystemDurableEventSink {
    /// Creates a sink writing to `run_dir` (`<root>/<run_id>/`).
    #[must_use]
    pub fn new(run_dir: impl Into<PathBuf>) -> Self {
        Self {
            run_dir: run_dir.into(),
            append_lock: Mutex::new(()),
        }
    }

    /// Returns the run directory this sink writes to.
    #[must_use]
    pub fn run_dir(&self) -> &Path {
        &self.run_dir
    }
}

#[async_trait]
impl DurableEventSink for FilesystemDurableEventSink {
    async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
        let event_type = event.event_type();
        let line = serde_json::to_string(&event)
            .map_err(|source| FilesystemEventSinkError::Serialize { event_type, source })?;
        let run_dir = self.run_dir.clone();
        // Hold the async-aware lock across the blocking write so concurrent
        // appends stay serialized; never hold a std mutex guard here.
        let _permit = self.append_lock.lock().await;
        tokio::task::spawn_blocking(move || append_line(&run_dir, &line))
            .await
            .map_err(|_| DurableEventSinkError::PublisherUnavailable)??;
        Ok(())
    }
}

fn append_line(run_dir: &Path, line: &str) -> Result<(), FilesystemEventSinkError> {
    use std::io::Write;

    std::fs::create_dir_all(run_dir).map_err(|source| FilesystemEventSinkError::CreateRunDir {
        path: run_dir.to_path_buf(),
        source,
    })?;
    let path = run_dir.join(EVENTS_FILE_NAME);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| FilesystemEventSinkError::Append {
            path: path.clone(),
            source,
        })?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| FilesystemEventSinkError::Append {
            path: path.clone(),
            source,
        })?;
    // Fsync the directory so the file's directory entry survives a crash.
    std::fs::File::open(run_dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|source| FilesystemEventSinkError::SyncRunDir {
            path: run_dir.to_path_buf(),
            source,
        })?;
    Ok(())
}

/// Reads the durable events recorded in `run_dir`, in append order.
///
/// Semantics:
///
/// - a missing `events.jsonl` yields an empty event stream (the run never
///   persisted anything);
/// - any other read failure (for example an unreadable or invalid run
///   directory) is a typed error;
/// - a malformed non-tail line is a typed error, because append-only writes
///   cannot corrupt middle lines;
/// - a malformed final line is ignored and the complete prefix is returned,
///   because a crash can leave a truncated tail line.
///
/// This performs blocking file IO; async callers handling large logs should
/// offload it with `tokio::task::spawn_blocking`.
///
/// # Errors
///
/// Returns [`FilesystemEventSinkError::Read`] when the log cannot be read and
/// [`FilesystemEventSinkError::MalformedEvent`] when a non-tail line fails to
/// parse.
pub fn read_events(run_dir: &Path) -> Result<Vec<DurableAgentEvent>, FilesystemEventSinkError> {
    let path = run_dir.join(EVENTS_FILE_NAME);
    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(FilesystemEventSinkError::Read { path, source }),
    };
    let lines: Vec<(u64, &str)> = contents
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index as u64 + 1, line))
        .collect();
    let mut events = Vec::with_capacity(lines.len());
    for (position, (line_number, line)) in lines.iter().enumerate() {
        match serde_json::from_str(line) {
            Ok(event) => events.push(event),
            Err(_) if position + 1 == lines.len() => {
                // Truncated tail line left by a crash mid-append.
                break;
            }
            Err(source) => {
                return Err(FilesystemEventSinkError::MalformedEvent {
                    path: path.clone(),
                    line: *line_number,
                    source,
                });
            }
        }
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use std::{
        path::{Path, PathBuf},
        sync::{
            Arc,
            atomic::{AtomicU64, Ordering},
        },
    };

    use serde_json::json;
    use stratum_core::{
        ApprovalDecision, ApprovalId, CallId, ChatMessage, DangerLevel, DurableAgentEvent,
        TokenUsage, ToolKind, ToolName,
    };

    use super::{
        EVENTS_FILE_NAME, FilesystemDurableEventSink, FilesystemEventSinkError, read_events,
    };
    use crate::DurableEventSink;

    static TEMP_DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

    struct TestRunDir(PathBuf);

    impl TestRunDir {
        fn new(test_name: &str) -> Self {
            let unique = TEMP_DIR_COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "stratum-infra-event-sink-{test_name}-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).expect("test run directory should be creatable");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TestRunDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn usage() -> TokenUsage {
        TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            total_tokens: 3,
        }
    }

    fn sample_events() -> Vec<DurableAgentEvent> {
        vec![
            DurableAgentEvent::LoopStarted,
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("hello"),
            },
            DurableAgentEvent::ToolApprovalRequested {
                approval_id: ApprovalId::new(),
                call_id: CallId::from("tool-call-1"),
                tool_name: ToolName::from("echo"),
                arguments: json!({ "text": "hello" }),
                tool_kind: ToolKind::Read,
                danger_level: DangerLevel::Low,
            },
            DurableAgentEvent::ToolApprovalResolved {
                approval_id: ApprovalId::new(),
                decision: ApprovalDecision::Approve,
            },
            DurableAgentEvent::ToolExecutionStarted {
                call_id: CallId::from("tool-call-1"),
                tool_name: ToolName::from("echo"),
            },
            DurableAgentEvent::IterationCompleted {
                iteration: 1,
                usage: usage(),
            },
            DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage: usage(),
            },
        ]
    }

    #[tokio::test]
    async fn appended_events_read_back_equal_and_in_order() {
        let run = TestRunDir::new("roundtrip");
        let sink = FilesystemDurableEventSink::new(run.path());
        let events = sample_events();

        for event in &events {
            sink.append(event.clone())
                .await
                .expect("append should succeed");
        }

        assert_eq!(
            read_events(run.path()).expect("read should succeed"),
            events
        );
    }

    #[tokio::test]
    async fn every_appended_line_is_one_complete_event() {
        let run = TestRunDir::new("one-line-per-event");
        let sink = FilesystemDurableEventSink::new(run.path());

        for event in sample_events() {
            sink.append(event).await.expect("append should succeed");
        }

        let raw = std::fs::read_to_string(run.path().join(EVENTS_FILE_NAME))
            .expect("event log should be readable");
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 7);
        for line in lines {
            serde_json::from_str::<DurableAgentEvent>(line)
                .expect("each line must be one complete event");
        }
    }

    #[tokio::test]
    async fn concurrent_appends_do_not_interleave_lines() {
        let run = TestRunDir::new("concurrent");
        let sink = Arc::new(FilesystemDurableEventSink::new(run.path()));
        const TASKS: u64 = 4;
        const APPENDS_PER_TASK: u64 = 20;

        let mut handles = Vec::new();
        for task in 0..TASKS {
            let sink = Arc::clone(&sink);
            handles.push(tokio::spawn(async move {
                for index in 0..APPENDS_PER_TASK {
                    sink.append(DurableAgentEvent::MessageAppended {
                        message: ChatMessage::user(format!("task-{task}-event-{index}")),
                    })
                    .await
                    .expect("append should succeed");
                }
            }));
        }
        for handle in handles {
            handle.await.expect("append task should not panic");
        }

        let events = read_events(run.path()).expect("all lines must parse as complete events");
        assert_eq!(events.len() as u64, TASKS * APPENDS_PER_TASK);
        // Each task's events must appear in its own append order.
        for task in 0..TASKS {
            let mut expected_index = 0;
            for event in &events {
                let DurableAgentEvent::MessageAppended { message } = event else {
                    panic!("only message events were appended");
                };
                let text = format!("{message:?}");
                if text.contains(&format!("task-{task}-event-")) {
                    assert!(
                        text.contains(&format!("task-{task}-event-{expected_index}")),
                        "task {task} events must keep append order"
                    );
                    expected_index += 1;
                }
            }
            assert_eq!(expected_index, APPENDS_PER_TASK);
        }
    }

    #[tokio::test]
    async fn append_to_read_only_run_dir_fails_with_typed_error() {
        let run = TestRunDir::new("read-only");
        let sink = FilesystemDurableEventSink::new(run.path());

        let mut permissions = std::fs::metadata(run.path())
            .expect("run dir metadata should be readable")
            .permissions();
        permissions.set_readonly(true);
        std::fs::set_permissions(run.path(), permissions).expect("permissions should be settable");

        let result = sink.append(DurableAgentEvent::LoopStarted).await;

        let mut permissions = std::fs::metadata(run.path())
            .expect("run dir metadata should be readable")
            .permissions();
        #[expect(
            clippy::permissions_set_readonly_false,
            reason = "test cleanup must restore writability"
        )]
        permissions.set_readonly(false);
        std::fs::set_permissions(run.path(), permissions)
            .expect("permissions should be restorable");

        let error = result.expect_err("append into a read-only directory must fail");
        assert!(
            matches!(
                error,
                crate::DurableEventSinkError::Filesystem(
                    FilesystemEventSinkError::Append { .. }
                        | FilesystemEventSinkError::CreateRunDir { .. }
                )
            ),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn missing_event_log_reads_as_empty_stream() {
        let run = TestRunDir::new("missing-log");

        assert_eq!(
            read_events(run.path()).expect("missing log should read as empty"),
            Vec::new()
        );
    }

    #[test]
    fn invalid_run_dir_reads_as_typed_error() {
        let run = TestRunDir::new("invalid-dir");
        let not_a_dir = run.path().join("not-a-directory");
        std::fs::write(&not_a_dir, b"payload").expect("file should be writable");

        let error = read_events(&not_a_dir).expect_err("a file as run dir must fail");
        assert!(
            matches!(error, FilesystemEventSinkError::Read { .. }),
            "unexpected error: {error:?}"
        );
    }

    #[tokio::test]
    async fn truncated_tail_line_is_ignored_and_prefix_returned() {
        let run = TestRunDir::new("truncated-tail");
        let sink = FilesystemDurableEventSink::new(run.path());
        let events = sample_events();
        for event in &events {
            sink.append(event.clone())
                .await
                .expect("append should succeed");
        }

        // Simulate a crash mid-append: a partial JSON line at the tail.
        let log_path = run.path().join(EVENTS_FILE_NAME);
        let mut raw = std::fs::read_to_string(&log_path).expect("event log should be readable");
        raw.push_str("{\"type\":\"message_appended\",\"data\":{\"message\":{\"role\":\"user\"");
        std::fs::write(&log_path, raw).expect("event log should be writable");

        assert_eq!(
            read_events(run.path()).expect("truncated tail must be tolerated"),
            events
        );
    }

    #[tokio::test]
    async fn malformed_middle_line_is_a_typed_error() {
        let run = TestRunDir::new("malformed-middle");
        let sink = FilesystemDurableEventSink::new(run.path());
        sink.append(DurableAgentEvent::LoopStarted)
            .await
            .expect("append should succeed");

        let log_path = run.path().join(EVENTS_FILE_NAME);
        let mut raw = std::fs::read_to_string(&log_path).expect("event log should be readable");
        raw.push_str("{\"type\":\"message_appended\"\n");
        raw.push_str(
            &serde_json::to_string(&DurableAgentEvent::LoopCancelled { usage: usage() })
                .expect("event should serialize"),
        );
        raw.push('\n');
        std::fs::write(&log_path, raw).expect("event log should be writable");

        let error = read_events(run.path()).expect_err("malformed middle line must fail");
        match error {
            FilesystemEventSinkError::MalformedEvent { line, .. } => assert_eq!(line, 2),
            other => panic!("unexpected error: {other:?}"),
        }
    }
}
