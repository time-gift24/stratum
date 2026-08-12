//! Historical baseline materialization for fresh Turns and resume.
//!
//! Given an Agent and a fixed `base_event_seq`, this module rebuilds the
//! committed provider context at that barrier: either the fast path (latest
//! valid compaction companion's summary plus the retained message suffix) or
//! an in-memory full replay from sequence 1 that applies every
//! `TranscriptCompacted` with the same cut semantics as kernel replay. An
//! invalid retained pointer only discards the acceleration and falls back to
//! full replay; a missing companion or malformed summary fails closed as
//! `durable_state_corrupt`. Stored rows are never modified.
//!
//! Historical terminal Turns are normalized (design 7.10): a failed or
//! cancelled Turn ending with an unclosed assistant tool-call group is
//! adjusted only in the in-memory view — zero results drop the trailing
//! group, an exact ordered prefix of `k` results keeps those results and
//! trims the assistant `tool_calls` to the same prefix. A finished Turn with
//! an unclosed group, or a gap in the middle of history, is
//! `durable_state_corrupt`. The current running Turn is never part of the
//! baseline and therefore never normalized.

use stratum_core::{AgentRuntimeId, ChatMessage, ChatRole, DurableAgentEvent};
use stratum_postgres::PostgresBackend;

use crate::error::{ApiError, BaselineCorruptError, ErrorKind};
use crate::provenance::ContextLineage;

/// Materialized committed context at one historical barrier.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Baseline {
    /// Committed messages in provider order.
    pub(crate) messages: Vec<ChatMessage>,
    /// Origin event sequence per message (`None` for summary markers).
    pub(crate) lineage: ContextLineage,
}

/// One durable row reduced to what baseline assembly needs; kept local so the
/// assembly logic is unit-testable without constructing store rows.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LedgerEvent {
    /// AgentRuntime-wide event sequence.
    event_seq: u64,
    /// Typed durable event (compactions are already companion-materialized).
    event: DurableAgentEvent,
}

impl LedgerEvent {
    /// Builds one assembly input row.
    #[must_use]
    pub(crate) fn new(event_seq: u64, event: DurableAgentEvent) -> Self {
        Self { event_seq, event }
    }
}

/// Replay mode of one assembly pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReplayMode {
    /// Full replay from the ledger start: the first row must be a
    /// `LoopStarted` and every compaction cut is applied.
    Full,
    /// Accelerated window behind a companion summary: the window starts
    /// mid-Turn and compaction discriminators are skipped (the seed summary
    /// already embodies every compaction at or below the base).
    Accelerated,
}

/// Materializes the historical committed context at `base_event_seq`.
///
/// # Errors
///
/// Returns [`ErrorKind::DurableStateCorrupt`] when durable truth is
/// incomplete or inconsistent, [`ErrorKind::StoreUnavailable`] on storage
/// failure, and [`ErrorKind::RuntimeIncompatible`] for unsupported versions.
pub(crate) async fn materialize_baseline(
    pg: &PostgresBackend,
    agent_runtime_id: AgentRuntimeId,
    base_event_seq: u64,
) -> Result<Baseline, ApiError> {
    if base_event_seq == 0 {
        return Ok(Baseline {
            messages: Vec::new(),
            lineage: ContextLineage::default(),
        });
    }

    let companion = pg
        .read_latest_companion(agent_runtime_id, base_event_seq)
        .await
        .map_err(ApiError::from_postgres)?;
    if let Some(companion) = companion {
        let pointer = companion.retained_from_event_seq;
        if pointer >= 1 && pointer <= base_event_seq {
            let window_from = pointer
                .checked_sub(1)
                .ok_or_else(|| ApiError::new(ErrorKind::DurableStateCorrupt))?;
            let rows =
                read_checked_range(pg, agent_runtime_id, window_from, base_event_seq).await?;
            if retained_window_is_usable(pointer, base_event_seq, &rows) {
                return assemble(rows, Some(companion.summary), ReplayMode::Accelerated)
                    .map_err(corrupt);
            }
            // The retained pointer cannot serve as an acceleration start:
            // ignore it and replay fully in memory (the summary itself stays
            // available through the permanent event rows).
        }
    }
    full_replay(pg, agent_runtime_id, base_event_seq).await
}

/// Whether a companion's retained pointer can start an accelerated replay
/// window. Invalid pointers are locators only: callers fall back to full
/// in-memory replay and never repair durable state.
fn retained_window_is_usable(
    retained_from_event_seq: u64,
    base_event_seq: u64,
    rows: &[LedgerEvent],
) -> bool {
    retained_from_event_seq >= 1
        && retained_from_event_seq <= base_event_seq
        && matches!(
            rows.first(),
            Some(row) if row.event_seq == retained_from_event_seq
                && matches!(row.event, DurableAgentEvent::MessageAppended { .. })
        )
}

/// In-memory full replay from the ledger start.
async fn full_replay(
    pg: &PostgresBackend,
    agent_runtime_id: AgentRuntimeId,
    base_event_seq: u64,
) -> Result<Baseline, ApiError> {
    let rows = read_checked_range(pg, agent_runtime_id, 0, base_event_seq).await?;
    assemble(rows, None, ReplayMode::Full).map_err(corrupt)
}

/// Reads `(from, to]` verifying the truth range is gapless, including the
/// tail: the last returned row must be exactly `to_event_seq`, otherwise the
/// high-water points past the retained rows and continuing would cement a
/// truth gap.
async fn read_checked_range(
    pg: &PostgresBackend,
    agent_runtime_id: AgentRuntimeId,
    from_event_seq: u64,
    to_event_seq: u64,
) -> Result<Vec<LedgerEvent>, ApiError> {
    let rows = pg
        .read_events_range(agent_runtime_id, from_event_seq, to_event_seq)
        .await
        .map_err(ApiError::from_postgres)?;
    let events: Vec<LedgerEvent> = rows
        .into_iter()
        .map(|row| LedgerEvent::new(row.event_seq, row.event))
        .collect();
    check_gapless(from_event_seq, to_event_seq, &events).map_err(corrupt)?;
    Ok(events)
}

/// Pure continuity check of one `(from, to]` window; separated from the store
/// read so the gap matrix is unit-testable.
fn check_gapless(
    from_event_seq: u64,
    to_event_seq: u64,
    events: &[LedgerEvent],
) -> Result<(), BaselineCorruptError> {
    let mut expected = from_event_seq;
    for row in events {
        expected = expected.checked_add(1).ok_or(BaselineCorruptError(
            "historical truth range sequence overflowed",
        ))?;
        if row.event_seq != expected {
            return Err(BaselineCorruptError(
                "historical truth range has missing rows",
            ));
        }
    }
    if expected != to_event_seq {
        return Err(BaselineCorruptError(
            "historical truth range has missing rows",
        ));
    }
    Ok(())
}

fn corrupt(error: BaselineCorruptError) -> ApiError {
    ApiError::with_source(ErrorKind::DurableStateCorrupt, error)
}

/// Terminal boundary kind of one historical Turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TerminalKind {
    Finished,
    Failed,
    Cancelled,
}

/// Assembles the baseline from ordered rows plus an optional summary seed.
///
/// This is the pure half of [`materialize_baseline`]: turn-boundary
/// detection, kernel-equivalent compaction cuts, and the historical
/// terminal-Turn normalization matrix.
pub(crate) fn assemble(
    rows: Vec<LedgerEvent>,
    seed: Option<ChatMessage>,
    mode: ReplayMode,
) -> Result<Baseline, BaselineCorruptError> {
    let mut messages: Vec<ChatMessage> = Vec::new();
    let mut origins: Vec<Option<u64>> = Vec::new();
    if let Some(summary) = seed {
        messages.push(summary);
        origins.push(None);
    }
    let mut open = mode == ReplayMode::Accelerated;
    let mut segment_start = 0_usize;

    for row in &rows {
        match &row.event {
            DurableAgentEvent::LoopStarted { .. } => {
                if open {
                    return Err(BaselineCorruptError(
                        "a new turn started before the previous turn's terminal boundary",
                    ));
                }
                open = true;
                segment_start = messages.len();
            }
            DurableAgentEvent::MessageAppended { message } => {
                if !open {
                    return Err(BaselineCorruptError("message outside any turn boundary"));
                }
                messages.push(message.clone());
                origins.push(Some(row.event_seq));
            }
            DurableAgentEvent::TranscriptCompacted { upto, summary, .. } => {
                if mode == ReplayMode::Accelerated {
                    // The seed summary embodies every compaction at or below
                    // the base; the window only carries their discriminators.
                    continue;
                }
                if !open {
                    return Err(BaselineCorruptError("compaction outside any turn boundary"));
                }
                let upto = usize::try_from(*upto).unwrap_or(usize::MAX);
                if upto == 0 {
                    return Err(BaselineCorruptError("compaction cut is zero"));
                }
                if upto <= messages.len() {
                    // Full-stream cut, mirroring kernel replay.
                    messages.splice(..upto, std::iter::once(summary.clone()));
                    origins.splice(..upto, std::iter::once(None));
                    segment_start = if segment_start >= upto {
                        segment_start
                            .checked_sub(upto)
                            .and_then(|start| start.checked_add(1))
                            .ok_or(BaselineCorruptError("historical baseline index overflowed"))?
                    } else {
                        1
                    };
                } else {
                    // Overshooting cut: the kernel accepts this as a
                    // checkpoint window whose rebuilt context is already the
                    // retained suffix; mirror that acceptance.
                    messages.insert(0, summary.clone());
                    origins.insert(0, None);
                    segment_start = 1;
                }
            }
            DurableAgentEvent::LoopFinished { .. } => {
                close_segment(
                    &mut messages,
                    &mut origins,
                    segment_start,
                    TerminalKind::Finished,
                )?;
                open = false;
            }
            DurableAgentEvent::LoopFailed { .. } => {
                close_segment(
                    &mut messages,
                    &mut origins,
                    segment_start,
                    TerminalKind::Failed,
                )?;
                open = false;
            }
            DurableAgentEvent::LoopCancelled { .. } => {
                close_segment(
                    &mut messages,
                    &mut origins,
                    segment_start,
                    TerminalKind::Cancelled,
                )?;
                open = false;
            }
            // Internal execution facts carry no provider context.
            DurableAgentEvent::ToolExecutionStarted { .. }
            | DurableAgentEvent::HookInvocationPending { .. }
            | DurableAgentEvent::HookInvocationCompleted { .. }
            | DurableAgentEvent::HookInvocationFailed { .. }
            | DurableAgentEvent::IterationCompleted { .. }
            | DurableAgentEvent::ToolApprovalRequested { .. }
            | DurableAgentEvent::ToolApprovalResolved { .. } => {}
            _ => {
                return Err(BaselineCorruptError("unsupported durable event in history"));
            }
        }
    }
    if open && !rows.is_empty() {
        return Err(BaselineCorruptError(
            "last historical turn has no terminal boundary",
        ));
    }
    Ok(Baseline {
        messages,
        lineage: ContextLineage::from_origins(origins),
    })
}

/// Validates one closed Turn segment and applies the terminal normalization.
fn close_segment(
    messages: &mut Vec<ChatMessage>,
    origins: &mut Vec<Option<u64>>,
    segment_start: usize,
    terminal: TerminalKind,
) -> Result<(), BaselineCorruptError> {
    let mut index = segment_start;
    while index < messages.len() {
        let message = &messages[index];
        if message.role == ChatRole::Tool {
            return Err(BaselineCorruptError(
                "tool result outside its assistant tool-call group",
            ));
        }
        if message.role == ChatRole::Assistant && !message.tool_calls.is_empty() {
            let tool_calls = message.tool_calls.clone();
            let mut committed = 0_usize;
            while committed < tool_calls.len() {
                let result_index = index
                    .checked_add(1)
                    .and_then(|next| next.checked_add(committed))
                    .ok_or(BaselineCorruptError("historical baseline index overflowed"))?;
                let Some(result) = messages.get(result_index) else {
                    break;
                };
                if result.role != ChatRole::Tool {
                    break;
                }
                if result.tool_call_id.as_ref() != Some(&tool_calls[committed].call_id) {
                    return Err(BaselineCorruptError(
                        "tool result does not match the expected call order",
                    ));
                }
                committed = committed
                    .checked_add(1)
                    .ok_or(BaselineCorruptError("historical baseline index overflowed"))?;
            }
            let group_end = index
                .checked_add(1)
                .and_then(|next| next.checked_add(committed))
                .ok_or(BaselineCorruptError("historical baseline index overflowed"))?;
            if committed == tool_calls.len() {
                index = group_end;
                continue;
            }
            let trailing = group_end == messages.len();
            let recoverable = matches!(terminal, TerminalKind::Failed | TerminalKind::Cancelled);
            if !trailing || !recoverable {
                return Err(BaselineCorruptError(
                    "unclosed assistant tool-call group in committed history",
                ));
            }
            // Terminal-trailing group of a failed/cancelled Turn: adjust only
            // the in-memory view, never the stored rows.
            if committed == 0 {
                messages.remove(index);
                origins.remove(index);
            } else {
                messages[index].tool_calls.truncate(committed);
            }
            return Ok(());
        }
        index = index
            .checked_add(1)
            .ok_or(BaselineCorruptError("historical baseline index overflowed"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stratum_core::{CallId, TokenUsage, ToolCall};

    use super::*;

    fn user(text: &str) -> ChatMessage {
        ChatMessage::user(text)
    }

    fn assistant_with_calls(call_ids: &[&str]) -> ChatMessage {
        ChatMessage::assistant("").with_tool_calls(
            call_ids
                .iter()
                .map(|call_id| ToolCall {
                    call_id: CallId::from(*call_id),
                    name: "echo".to_owned(),
                    arguments: json!({}),
                })
                .collect(),
        )
    }

    fn tool_result(call_id: &str) -> ChatMessage {
        ChatMessage::tool(CallId::from(call_id), json!({"ok": true}))
    }

    fn loop_started(seq: u64) -> LedgerEvent {
        LedgerEvent::new(
            seq,
            DurableAgentEvent::LoopStarted {
                extension_set_version_id: None,
            },
        )
    }

    fn message(seq: u64, message: ChatMessage) -> LedgerEvent {
        LedgerEvent::new(seq, DurableAgentEvent::MessageAppended { message })
    }

    fn terminal(seq: u64, kind: TerminalKind) -> LedgerEvent {
        let usage = TokenUsage::default();
        let event = match kind {
            TerminalKind::Finished => DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage,
            },
            TerminalKind::Failed => DurableAgentEvent::LoopFailed {
                error_text: "failed".to_owned(),
                usage,
            },
            TerminalKind::Cancelled => DurableAgentEvent::LoopCancelled { usage },
        };
        LedgerEvent::new(seq, event)
    }

    fn compacted(seq: u64, upto: u64, summary: &str) -> LedgerEvent {
        LedgerEvent::new(
            seq,
            DurableAgentEvent::TranscriptCompacted {
                upto,
                summary: ChatMessage::system(summary),
                compacted_iteration: 0,
            },
        )
    }

    #[test]
    fn checked_range_rejects_middle_and_tail_gaps() {
        // A contiguous window passes.
        assert!(
            check_gapless(
                0,
                3,
                &[
                    message(1, user("a")),
                    message(2, user("b")),
                    message(3, user("c"))
                ]
            )
            .is_ok()
        );
        // A missing row in the middle is corruption.
        assert!(check_gapless(0, 3, &[message(1, user("a")), message(3, user("c"))]).is_err());
        // A missing tail row (high-water ahead of the retained rows) is
        // corruption too — continuing would cement the truth gap.
        assert!(check_gapless(0, 3, &[message(1, user("a")), message(2, user("b"))]).is_err());
        // An empty window exactly at the frontier is valid.
        assert!(check_gapless(3, 3, &[]).is_ok());
        // Arithmetic overflow cannot wrap continuity back to zero.
        assert!(check_gapless(u64::MAX, u64::MAX, &[message(0, user("x"))]).is_err());
    }

    #[test]
    fn retained_pointer_only_accelerates_from_its_exact_message_row() {
        let valid = vec![
            message(4, user("retained")),
            terminal(5, TerminalKind::Finished),
        ];
        assert!(retained_window_is_usable(4, 5, &valid));
        assert!(!retained_window_is_usable(0, 5, &valid));
        assert!(!retained_window_is_usable(6, 5, &valid));
        assert!(!retained_window_is_usable(3, 5, &valid));

        let non_message = vec![loop_started(4), message(5, user("retained"))];
        assert!(!retained_window_is_usable(4, 5, &non_message));
    }

    #[test]
    fn full_replay_collects_messages_across_terminal_turns() {
        let baseline = assemble(
            vec![
                loop_started(1),
                message(2, user("one")),
                message(3, ChatMessage::assistant("answer one")),
                terminal(4, TerminalKind::Finished),
                loop_started(5),
                message(6, user("two")),
                message(7, ChatMessage::assistant("answer two")),
                terminal(8, TerminalKind::Finished),
            ],
            None,
            ReplayMode::Full,
        )
        .expect("clean history assembles");

        assert_eq!(baseline.messages.len(), 4);
        assert_eq!(
            baseline.lineage,
            ContextLineage::from_origins(vec![Some(2), Some(3), Some(6), Some(7)])
        );
    }

    #[test]
    fn full_replay_applies_compaction_cuts_like_the_kernel() {
        let baseline = assemble(
            vec![
                loop_started(1),
                message(2, user("one")),
                message(3, user("two")),
                message(4, user("three")),
                compacted(5, 2, "summary so far"),
                message(6, ChatMessage::assistant("answer")),
                terminal(7, TerminalKind::Finished),
            ],
            None,
            ReplayMode::Full,
        )
        .expect("compaction applies");

        assert_eq!(
            baseline.messages,
            vec![
                ChatMessage::system("summary so far"),
                user("three"),
                ChatMessage::assistant("answer"),
            ]
        );
        assert_eq!(
            baseline.lineage,
            ContextLineage::from_origins(vec![None, Some(4), Some(6)])
        );
    }

    #[test]
    fn accelerated_window_uses_seed_and_skips_compaction_discriminators() {
        let baseline = assemble(
            vec![
                message(4, user("three")),
                compacted(5, 2, "summary so far"),
                message(6, ChatMessage::assistant("answer")),
                terminal(7, TerminalKind::Finished),
            ],
            Some(ChatMessage::system("summary so far")),
            ReplayMode::Accelerated,
        )
        .expect("window assembles");

        assert_eq!(
            baseline.messages,
            vec![
                ChatMessage::system("summary so far"),
                user("three"),
                ChatMessage::assistant("answer"),
            ]
        );
        assert_eq!(
            baseline.lineage,
            ContextLineage::from_origins(vec![None, Some(4), Some(6)])
        );
    }

    #[test]
    fn cancelled_turn_with_zero_results_drops_the_trailing_group() {
        let baseline = assemble(
            vec![
                loop_started(1),
                message(2, user("do it")),
                message(3, assistant_with_calls(&["call-1", "call-2"])),
                terminal(4, TerminalKind::Cancelled),
            ],
            None,
            ReplayMode::Full,
        )
        .expect("cancelled trailing group is normalized");

        assert_eq!(baseline.messages, vec![user("do it")]);
        assert_eq!(
            baseline.lineage,
            ContextLineage::from_origins(vec![Some(2)])
        );
    }

    #[test]
    fn failed_turn_with_partial_results_keeps_the_exact_prefix() {
        let baseline = assemble(
            vec![
                loop_started(1),
                message(2, user("do it")),
                message(3, assistant_with_calls(&["call-1", "call-2", "call-3"])),
                message(4, tool_result("call-1")),
                message(5, tool_result("call-2")),
                terminal(6, TerminalKind::Failed),
            ],
            None,
            ReplayMode::Full,
        )
        .expect("partial results are kept");

        let mut expected_assistant = assistant_with_calls(&["call-1", "call-2", "call-3"]);
        expected_assistant.tool_calls.truncate(2);
        assert_eq!(
            baseline.messages,
            vec![
                user("do it"),
                expected_assistant,
                tool_result("call-1"),
                tool_result("call-2")
            ]
        );
        assert_eq!(
            baseline.lineage,
            ContextLineage::from_origins(vec![Some(2), Some(3), Some(4), Some(5)])
        );
    }

    #[test]
    fn finished_turn_with_unclosed_group_is_corrupt() {
        let error = assemble(
            vec![
                loop_started(1),
                message(2, user("do it")),
                message(3, assistant_with_calls(&["call-1"])),
                terminal(4, TerminalKind::Finished),
            ],
            None,
            ReplayMode::Full,
        )
        .expect_err("finished turn must close its tool groups");
        assert_eq!(
            error,
            BaselineCorruptError("unclosed assistant tool-call group in committed history")
        );
    }

    #[test]
    fn mid_history_gaps_are_corrupt_even_in_failed_turns() {
        let error = assemble(
            vec![
                loop_started(1),
                message(2, assistant_with_calls(&["call-1", "call-2"])),
                message(3, tool_result("call-1")),
                message(4, user("interleaved")),
                terminal(5, TerminalKind::Failed),
            ],
            None,
            ReplayMode::Full,
        )
        .expect_err("non-trailing gaps are corruption");
        assert_eq!(
            error,
            BaselineCorruptError("unclosed assistant tool-call group in committed history")
        );

        let out_of_order = assemble(
            vec![
                loop_started(1),
                message(2, assistant_with_calls(&["call-1", "call-2"])),
                message(3, tool_result("call-2")),
                terminal(4, TerminalKind::Cancelled),
            ],
            None,
            ReplayMode::Full,
        )
        .expect_err("out-of-order results are corruption");
        assert_eq!(
            out_of_order,
            BaselineCorruptError("tool result does not match the expected call order")
        );
    }

    #[test]
    fn missing_boundaries_are_corrupt() {
        // Full replay must start at a loop boundary.
        assert!(assemble(vec![message(1, user("one"))], None, ReplayMode::Full).is_err());
        // A turn that never terminates below the base is corrupt.
        assert!(
            assemble(
                vec![loop_started(1), message(2, user("one"))],
                None,
                ReplayMode::Full
            )
            .is_err()
        );
        // A new turn before the previous terminal is corrupt.
        assert!(
            assemble(
                vec![loop_started(1), message(2, user("one")), loop_started(3)],
                None,
                ReplayMode::Full
            )
            .is_err()
        );
        // A zero compaction cut is corrupt.
        assert!(
            assemble(
                vec![
                    loop_started(1),
                    message(2, user("one")),
                    compacted(3, 0, "bad"),
                    terminal(4, TerminalKind::Finished),
                ],
                None,
                ReplayMode::Full
            )
            .is_err()
        );
    }
}
