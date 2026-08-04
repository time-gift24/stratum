//! Kernel-side durable transcript compaction.
//!
//! A `prepare_next_turn` hook decision of
//! [`PrepareNextTurnDecision::Compact`](crate::PrepareNextTurnDecision) asks
//! the kernel to durably compact the committed transcript: at the iteration
//! boundary, after the decision's journal record is durable and before the
//! `IterationCompleted` boundary, the kernel commits
//! [`DurableAgentEvent::TranscriptCompacted`](stratum_core::DurableAgentEvent)
//! and replaces the committed context prefix `[0, upto)` with one kernel-owned
//! marker message. The event log keeps every original message; only the
//! rebuilt view shrinks.
//!
//! Coordinate system: `upto` is a zero-based, left-closed/right-open index
//! into the committed context exactly as the `prepare_next_turn` snapshot
//! presents it. Request-only `ContextPatch` views (for example `DropHistory`)
//! use per-request coordinates that must never be mixed with this committed
//! coordinate system; handlers recompute `upto` from the current snapshot on
//! every invocation and never cache or reuse an index across compactions.
//!
//! Cut invariants enforced here (violations fail closed as
//! [`HookFailure::InvalidOutput`]): `upto` must be non-zero, must stay in
//! bounds, must not cut a tool_call/tool_result pair (an assistant message's
//! calls and their results stay on the same side of the cut), and must not
//! cut into the current iteration's committed messages — the iteration start
//! index is tracked by the kernel.
//!
//! Marker message contract: the replacement message is a system-role text
//! message owned by the kernel. Its text is the stable prefix
//! [`COMPACTION_MARKER_PREFIX`], a newline, then the handler summary's text
//! (non-text summary content is serialized to its JSON text). The prefix is
//! part of the crate's public contract: a `transform_context` or
//! `prepare_next_turn` handler recognizes an already-compacted baseline by
//! finding this prefix at the start of the first committed message.

use stratum_core::{ChatContent, ChatMessage, ChatRole, HookFailure};

/// Stable text prefix of every kernel-owned compaction marker message.
///
/// The full marker text is this prefix, a newline, and the summary body.
pub const COMPACTION_MARKER_PREFIX: &str = "[stratum:transcript-compacted]";

/// Builds the kernel-owned marker message replacing a compacted prefix.
pub(crate) fn compaction_marker(summary: &ChatMessage) -> ChatMessage {
    let body = match &summary.content {
        ChatContent::Text(text) => text.clone(),
        // Non-text summary content still lands in the marker as its JSON text.
        other => serde_json::to_string(other).unwrap_or_default(),
    };
    ChatMessage::system(format!("{COMPACTION_MARKER_PREFIX}\n{body}"))
}

/// Validates a compaction cut against the committed messages: `upto` is a
/// zero-based, left-closed/right-open prefix end that must be non-zero, must
/// not reach into the current iteration (`iteration_start` is the committed
/// message count when the current iteration began, which also bounds the
/// index), and must not cut a tool_call/tool_result pair (a compacted
/// assistant message's results must be compacted with it).
pub(crate) fn validate_compaction_cut(
    committed: &[ChatMessage],
    upto: usize,
    iteration_start: usize,
) -> Result<(), HookFailure> {
    debug_assert!(
        iteration_start <= committed.len(),
        "the iteration start indexes the committed context"
    );
    if upto == 0 || upto > iteration_start || upto > committed.len() {
        return Err(HookFailure::InvalidOutput);
    }
    for (index, message) in committed.iter().enumerate().take(upto) {
        if message.role == ChatRole::Assistant && !message.tool_calls.is_empty() {
            let results_end = index + 1 + message.tool_calls.len();
            if results_end > upto {
                return Err(HookFailure::InvalidOutput);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stratum_core::{CallId, ToolCall};

    use super::*;

    fn paired_history() -> Vec<ChatMessage> {
        vec![
            ChatMessage::user("question"),
            ChatMessage::assistant("").with_tool_calls(vec![ToolCall {
                call_id: CallId::from("call-1"),
                name: "echo".to_owned(),
                arguments: json!({}),
            }]),
            ChatMessage::tool(CallId::from("call-1"), json!({"ok": true})),
            ChatMessage::assistant("answer"),
        ]
    }

    #[test]
    fn marker_uses_the_stable_prefix_and_the_summary_text() {
        let marker = compaction_marker(&ChatMessage::system("summary so far"));

        assert_eq!(marker.role, ChatRole::System);
        let ChatContent::Text(text) = &marker.content else {
            panic!("the marker is a text message");
        };
        assert!(text.starts_with(COMPACTION_MARKER_PREFIX));
        assert!(text.ends_with("summary so far"));
        assert!(marker.tool_calls.is_empty());
        assert!(marker.tool_call_id.is_none());
        assert!(marker.reasoning_content.is_none());
    }

    #[test]
    fn cut_validation_accepts_boundaries_that_keep_tool_pairs_together() {
        let history = paired_history();

        // Dropping the question alone, the whole tool pair with it, or the
        // entire completed history up to the iteration start are all legal.
        assert_eq!(validate_compaction_cut(&history, 1, 3), Ok(()));
        assert_eq!(validate_compaction_cut(&history, 3, 3), Ok(()));
        assert_eq!(validate_compaction_cut(&history, 4, 4), Ok(()));
    }

    #[test]
    fn cut_validation_rejects_invalid_cuts() {
        let history = paired_history();

        // A zero cut is a no-op compaction.
        assert_eq!(
            validate_compaction_cut(&history, 0, 3),
            Err(HookFailure::InvalidOutput)
        );
        // The cut drops the assistant message but not its tool result.
        assert_eq!(
            validate_compaction_cut(&history, 2, 3),
            Err(HookFailure::InvalidOutput)
        );
        // The cut reaches into the current iteration's messages.
        assert_eq!(
            validate_compaction_cut(&history, 4, 3),
            Err(HookFailure::InvalidOutput)
        );
        // The cut runs past the committed context.
        assert_eq!(
            validate_compaction_cut(&history, 5, 4),
            Err(HookFailure::InvalidOutput)
        );
    }
}
