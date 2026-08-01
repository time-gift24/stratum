//! Filesystem durable event backend for one agent-loop run.
//!
//! Layout: `<root>/<run_id>/events.jsonl`, one JSON event per line. Every
//! append writes the full line, fsyncs the file, and fsyncs the run
//! directory before acknowledging, so an acknowledged event survives a
//! crash. [`read_events`] replays the log and tolerates a truncated tail
//! line left by a crash mid-append.
//!
//! A `TranscriptCompacted` append additionally records one checkpoint line in
//! `<root>/<run_id>/compact.jsonl` after the event is durable. The index is a
//! rebuildable derivative of the event log, never a second source of truth:
//! [`read_events_from_checkpoint`] uses the newest matching checkpoint to skip
//! the compacted prefix, and any index problem falls back to a full replay.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use stratum_core::{ChatMessage, DurableAgentEvent};
use tokio::sync::Mutex;

use super::{DurableEventSink, DurableEventSinkError, FilesystemEventSinkError};

/// Name of the JSONL event log inside each run directory.
pub const EVENTS_FILE_NAME: &str = "events.jsonl";

/// Name of the derived compaction checkpoint index inside each run directory.
pub const COMPACT_INDEX_FILE_NAME: &str = "compact.jsonl";

/// One derived compaction checkpoint recorded in `compact.jsonl`.
///
/// The index only accelerates resume; it is a rebuildable derivative of the
/// event log, never the truth. Every field can be recomputed from
/// `events.jsonl`, and a missing, corrupt, or mismatching index must degrade
/// to a full replay rather than fail closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct CompactionCheckpoint {
    /// Iteration whose prepare boundary executed the compaction.
    pub compacted_iteration: u64,
    /// 1-based line number of the matching `TranscriptCompacted` event in
    /// `events.jsonl` (the log is append-only, so line numbers are stable).
    pub event_line: u64,
    /// Exclusive end index of the replaced committed-context prefix.
    pub upto: u64,
    /// Lowercase hex SHA-256 of the canonical JSON of the summary message.
    pub summary_digest: String,
}

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
        let checkpoint = match &event {
            DurableAgentEvent::TranscriptCompacted {
                upto,
                summary,
                compacted_iteration,
            } => Some((*compacted_iteration, *upto, summary.clone())),
            _ => None,
        };
        let run_dir = self.run_dir.clone();
        // Hold the async-aware lock across the blocking writes so concurrent
        // appends stay serialized; never hold a std mutex guard here.
        let _permit = self.append_lock.lock().await;
        tracing::debug!(
            event_type,
            run_dir = %self.run_dir.display(),
            "appending durable event"
        );
        tokio::task::spawn_blocking(move || append_line(&run_dir, &line))
            .await
            .map_err(|source| FilesystemEventSinkError::Join { source })??;
        if let Some((compacted_iteration, upto, summary)) = checkpoint {
            // Write order is irreversible: the event line is durable before
            // its checkpoint is appended. A crash between the two only leaves
            // the index behind, which readers tolerate by falling back to a
            // full replay or an earlier checkpoint.
            let run_dir = self.run_dir.clone();
            tokio::task::spawn_blocking(move || {
                append_checkpoint(&run_dir, compacted_iteration, upto, &summary)
            })
            .await
            .map_err(|source| FilesystemEventSinkError::Join { source })??;
        }
        Ok(())
    }
}

fn append_line(run_dir: &Path, line: &str) -> Result<(), FilesystemEventSinkError> {
    append_durable_line(run_dir, EVENTS_FILE_NAME, line, |path, source| {
        FilesystemEventSinkError::Append { path, source }
    })
}

fn append_checkpoint_line(run_dir: &Path, line: &str) -> Result<(), FilesystemEventSinkError> {
    append_durable_line(run_dir, COMPACT_INDEX_FILE_NAME, line, |path, source| {
        FilesystemEventSinkError::AppendCheckpoint { path, source }
    })
}

fn append_durable_line(
    run_dir: &Path,
    file_name: &str,
    line: &str,
    append_error: impl Fn(PathBuf, std::io::Error) -> FilesystemEventSinkError,
) -> Result<(), FilesystemEventSinkError> {
    use std::io::Write;

    std::fs::create_dir_all(run_dir).map_err(|source| FilesystemEventSinkError::CreateRunDir {
        path: run_dir.to_path_buf(),
        source,
    })?;
    let path = run_dir.join(file_name);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|source| append_error(path.clone(), source))?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|source| append_error(path.clone(), source))?;
    // Fsync the directory so the file's directory entry survives a crash.
    std::fs::File::open(run_dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|source| FilesystemEventSinkError::SyncRunDir {
            path: run_dir.to_path_buf(),
            source,
        })?;
    Ok(())
}

/// Appends one derived checkpoint for a `TranscriptCompacted` event that has
/// already been durably appended to the event log.
fn append_checkpoint(
    run_dir: &Path,
    compacted_iteration: u64,
    upto: u64,
    summary: &ChatMessage,
) -> Result<(), FilesystemEventSinkError> {
    let log_path = run_dir.join(EVENTS_FILE_NAME);
    let bytes = std::fs::read(&log_path).map_err(|source| FilesystemEventSinkError::Read {
        path: log_path.clone(),
        source,
    })?;
    let checkpoint = CompactionCheckpoint {
        compacted_iteration,
        // The event was just appended at the tail, so the current line count
        // is its 1-based line number.
        event_line: count_log_lines(&bytes),
        upto,
        summary_digest: summary_digest(summary),
    };
    let line = serde_json::to_string(&checkpoint).map_err(|source| {
        FilesystemEventSinkError::Serialize {
            event_type: "compaction_checkpoint",
            source,
        }
    })?;
    append_checkpoint_line(run_dir, &line)
}

/// Counts the physical lines of an append-only log: newline-terminated lines
/// plus a possible unterminated tail line left by a crash mid-append.
fn count_log_lines(bytes: &[u8]) -> u64 {
    let terminated =
        u64::try_from(bytes.iter().filter(|&&byte| byte == b'\n').count()).unwrap_or(u64::MAX);
    let unterminated_tail = u64::from(bytes.last().is_some_and(|&byte| byte != b'\n'));
    terminated + unterminated_tail
}

/// Computes the lowercase hex SHA-256 of the canonical JSON of a summary
/// message, matching the digest convention of the hook journal.
fn summary_digest(summary: &ChatMessage) -> String {
    use std::fmt::Write as _;

    let encoded =
        serde_json::to_vec(summary).expect("serializing a chat message to JSON is infallible");
    Sha256::digest(&encoded)
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a string cannot fail");
            output
        })
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
#[tracing::instrument(level = "debug", skip_all, fields(run_dir = %run_dir.display()))]
pub fn read_events(run_dir: &Path) -> Result<Vec<DurableAgentEvent>, FilesystemEventSinkError> {
    let path = run_dir.join(EVENTS_FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(FilesystemEventSinkError::Read { path, source }),
    };
    // Parse from bytes rather than UTF-8 text so a torn write that lands in
    // the middle of a multi-byte character is tolerated like any other
    // truncated tail line instead of failing the whole read.
    parse_log_lines(&path, &split_log_lines(&bytes))
}

/// Reads the durable events in `run_dir`, accelerated by the compaction
/// checkpoint index when a valid checkpoint exists.
///
/// The index is a rebuildable derivative of the event log, never a second
/// source of truth. Any index problem — a missing or empty `compact.jsonl`,
/// a truncated tail line, corrupt JSON, or a checkpoint whose target line is
/// not the matching `TranscriptCompacted` event — falls back to
/// [`read_events`] instead of failing closed; only corruption of the event
/// log itself is a typed error, exactly as in [`read_events`].
///
/// On the fast path the returned sequence is the `LoopStarted` head event —
/// resume still needs it for chain version validation — followed by the event
/// window starting at the checkpoint's `TranscriptCompacted` line, which
/// replay treats as equivalent to the full stream.
///
/// This performs blocking file IO; async callers handling large logs should
/// offload it with `tokio::task::spawn_blocking`.
///
/// # Errors
///
/// Returns [`FilesystemEventSinkError::Read`] when the log cannot be read and
/// [`FilesystemEventSinkError::MalformedEvent`] when a non-tail line of the
/// replayed window fails to parse.
#[tracing::instrument(level = "debug", skip_all, fields(run_dir = %run_dir.display()))]
pub fn read_events_from_checkpoint(
    run_dir: &Path,
) -> Result<Vec<DurableAgentEvent>, FilesystemEventSinkError> {
    let Some(checkpoint) = read_latest_checkpoint(run_dir) else {
        return read_events(run_dir);
    };
    let path = run_dir.join(EVENTS_FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => return Err(FilesystemEventSinkError::Read { path, source }),
    };
    let lines = split_log_lines(&bytes);
    let Some(&(1, first_line)) = lines.first() else {
        return read_events(run_dir);
    };
    let Ok(first_event) = serde_json::from_slice::<DurableAgentEvent>(first_line) else {
        // A malformed head line is event-log corruption; delegating to the
        // full reader surfaces the typed error.
        return read_events(run_dir);
    };
    if !matches!(first_event, DurableAgentEvent::LoopStarted { .. }) {
        return read_events(run_dir);
    }
    let Some(window_start) = lines
        .iter()
        .position(|(line_number, _)| *line_number == checkpoint.event_line)
    else {
        tracing::warn!(
            path = %path.display(),
            event_line = checkpoint.event_line,
            "compaction checkpoint points past the event log; falling back to full replay"
        );
        return read_events(run_dir);
    };
    let window = parse_log_lines(&path, &lines[window_start..])?;
    let matches_checkpoint = matches!(
        window.first(),
        Some(DurableAgentEvent::TranscriptCompacted {
            upto,
            summary,
            compacted_iteration,
        }) if *upto == checkpoint.upto
            && *compacted_iteration == checkpoint.compacted_iteration
            && summary_digest(summary) == checkpoint.summary_digest
    );
    if !matches_checkpoint {
        tracing::warn!(
            path = %path.display(),
            event_line = checkpoint.event_line,
            "compaction checkpoint does not match the event log; falling back to full replay"
        );
        return read_events(run_dir);
    }
    let mut events = Vec::with_capacity(window.len() + 1);
    events.push(first_event);
    events.extend(window);
    Ok(events)
}

/// Reads the newest complete checkpoint from the derived index.
///
/// Returns `None` — never an error — when the index is missing, unreadable,
/// empty, or corrupt, so callers fall back to a full replay. A truncated tail
/// line left by a crash mid-append is skipped in favor of the previous
/// checkpoint.
fn read_latest_checkpoint(run_dir: &Path) -> Option<CompactionCheckpoint> {
    let path = run_dir.join(COMPACT_INDEX_FILE_NAME);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(source) => {
            tracing::warn!(
                path = %path.display(),
                error = %source,
                "compaction checkpoint index unreadable; falling back to full replay"
            );
            return None;
        }
    };
    let mut lines: Vec<&[u8]> = bytes
        .split(|byte| *byte == b'\n')
        .filter(|line| line.iter().any(|byte| !byte.is_ascii_whitespace()))
        .collect();
    // A crash mid-append can only tear the final line; tolerate at most that
    // one. Any other unparseable line means the index is corrupt.
    let mut tolerate_tail = true;
    while let Some(line) = lines.pop() {
        match serde_json::from_slice::<CompactionCheckpoint>(line) {
            Ok(checkpoint) => return Some(checkpoint),
            Err(source) if tolerate_tail => {
                tolerate_tail = false;
                tracing::warn!(
                    path = %path.display(),
                    error = %source,
                    "ignoring truncated tail line of the compaction checkpoint index"
                );
            }
            Err(source) => {
                tracing::warn!(
                    path = %path.display(),
                    error = %source,
                    "compaction checkpoint index corrupt; falling back to full replay"
                );
                return None;
            }
        }
    }
    None
}

/// Splits raw log bytes into physical lines with 1-based line numbers,
/// skipping whitespace-only lines.
fn split_log_lines(bytes: &[u8]) -> Vec<(u64, &[u8])> {
    bytes
        .split(|byte| *byte == b'\n')
        .enumerate()
        .filter(|(_, line)| line.iter().any(|byte| !byte.is_ascii_whitespace()))
        .map(|(index, line)| (u64::try_from(index + 1).unwrap_or(u64::MAX), line))
        .collect()
}

/// Parses log lines in order, tolerating a truncated tail line.
fn parse_log_lines(
    path: &Path,
    lines: &[(u64, &[u8])],
) -> Result<Vec<DurableAgentEvent>, FilesystemEventSinkError> {
    let mut events = Vec::with_capacity(lines.len());
    for (position, (line_number, line)) in lines.iter().enumerate() {
        match serde_json::from_slice(line) {
            Ok(event) => events.push(event),
            Err(_) if position + 1 == lines.len() => {
                // Truncated tail line left by a crash mid-append: observable
                // crash evidence, so it is worth a warning.
                tracing::warn!(
                    path = %path.display(),
                    line = *line_number,
                    "ignoring truncated tail line of the durable event log"
                );
                break;
            }
            Err(source) => {
                return Err(FilesystemEventSinkError::MalformedEvent {
                    path: path.to_path_buf(),
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
        COMPACT_INDEX_FILE_NAME, CompactionCheckpoint, EVENTS_FILE_NAME,
        FilesystemDurableEventSink, FilesystemEventSinkError, read_events,
        read_events_from_checkpoint, summary_digest,
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
            DurableAgentEvent::LoopStarted {
                extension_set_version_id: None,
            },
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
        assert_eq!(
            u64::try_from(events.len()).unwrap_or(u64::MAX),
            TASKS * APPENDS_PER_TASK
        );
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

        let result = sink
            .append(DurableAgentEvent::LoopStarted {
                extension_set_version_id: None,
            })
            .await;

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
    async fn torn_utf8_tail_line_is_tolerated() {
        let run = TestRunDir::new("torn-utf8-tail");
        let sink = FilesystemDurableEventSink::new(run.path());
        let events = sample_events();
        for event in &events {
            sink.append(event.clone())
                .await
                .expect("append should succeed");
        }

        // Simulate a crash mid-append that stops inside a multi-byte UTF-8
        // character: the tail is not even valid UTF-8.
        let log_path = run.path().join(EVENTS_FILE_NAME);
        let mut raw = std::fs::read(&log_path).expect("event log should be readable");
        raw.extend_from_slice("{\"type\":\"message_appended\",\"data\":\"".as_bytes());
        raw.extend_from_slice(&"中".as_bytes()[..2]);
        std::fs::write(&log_path, raw).expect("event log should be writable");

        assert_eq!(
            read_events(run.path()).expect("torn utf-8 tail must be tolerated"),
            events
        );
    }

    #[tokio::test]
    async fn malformed_middle_line_is_a_typed_error() {
        let run = TestRunDir::new("malformed-middle");
        let sink = FilesystemDurableEventSink::new(run.path());
        sink.append(DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        })
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

    fn compacted_event(
        upto: u64,
        compacted_iteration: u64,
        summary_text: &str,
    ) -> DurableAgentEvent {
        DurableAgentEvent::TranscriptCompacted {
            upto,
            summary: ChatMessage::system(summary_text),
            compacted_iteration,
        }
    }

    /// A run compacted at event line 4 and again at event line 7.
    fn compacted_run_events() -> Vec<DurableAgentEvent> {
        vec![
            DurableAgentEvent::LoopStarted {
                extension_set_version_id: None,
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("m1"),
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("m2"),
            },
            compacted_event(2, 1, "[stratum:transcript-compacted]\nfirst summary"),
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("m3"),
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("m4"),
            },
            compacted_event(3, 2, "[stratum:transcript-compacted]\nsecond summary"),
            DurableAgentEvent::IterationCompleted {
                iteration: 2,
                usage: usage(),
            },
        ]
    }

    async fn write_events(run: &TestRunDir, events: &[DurableAgentEvent]) {
        let sink = FilesystemDurableEventSink::new(run.path());
        for event in events {
            sink.append(event.clone())
                .await
                .expect("append should succeed");
        }
    }

    fn compact_index_path(run: &TestRunDir) -> PathBuf {
        run.path().join(COMPACT_INDEX_FILE_NAME)
    }

    fn read_checkpoints(run: &TestRunDir) -> Vec<CompactionCheckpoint> {
        std::fs::read_to_string(compact_index_path(run))
            .expect("checkpoint index should be readable")
            .lines()
            .map(|line| serde_json::from_str(line).expect("checkpoint line should parse"))
            .collect()
    }

    fn rewrite_checkpoint_index(run: &TestRunDir, checkpoints: &[CompactionCheckpoint]) {
        let mut raw = String::new();
        for checkpoint in checkpoints {
            raw.push_str(&serde_json::to_string(checkpoint).expect("checkpoint should serialize"));
            raw.push('\n');
        }
        std::fs::write(compact_index_path(run), raw).expect("checkpoint index should be writable");
    }

    #[tokio::test]
    async fn compaction_append_writes_matching_checkpoint() {
        let run = TestRunDir::new("checkpoint-write");
        let events = compacted_run_events();
        write_events(&run, &events).await;

        let checkpoints = read_checkpoints(&run);
        assert_eq!(checkpoints.len(), 2);
        assert_eq!(checkpoints[0].compacted_iteration, 1);
        assert_eq!(checkpoints[0].event_line, 4);
        assert_eq!(checkpoints[0].upto, 2);
        let latest = &checkpoints[1];
        assert_eq!(latest.compacted_iteration, 2);
        assert_eq!(latest.event_line, 7);
        assert_eq!(latest.upto, 3);
        assert!(
            latest.summary_digest.len() == 64
                && latest
                    .summary_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "digest must be lowercase hex sha-256"
        );
        let DurableAgentEvent::TranscriptCompacted { summary, .. } = &events[6] else {
            panic!("event at line 7 must be the second compaction");
        };
        assert_eq!(latest.summary_digest, summary_digest(summary));
    }

    #[tokio::test]
    async fn non_compaction_events_do_not_write_checkpoint_index() {
        let run = TestRunDir::new("no-checkpoint-without-compaction");
        write_events(&run, &sample_events()).await;

        assert!(!compact_index_path(&run).exists());
    }

    #[tokio::test]
    async fn checkpoint_read_returns_loop_started_plus_compacted_window() {
        let run = TestRunDir::new("checkpoint-fast-path");
        let events = compacted_run_events();
        write_events(&run, &events).await;

        let fast = read_events_from_checkpoint(run.path()).expect("checkpoint read should succeed");

        // The fast path returns LoopStarted followed by the window starting
        // at the latest TranscriptCompacted; replaying that sequence is
        // equivalent to replaying the full stream.
        let mut expected = vec![events[0].clone()];
        expected.extend(events[6..].iter().cloned());
        assert_eq!(fast, expected);
        let full = read_events(run.path()).expect("full replay should succeed");
        assert_eq!(full, events);
        assert_eq!(fast[0], full[0]);
        assert_eq!(fast[1..], full[6..]);
    }

    #[tokio::test]
    async fn missing_checkpoint_index_falls_back_to_full_replay() {
        let run = TestRunDir::new("checkpoint-missing");
        let events = compacted_run_events();
        write_events(&run, &events).await;
        std::fs::remove_file(compact_index_path(&run)).expect("index should be removable");

        assert_eq!(
            read_events_from_checkpoint(run.path()).expect("missing index must not fail the read"),
            events
        );
    }

    #[tokio::test]
    async fn empty_checkpoint_index_falls_back_to_full_replay() {
        let run = TestRunDir::new("checkpoint-empty");
        let events = compacted_run_events();
        write_events(&run, &events).await;
        std::fs::write(compact_index_path(&run), "").expect("index should be writable");

        assert_eq!(
            read_events_from_checkpoint(run.path()).expect("empty index must not fail the read"),
            events
        );
    }

    #[tokio::test]
    async fn truncated_checkpoint_tail_uses_previous_checkpoint() {
        let run = TestRunDir::new("checkpoint-truncated-tail");
        let events = compacted_run_events();
        write_events(&run, &events).await;
        // Simulate a crash mid-append of the second checkpoint: the first
        // line is complete, the tail line is torn.
        let checkpoints = read_checkpoints(&run);
        let mut raw = serde_json::to_string(&checkpoints[0]).expect("checkpoint should serialize");
        raw.push('\n');
        raw.push_str("{\"compacted_iteration\":2,\"event_line\":7");
        std::fs::write(compact_index_path(&run), raw).expect("index should be writable");

        let fast = read_events_from_checkpoint(run.path()).expect("checkpoint read should succeed");

        let mut expected = vec![events[0].clone()];
        expected.extend(events[3..].iter().cloned());
        assert_eq!(fast, expected);
    }

    #[tokio::test]
    async fn corrupt_checkpoint_index_falls_back_to_full_replay() {
        let run = TestRunDir::new("checkpoint-corrupt");
        let events = compacted_run_events();
        write_events(&run, &events).await;
        // Neither line parses: the torn tail is tolerated, the remaining
        // corrupt line marks the index as corrupt.
        std::fs::write(
            compact_index_path(&run),
            "not a checkpoint\nstill not a checkpoint\n",
        )
        .expect("index should be writable");

        assert_eq!(
            read_events_from_checkpoint(run.path()).expect("corrupt index must not fail the read"),
            events
        );
    }

    #[tokio::test]
    async fn checkpoint_with_mismatched_event_line_falls_back_to_full_replay() {
        let run = TestRunDir::new("checkpoint-tampered-line");
        let events = compacted_run_events();
        write_events(&run, &events).await;
        let mut checkpoints = read_checkpoints(&run);
        // Tamper: point the latest checkpoint at a MessageAppended line.
        checkpoints[1].event_line = 5;
        rewrite_checkpoint_index(&run, &checkpoints);

        assert_eq!(
            read_events_from_checkpoint(run.path())
                .expect("mismatched checkpoint must not fail the read"),
            events
        );
    }

    #[tokio::test]
    async fn checkpoint_with_mismatched_summary_digest_falls_back_to_full_replay() {
        let run = TestRunDir::new("checkpoint-tampered-digest");
        let events = compacted_run_events();
        write_events(&run, &events).await;
        let mut checkpoints = read_checkpoints(&run);
        checkpoints[1].summary_digest = "0".repeat(64);
        rewrite_checkpoint_index(&run, &checkpoints);

        assert_eq!(
            read_events_from_checkpoint(run.path())
                .expect("mismatched checkpoint must not fail the read"),
            events
        );
    }

    #[tokio::test]
    async fn lagging_checkpoint_index_replays_from_earlier_checkpoint() {
        let run = TestRunDir::new("checkpoint-lagging");
        let events = compacted_run_events();
        write_events(&run, &events).await;
        // Simulate a crash after the second TranscriptCompacted landed but
        // before its checkpoint was appended: drop the last index line.
        let checkpoints = read_checkpoints(&run);
        rewrite_checkpoint_index(&run, &checkpoints[..1]);

        let fast = read_events_from_checkpoint(run.path()).expect("checkpoint read should succeed");

        let mut expected = vec![events[0].clone()];
        expected.extend(events[3..].iter().cloned());
        assert_eq!(fast, expected);
    }
}
