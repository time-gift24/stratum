//! Filesystem durable event backend for one agent-loop run.
//!
//! Layout: `<root>/<run_id>/events.jsonl`, one JSON event per line. Every
//! append writes the full line, fsyncs the file, and fsyncs the run
//! directory before acknowledging, so an acknowledged event survives a
//! crash. [`read_events`] replays the log and tolerates a truncated tail
//! line left by a crash mid-append.
//!
//! A `TranscriptCompacted` append records a pending compaction in memory; the
//! following `IterationCompleted` flushes one checkpoint line to
//! `<root>/<run_id>/compact.jsonl` after the boundary is durable. The index is
//! a rebuildable derivative of the event log, never a second source of truth:
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
/// event log, never the truth. A checkpoint is appended only after the
/// compaction's `IterationCompleted` is durable, so a checkpoint implies the
/// iteration boundary is committed. Every field can be recomputed from
/// `events.jsonl`, and a missing, corrupt, or mismatching index must degrade
/// to a full replay rather than fail closed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub struct CompactionCheckpoint {
    /// Iteration whose prepare boundary executed the compaction.
    pub compacted_iteration: u64,
    /// 1-based physical line of the first retained message — the
    /// committed-context message at index `upto` — in `events.jsonl`. The
    /// replay window starting here is self-contained: it carries the full
    /// retained suffix, the iteration's prepare journal records, the
    /// `TranscriptCompacted` event, and the iteration boundary.
    pub window_start_line: u64,
    /// Exclusive end index of the replaced committed-context prefix.
    pub upto: u64,
    /// Lowercase hex SHA-256 of the canonical JSON of the summary message.
    pub summary_digest: String,
}

/// A compaction whose `TranscriptCompacted` is durable but whose iteration
/// boundary has not landed yet; its checkpoint is flushed when the next
/// `IterationCompleted` becomes durable.
struct PendingCompaction {
    compacted_iteration: u64,
    upto: u64,
    summary: ChatMessage,
}

/// Append state serialized by the sink's internal async lock.
#[derive(Default)]
struct AppendState {
    pending_compaction: Option<PendingCompaction>,
}

/// Durable event sink appending one JSON line per event to a run directory.
///
/// Appends are serialized internally, so concurrent writers never interleave
/// partial lines. The run directory is created lazily on the first append.
pub struct FilesystemDurableEventSink {
    run_dir: PathBuf,
    append_state: Mutex<AppendState>,
}

impl FilesystemDurableEventSink {
    /// Creates a sink writing to `run_dir` (`<root>/<run_id>/`).
    #[must_use]
    pub fn new(run_dir: impl Into<PathBuf>) -> Self {
        Self {
            run_dir: run_dir.into(),
            append_state: Mutex::new(AppendState::default()),
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
        // Hold the async-aware lock across the blocking writes so concurrent
        // appends stay serialized; never hold a std mutex guard here.
        let mut state = self.append_state.lock().await;
        tracing::debug!(
            event_type,
            run_dir = %self.run_dir.display(),
            "appending durable event"
        );
        tokio::task::spawn_blocking(move || append_line(&run_dir, &line))
            .await
            .map_err(|source| FilesystemEventSinkError::Join { source })??;
        match &event {
            DurableAgentEvent::TranscriptCompacted {
                upto,
                summary,
                compacted_iteration,
            } => {
                // Record the compaction in memory only. Its checkpoint is
                // flushed after this compaction's IterationCompleted is
                // durable, so the crash window between the two never has a
                // checkpoint and can only take the full-replay path.
                let pending = PendingCompaction {
                    compacted_iteration: *compacted_iteration,
                    upto: *upto,
                    summary: summary.clone(),
                };
                if state.pending_compaction.replace(pending).is_some() {
                    tracing::warn!(
                        "compaction superseded before its iteration boundary; dropping the earlier pending checkpoint"
                    );
                }
            }
            DurableAgentEvent::IterationCompleted { .. } => {
                if let Some(pending) = state.pending_compaction.take() {
                    // Write order is irreversible: the boundary event is
                    // durable before the checkpoint is appended. A crash
                    // between the two only leaves the index behind, which
                    // readers tolerate by falling back to a full replay or an
                    // earlier checkpoint.
                    let run_dir = self.run_dir.clone();
                    // The index is derived data: a failed checkpoint write
                    // must degrade to a lagging index, never kill the run
                    // whose boundary is already durable.
                    let outcome =
                        tokio::task::spawn_blocking(move || append_checkpoint(&run_dir, &pending))
                            .await;
                    let flush_result = match outcome {
                        Ok(result) => result,
                        Err(source) => Err(FilesystemEventSinkError::Join { source }),
                    };
                    if let Err(error) = flush_result {
                        tracing::warn!(
                            run_dir = %self.run_dir.display(),
                            %error,
                            "checkpoint index write failed; the index lags behind and resume falls back to a full replay"
                        );
                    }
                }
            }
            _ => {}
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

/// Appends one derived checkpoint for a pending compaction whose iteration
/// boundary has just become durable.
fn append_checkpoint(
    run_dir: &Path,
    pending: &PendingCompaction,
) -> Result<(), FilesystemEventSinkError> {
    let log_path = run_dir.join(EVENTS_FILE_NAME);
    let bytes = std::fs::read(&log_path).map_err(|source| FilesystemEventSinkError::Read {
        path: log_path.clone(),
        source,
    })?;
    let checkpoint = CompactionCheckpoint {
        compacted_iteration: pending.compacted_iteration,
        window_start_line: locate_window_start(&log_path, &bytes, pending.upto)?,
        upto: pending.upto,
        summary_digest: summary_digest(&pending.summary),
    };
    let line = serde_json::to_string(&checkpoint).map_err(|source| {
        FilesystemEventSinkError::Serialize {
            event_type: "compaction_checkpoint",
            source,
        }
    })?;
    append_checkpoint_line(run_dir, &line)
}

/// Finds the 1-based physical line of the first retained message: the
/// committed-context message at index `upto`.
///
/// The scan replays committed-message ordinals over the log: every
/// `message_appended` takes the next committed index, and every earlier
/// `TranscriptCompacted` collapses `upto` committed messages into one summary
/// that has no `message_appended` line of its own, shifting later messages
/// back by `upto - 1`. Compaction is rare, so one O(log) scan per checkpoint
/// is acceptable.
fn locate_window_start(
    log_path: &Path,
    bytes: &[u8],
    upto: u64,
) -> Result<u64, FilesystemEventSinkError> {
    let lines = split_log_lines(bytes);
    let mut committed_index = 0_u64;
    for (position, (line_number, line)) in lines.iter().enumerate() {
        let event = match serde_json::from_slice::<DurableAgentEvent>(line) {
            Ok(event) => event,
            // A crash-torn tail line is tolerated exactly as in `read_events`.
            Err(_) if position + 1 == lines.len() => break,
            Err(source) => {
                return Err(FilesystemEventSinkError::MalformedEvent {
                    path: log_path.to_path_buf(),
                    line: *line_number,
                    source,
                });
            }
        };
        match event {
            DurableAgentEvent::MessageAppended { .. } => {
                if committed_index == upto {
                    return Ok(*line_number);
                }
                committed_index += 1;
            }
            DurableAgentEvent::TranscriptCompacted { upto, .. } => {
                committed_index = committed_index.saturating_sub(upto.saturating_sub(1));
            }
            _ => {}
        }
    }
    Err(FilesystemEventSinkError::MissingRetainedMessage {
        path: log_path.to_path_buf(),
        upto,
    })
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
/// a truncated tail line, corrupt JSON, or a checkpoint that fails validation
/// against the event log — falls back to [`read_events`] instead of failing
/// closed; only corruption of the event log itself is a typed error, exactly
/// as in [`read_events`].
///
/// On the fast path the returned sequence is the `LoopStarted` head event —
/// resume still needs it for chain version validation — followed by the event
/// window starting at the checkpoint's `window_start_line`. Validation
/// requires the head line to be `LoopStarted`, the window's first line to be
/// a `message_appended` (the first retained message), and the window to
/// contain a `TranscriptCompacted` matching the checkpoint's
/// `compacted_iteration`, `upto`, and `summary_digest`. A valid window is
/// self-contained — retained suffix, prepare journal records, compaction
/// event, and iteration boundary — so replay treats it as equivalent to the
/// full stream.
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
        .position(|(line_number, _)| *line_number == checkpoint.window_start_line)
    else {
        tracing::warn!(
            path = %path.display(),
            window_start_line = checkpoint.window_start_line,
            "compaction checkpoint points past the event log; falling back to full replay"
        );
        return read_events(run_dir);
    };
    let window = parse_log_lines(&path, &lines[window_start..])?;
    if !matches!(
        window.first(),
        Some(DurableAgentEvent::MessageAppended { .. })
    ) {
        tracing::warn!(
            path = %path.display(),
            window_start_line = checkpoint.window_start_line,
            "compaction checkpoint window does not start at a retained message; falling back to full replay"
        );
        return read_events(run_dir);
    }
    let matches_checkpoint = window.iter().any(|event| {
        matches!(
            event,
            DurableAgentEvent::TranscriptCompacted {
                upto,
                summary,
                compacted_iteration,
            } if *upto == checkpoint.upto
                && *compacted_iteration == checkpoint.compacted_iteration
                && summary_digest(summary) == checkpoint.summary_digest
        )
    });
    if !matches_checkpoint {
        tracing::warn!(
            path = %path.display(),
            compacted_iteration = checkpoint.compacted_iteration,
            "compaction checkpoint matches no compaction event in the window; falling back to full replay"
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
        HookDecisionRecord, HookInvocationId, HookPoint, PrepareNextTurnDecisionRecord, TokenUsage,
        ToolKind, ToolName,
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

    #[tokio::test]
    async fn checkpoint_flush_failure_degrades_without_failing_the_boundary_append() {
        let run = TestRunDir::new("checkpoint-flush-failure");
        // Make the index path a directory so every checkpoint append fails.
        std::fs::create_dir(compact_index_path(&run))
            .expect("index path should be creatable as a directory");

        let sink = FilesystemDurableEventSink::new(run.path());
        sink.append(DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        })
        .await
        .expect("append should succeed");
        sink.append(compacted_event(1, 1, "summary"))
            .await
            .expect("compaction append should succeed");
        sink.append(DurableAgentEvent::IterationCompleted {
            iteration: 1,
            usage: usage(),
        })
        .await
        .expect("boundary append must tolerate a failed checkpoint flush");

        let events = read_events(run.path()).expect("event log should be readable");
        assert_eq!(events.len(), 3);
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

    fn prepare_pending(iteration: u64) -> DurableAgentEvent {
        DurableAgentEvent::HookInvocationPending {
            invocation_id: HookInvocationId::new(),
            point: HookPoint::PrepareNextTurn,
            iteration,
            call_id: None,
            input_digest: "b".repeat(64).parse().expect("valid digest"),
        }
    }

    fn prepare_completed_compact(upto: usize, summary_text: &str) -> DurableAgentEvent {
        DurableAgentEvent::HookInvocationCompleted {
            invocation_id: HookInvocationId::new(),
            decision: HookDecisionRecord::PrepareNextTurn(PrepareNextTurnDecisionRecord::Compact {
                upto,
                summary: ChatMessage::system(summary_text),
            }),
        }
    }

    /// A run compacted twice, each compaction committed right before its own
    /// iteration boundary. Line numbers (1-based):
    ///
    /// ```text
    ///  1 LoopStarted
    ///  2 m1  (committed idx 0)      3 m2  (committed idx 1)
    ///  4 IterationCompleted{0}
    ///  5 m3  (committed idx 2, retained by compaction 1)
    ///  6 prepare Pending            7 prepare Completed(Compact upto=2)
    ///  8 TranscriptCompacted#1      9 IterationCompleted{1}  → checkpoint 1 (window line 5)
    /// 10 m4  (committed idx 2 after compaction 1)
    /// 11 m5  (committed idx 3, retained by compaction 2)
    /// 12 prepare Pending           13 prepare Completed(Compact upto=3)
    /// 14 TranscriptCompacted#2     15 IterationCompleted{2}  → checkpoint 2 (window line 11)
    /// ```
    fn compacted_run_events() -> Vec<DurableAgentEvent> {
        const SUMMARY_1: &str = "[stratum:transcript-compacted]\nfirst summary";
        const SUMMARY_2: &str = "[stratum:transcript-compacted]\nsecond summary";
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
            DurableAgentEvent::IterationCompleted {
                iteration: 0,
                usage: usage(),
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("m3"),
            },
            prepare_pending(1),
            prepare_completed_compact(2, SUMMARY_1),
            compacted_event(2, 1, SUMMARY_1),
            DurableAgentEvent::IterationCompleted {
                iteration: 1,
                usage: usage(),
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("m4"),
            },
            DurableAgentEvent::MessageAppended {
                message: ChatMessage::user("m5"),
            },
            prepare_pending(2),
            prepare_completed_compact(3, SUMMARY_2),
            compacted_event(3, 2, SUMMARY_2),
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

    fn assert_canonical_digest(digest: &str) {
        assert!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "digest must be lowercase hex sha-256"
        );
    }

    #[tokio::test]
    async fn compaction_checkpoint_is_written_only_after_iteration_boundary() {
        let run = TestRunDir::new("checkpoint-write-after-boundary");
        let sink = FilesystemDurableEventSink::new(run.path());
        let events = compacted_run_events();

        // Through the first TranscriptCompacted, before its IterationCompleted:
        // the compaction is durable but no checkpoint may exist yet.
        for event in &events[..8] {
            sink.append(event.clone())
                .await
                .expect("append should succeed");
        }
        assert!(
            !compact_index_path(&run).exists(),
            "no checkpoint before the compaction's iteration boundary"
        );

        // The boundary landing flushes the pending checkpoint.
        sink.append(events[8].clone())
            .await
            .expect("append should succeed");
        let checkpoints = read_checkpoints(&run);
        assert_eq!(checkpoints.len(), 1);
        let first = &checkpoints[0];
        assert_eq!(first.compacted_iteration, 1);
        assert_eq!(first.window_start_line, 5, "first retained message is m3");
        assert_eq!(first.upto, 2);
        assert_canonical_digest(&first.summary_digest);
        let DurableAgentEvent::TranscriptCompacted { summary, .. } = &events[7] else {
            panic!("event at line 8 must be the first compaction");
        };
        assert_eq!(first.summary_digest, summary_digest(summary));

        // The second compaction flushes its own checkpoint at its own
        // boundary; its window start accounts for the first compaction.
        for event in &events[9..] {
            sink.append(event.clone())
                .await
                .expect("append should succeed");
        }
        let checkpoints = read_checkpoints(&run);
        assert_eq!(checkpoints.len(), 2);
        let latest = &checkpoints[1];
        assert_eq!(latest.compacted_iteration, 2);
        assert_eq!(latest.window_start_line, 11, "first retained message is m5");
        assert_eq!(latest.upto, 3);
        let DurableAgentEvent::TranscriptCompacted { summary, .. } = &events[13] else {
            panic!("event at line 14 must be the second compaction");
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
    async fn checkpoint_read_returns_loop_started_plus_self_contained_window() {
        let run = TestRunDir::new("checkpoint-fast-path");
        let events = compacted_run_events();
        write_events(&run, &events).await;

        let fast = read_events_from_checkpoint(run.path()).expect("checkpoint read should succeed");

        // The fast path returns LoopStarted followed by the window starting
        // at the first retained message of the latest compaction.
        let mut expected = vec![events[0].clone()];
        expected.extend(events[10..].iter().cloned());
        assert_eq!(fast, expected);
        // The window is self-contained: it starts at a message and carries
        // the prepare journal records, the compaction, and the boundary.
        assert!(
            matches!(&fast[1], DurableAgentEvent::MessageAppended { .. }),
            "window must start at the first retained message"
        );
        assert!(
            fast.iter()
                .any(|event| matches!(event, DurableAgentEvent::HookInvocationCompleted { .. }))
        );
        assert!(fast.iter().any(|event| matches!(
            event,
            DurableAgentEvent::TranscriptCompacted {
                compacted_iteration: 2,
                ..
            }
        )));
        assert!(fast.iter().any(|event| matches!(
            event,
            DurableAgentEvent::IterationCompleted { iteration: 2, .. }
        )));
        // The window matches the full stream's tail event for event.
        let full = read_events(run.path()).expect("full replay should succeed");
        assert_eq!(full, events);
        assert_eq!(fast[0], full[0]);
        assert_eq!(fast[1..], full[10..]);
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
        raw.push_str("{\"compacted_iteration\":2,\"window_start_line\":11");
        std::fs::write(compact_index_path(&run), raw).expect("index should be writable");

        let fast = read_events_from_checkpoint(run.path()).expect("checkpoint read should succeed");

        let mut expected = vec![events[0].clone()];
        expected.extend(events[4..].iter().cloned());
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
    async fn checkpoint_with_mismatched_window_start_line_falls_back_to_full_replay() {
        let run = TestRunDir::new("checkpoint-tampered-line");
        let events = compacted_run_events();
        write_events(&run, &events).await;
        let mut checkpoints = read_checkpoints(&run);
        // Tamper: point the window start at the prepare journal line, which
        // is not a message event.
        checkpoints[1].window_start_line = 12;
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
        // Simulate a crash after the second IterationCompleted landed but
        // before its checkpoint was appended: drop the last index line.
        let checkpoints = read_checkpoints(&run);
        rewrite_checkpoint_index(&run, &checkpoints[..1]);

        let fast = read_events_from_checkpoint(run.path()).expect("checkpoint read should succeed");

        let mut expected = vec![events[0].clone()];
        expected.extend(events[4..].iter().cloned());
        assert_eq!(fast, expected);
    }

    #[tokio::test]
    async fn uncommitted_boundary_leaves_no_checkpoint_and_falls_back_to_full_replay() {
        let run = TestRunDir::new("checkpoint-uncommitted-boundary");
        let events = compacted_run_events();
        // Simulate a crash after the first TranscriptCompacted landed but
        // before its IterationCompleted: stop right after the compaction.
        write_events(&run, &events[..8]).await;

        assert!(
            !compact_index_path(&run).exists(),
            "the crash window must not produce a checkpoint"
        );
        assert_eq!(
            read_events_from_checkpoint(run.path()).expect("read should succeed"),
            events[..8]
        );
    }
}
