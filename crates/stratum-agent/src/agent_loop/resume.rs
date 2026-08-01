//! Event-stream replay for agent-loop resume.
//!
//! The composing side reads a run's durable event stream and hands it to
//! [`AgentLoop::resume`](super::AgentLoop::resume) together with a freshly
//! supplied system prompt and run configuration; the kernel never touches
//! storage itself. Replay rebuilds the committed context from the
//! `MessageAppended` sequence, fixes the iteration frontier at one past the
//! maximum committed `IterationCompleted`, reconciles committed tool results
//! against their preceding assistant `tool_calls`, and reconstructs the hook
//! invocation journal.
//!
//! Tool execution stays at-least-once: a call whose `ToolExecutionStarted`
//! committed without a result has an unknown outcome and simply re-executes as
//! part of the missing result suffix. Terminal events (`LoopFinished`,
//! `LoopFailed`, `LoopCancelled`) make a run non-resumable.

use stratum_core::{ChatMessage, ChatRole, DurableAgentEvent, ExtensionSetVersionId, ToolCall};

use super::ResumeError;
use super::journal::{HookAddress, HookJournal};

/// Run state rebuilt from one durable event stream.
#[derive(Debug)]
pub(crate) struct ReplayState {
    /// Rebuilt committed transcript.
    pub(crate) messages: Vec<ChatMessage>,
    /// Next iteration index: one past the maximum committed
    /// `IterationCompleted`, or zero when no iteration completed.
    pub(crate) frontier: u64,
    /// Work the resumed run must finish at the frontier iteration before new
    /// model requests, when the stream ended mid-iteration.
    pub(crate) continuation: Option<ResumeContinuation>,
    /// Hook invocation journal reconstructed from the stream.
    pub(crate) journal: HookJournal,
    /// Extension set version the run pinned at `LoopStarted`, when the hook
    /// runtime reported one.
    pub(crate) extension_set_version_id: Option<ExtensionSetVersionId>,
}

/// Work the resumed run must finish at the frontier iteration.
#[derive(Debug)]
pub(crate) enum ResumeContinuation {
    /// Execute the missing ordered suffix of the trailing assistant's tool
    /// calls, then close the iteration through the regular prepare boundary.
    ToolSuffix(Vec<ToolCall>),
    /// Close the frontier iteration: run `prepare_next_turn` and commit the
    /// iteration boundary.
    CloseIteration,
    /// Close the frontier iteration and finish the loop. The trailing
    /// assistant response carried no tool calls, so the only lost work is the
    /// terminal boundary itself; its finish reason is not part of the durable
    /// stream and is projected as `stop`.
    FinishLoop,
}

/// Replays one run's durable event stream into a resumable state.
///
/// # Errors
///
/// Returns a typed [`ResumeError`] for a missing or duplicated `LoopStarted`,
/// a terminal event, corrupted tool-result history, a corrupted hook
/// journal, or an event variant this kernel does not understand; every
/// failure refuses the resume closed.
#[tracing::instrument(level = "debug", skip_all, fields(event_count = events.len()))]
pub(crate) fn replay_events(events: Vec<DurableAgentEvent>) -> Result<ReplayState, ResumeError> {
    let mut messages = Vec::new();
    let mut frontier = 0_u64;
    let mut seen_loop_started = false;
    let mut activity_after_frontier = false;
    let mut journal = HookJournal::default();
    let mut extension_set_version_id = None;
    for (event_index, event) in events.into_iter().enumerate() {
        let event_type = event.event_type();
        match event {
            DurableAgentEvent::LoopStarted {
                extension_set_version_id: recorded,
            } => {
                if seen_loop_started {
                    tracing::warn!(
                        event_index,
                        event_type,
                        "refusing resume: duplicate loop_started"
                    );
                    return Err(ResumeError::UnexpectedLoopStarted);
                }
                seen_loop_started = true;
                extension_set_version_id = recorded;
            }
            DurableAgentEvent::MessageAppended { message } => {
                messages.push(message);
                activity_after_frontier = true;
            }
            DurableAgentEvent::IterationCompleted { iteration, .. } => {
                frontier = frontier.max(iteration.saturating_add(1));
                activity_after_frontier = false;
            }
            DurableAgentEvent::LoopFinished { .. }
            | DurableAgentEvent::LoopFailed { .. }
            | DurableAgentEvent::LoopCancelled { .. } => {
                tracing::warn!(
                    event_index,
                    event_type,
                    "refusing resume: stream contains a terminal event"
                );
                return Err(ResumeError::TerminalEvent);
            }
            DurableAgentEvent::HookInvocationPending {
                invocation_id,
                point,
                iteration,
                call_id,
                input_digest,
            } => {
                journal
                    .record_pending(
                        HookAddress::new(iteration, point, call_id),
                        invocation_id,
                        input_digest,
                    )
                    .map_err(|error| {
                        tracing::warn!(
                            event_index,
                            event_type,
                            error = %error,
                            "refusing resume: corrupted hook journal"
                        );
                        error
                    })?;
                activity_after_frontier = true;
            }
            DurableAgentEvent::HookInvocationCompleted {
                invocation_id,
                decision,
            } => {
                journal
                    .record_completed(&invocation_id, decision)
                    .map_err(|error| {
                        tracing::warn!(
                            event_index,
                            event_type,
                            error = %error,
                            "refusing resume: corrupted hook journal"
                        );
                        error
                    })?;
                activity_after_frontier = true;
            }
            DurableAgentEvent::HookInvocationFailed {
                invocation_id,
                failure,
            } => {
                journal
                    .record_failed(&invocation_id, failure)
                    .map_err(|error| {
                        tracing::warn!(
                            event_index,
                            event_type,
                            error = %error,
                            "refusing resume: corrupted hook journal"
                        );
                        error
                    })?;
                activity_after_frontier = true;
            }
            DurableAgentEvent::ToolExecutionStarted { .. } => {
                // At-least-once stance: a started call without a committed
                // result has an unknown outcome and re-executes through the
                // missing result suffix; no replay state is needed.
                activity_after_frontier = true;
            }
            // Approval events are legacy-only and carry no kernel resume
            // state, so they are safe to skip.
            DurableAgentEvent::ToolApprovalRequested { .. }
            | DurableAgentEvent::ToolApprovalResolved { .. } => {}
            // An unknown future variant may carry resume state this kernel
            // cannot rebuild; refuse the resume closed instead of silently
            // dropping it.
            _ => {
                tracing::warn!(
                    event_index,
                    event_type,
                    "refusing resume: stream contains an unsupported event type"
                );
                return Err(ResumeError::UnsupportedEvent { event_type });
            }
        }
    }
    if !seen_loop_started {
        tracing::warn!("refusing resume: stream has no loop_started event");
        return Err(ResumeError::MissingLoopStarted);
    }
    let continuation = if activity_after_frontier {
        reconcile_tool_results(&messages)?
    } else {
        None
    };
    Ok(ReplayState {
        messages,
        frontier,
        continuation,
        journal,
        extension_set_version_id,
    })
}

/// Validates that every committed tool result forms the exact ordered prefix
/// of its immediately preceding assistant `tool_calls`, and derives the
/// frontier continuation from the trailing messages.
fn reconcile_tool_results(
    messages: &[ChatMessage],
) -> Result<Option<ResumeContinuation>, ResumeError> {
    let mut index = 0;
    while index < messages.len() {
        let message = &messages[index];
        if message.role == ChatRole::Tool {
            // A tool result outside an assistant tool-call group.
            tracing::warn!(
                message_index = index,
                "refusing resume: tool result outside its assistant tool-call group"
            );
            return Err(ResumeError::ToolResultMismatch);
        }
        if message.role == ChatRole::Assistant && !message.tool_calls.is_empty() {
            let tool_calls = &message.tool_calls;
            let mut committed = 0;
            while committed < tool_calls.len() {
                let Some(result) = messages.get(index + 1 + committed) else {
                    break;
                };
                if result.role != ChatRole::Tool {
                    break;
                }
                if result.tool_call_id.as_ref() != Some(&tool_calls[committed].call_id) {
                    // Unknown, duplicated, or out-of-order result.
                    tracing::warn!(
                        message_index = index + 1 + committed,
                        expected_call_id = %tool_calls[committed].call_id,
                        "refusing resume: tool result does not match the expected call"
                    );
                    return Err(ResumeError::ToolResultMismatch);
                }
                committed += 1;
            }
            if committed < tool_calls.len() {
                if index + 1 + committed != messages.len() {
                    // A sparse group: results missing in the middle of history.
                    tracing::warn!(
                        message_index = index + 1 + committed,
                        "refusing resume: tool results missing in the middle of history"
                    );
                    return Err(ResumeError::ToolResultMismatch);
                }
                return Ok(Some(ResumeContinuation::ToolSuffix(
                    tool_calls[committed..].to_vec(),
                )));
            }
            index += 1 + committed;
        } else {
            index += 1;
        }
    }
    match messages.last() {
        Some(last) if last.role == ChatRole::Assistant && last.tool_calls.is_empty() => {
            Ok(Some(ResumeContinuation::FinishLoop))
        }
        Some(last) if last.role == ChatRole::Tool => Ok(Some(ResumeContinuation::CloseIteration)),
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stratum_core::{
        ApprovalDecision, ApprovalId, CallId, DangerLevel, TokenUsage, ToolKind, ToolName,
    };

    use super::*;

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

    fn stream(messages: Vec<ChatMessage>) -> Vec<DurableAgentEvent> {
        let mut events = vec![DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        }];
        events.extend(
            messages
                .into_iter()
                .map(|message| DurableAgentEvent::MessageAppended { message }),
        );
        events
    }

    #[test]
    fn replay_rebuilds_context_and_frontier() {
        let mut events = stream(vec![
            ChatMessage::user("question"),
            ChatMessage::assistant("answer"),
        ]);
        events.push(DurableAgentEvent::IterationCompleted {
            iteration: 0,
            usage: TokenUsage::default(),
        });
        events.push(DurableAgentEvent::IterationCompleted {
            iteration: 2,
            usage: TokenUsage::default(),
        });

        let replay = replay_events(events).expect("a clean boundary should replay");

        assert_eq!(
            replay.messages,
            vec![
                ChatMessage::user("question"),
                ChatMessage::assistant("answer")
            ]
        );
        assert_eq!(replay.frontier, 3);
        assert!(replay.continuation.is_none());
    }

    #[test]
    fn replay_rejects_terminal_events_and_missing_start() {
        for terminal in [
            DurableAgentEvent::LoopFinished {
                finish_reason: "stop".to_owned(),
                usage: TokenUsage::default(),
            },
            DurableAgentEvent::LoopFailed {
                error_text: "boom".to_owned(),
                usage: TokenUsage::default(),
            },
            DurableAgentEvent::LoopCancelled {
                usage: TokenUsage::default(),
            },
        ] {
            let mut events = stream(vec![ChatMessage::user("question")]);
            events.push(terminal);
            assert_eq!(
                replay_events(events).expect_err("terminal events must refuse resume"),
                ResumeError::TerminalEvent,
            );
        }
        assert_eq!(
            replay_events(Vec::new()).expect_err("an empty stream has no start"),
            ResumeError::MissingLoopStarted
        );
        let mut duplicated = stream(vec![]);
        duplicated.push(DurableAgentEvent::LoopStarted {
            extension_set_version_id: None,
        });
        assert_eq!(
            replay_events(duplicated).expect_err("a duplicated start must refuse resume"),
            ResumeError::UnexpectedLoopStarted
        );
    }

    #[test]
    fn legacy_approval_events_are_skipped_without_resume_state() {
        let mut events = stream(vec![ChatMessage::user("question")]);
        events.push(DurableAgentEvent::ToolApprovalRequested {
            approval_id: ApprovalId::new(),
            call_id: CallId::from("call-1"),
            tool_name: ToolName::from("echo"),
            arguments: json!({}),
            tool_kind: ToolKind::Read,
            danger_level: DangerLevel::Low,
        });
        events.push(DurableAgentEvent::ToolApprovalResolved {
            approval_id: ApprovalId::new(),
            decision: ApprovalDecision::Approve,
        });

        let replay = replay_events(events).expect("legacy approval events should replay");

        assert_eq!(replay.messages, vec![ChatMessage::user("question")]);
        assert!(replay.continuation.is_none());
    }

    #[test]
    fn missing_result_suffix_reexecutes_at_the_frontier() {
        let events = stream(vec![
            ChatMessage::user("question"),
            assistant_with_calls(&["call-1", "call-2", "call-3"]),
            tool_result("call-1"),
            tool_result("call-2"),
        ]);

        let replay = replay_events(events).expect("a missing suffix should replay");

        assert_eq!(replay.frontier, 0);
        let Some(ResumeContinuation::ToolSuffix(suffix)) = replay.continuation else {
            panic!("the missing suffix should re-execute");
        };
        assert_eq!(suffix.len(), 1);
        assert_eq!(suffix[0].call_id, CallId::from("call-3"));
    }

    #[test]
    fn complete_trailing_cycle_closes_its_iteration() {
        let events = stream(vec![
            ChatMessage::user("question"),
            assistant_with_calls(&["call-1"]),
            tool_result("call-1"),
        ]);

        let replay = replay_events(events).expect("a complete cycle should replay");

        assert!(matches!(
            replay.continuation,
            Some(ResumeContinuation::CloseIteration)
        ));
    }

    #[test]
    fn trailing_assistant_without_tool_calls_finishes_the_loop() {
        let events = stream(vec![
            ChatMessage::user("question"),
            ChatMessage::assistant("answer"),
        ]);

        let replay = replay_events(events).expect("a trailing answer should replay");

        assert!(matches!(
            replay.continuation,
            Some(ResumeContinuation::FinishLoop)
        ));
    }

    #[test]
    fn corrupted_tool_results_fail_closed() {
        let cases: Vec<Vec<ChatMessage>> = vec![
            // Out-of-order results.
            vec![
                assistant_with_calls(&["call-1", "call-2"]),
                tool_result("call-2"),
                tool_result("call-1"),
            ],
            // Duplicated result.
            vec![
                assistant_with_calls(&["call-1", "call-2"]),
                tool_result("call-1"),
                tool_result("call-1"),
            ],
            // Unknown result identity.
            vec![
                assistant_with_calls(&["call-1"]),
                tool_result("call-unknown"),
            ],
            // Sparse group: missing result in the middle of history.
            vec![
                assistant_with_calls(&["call-1", "call-2"]),
                tool_result("call-1"),
                ChatMessage::assistant("later"),
            ],
            // Result without a preceding assistant tool-call group.
            vec![ChatMessage::user("question"), tool_result("call-1")],
        ];
        for messages in cases {
            assert_eq!(
                replay_events(stream(messages))
                    .expect_err("corrupted tool history must fail closed"),
                ResumeError::ToolResultMismatch,
            );
        }
    }
}
