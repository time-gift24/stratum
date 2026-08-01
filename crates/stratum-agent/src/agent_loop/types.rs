//! Context, limits, and successful outcome types for the agent loop kernel.

use std::time::Duration;

use stratum_core::{ChatMessage, HookPoint, TokenUsage};
use stratum_llm::FinishReason;

/// Committed conversation state supplied to an agent loop run.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LoopContext {
    /// Instruction prepended to the model conversation.
    pub system_prompt: String,
    /// Complete committed transcript in provider order.
    pub messages: Vec<ChatMessage>,
}

impl LoopContext {
    /// Creates an empty loop context with the provided system instruction.
    #[must_use]
    pub fn new(system_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            messages: Vec::new(),
        }
    }

    /// Moves a committed transcript into this context.
    #[must_use]
    pub fn with_messages(mut self, messages: Vec<ChatMessage>) -> Self {
        self.messages = messages;
        self
    }
}

/// Per-hook-point deadlines enforced by the loop around every hook invocation.
///
/// A `None` deadline means the point is bounded by the turn cancellation token
/// only. `decide_tool_call` defaults to `None` so interactive approval
/// handlers can wait for a human; every other point defaults to a timeout and
/// stays fail-closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct HookTimeouts {
    /// Deadline for `transform_context`.
    pub transform_context: Option<Duration>,
    /// Deadline for `transform_tool_call`.
    pub transform_tool_call: Option<Duration>,
    /// Deadline for `decide_tool_call`; `None` (cancellation only) by default.
    pub decide_tool_call: Option<Duration>,
    /// Deadline for `after_tool_call`.
    pub after_tool_call: Option<Duration>,
    /// Deadline for `prepare_next_turn`.
    pub prepare_next_turn: Option<Duration>,
}

impl HookTimeouts {
    const DEFAULT_TIMEOUT: Option<Duration> = Some(Duration::from_secs(30));

    /// Creates per-point deadlines with the default configuration.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            transform_context: Self::DEFAULT_TIMEOUT,
            transform_tool_call: Self::DEFAULT_TIMEOUT,
            decide_tool_call: None,
            after_tool_call: Self::DEFAULT_TIMEOUT,
            prepare_next_turn: Self::DEFAULT_TIMEOUT,
        }
    }

    /// Overrides the `transform_context` deadline.
    #[must_use]
    pub const fn with_transform_context(mut self, timeout: Option<Duration>) -> Self {
        self.transform_context = timeout;
        self
    }

    /// Overrides the `transform_tool_call` deadline.
    #[must_use]
    pub const fn with_transform_tool_call(mut self, timeout: Option<Duration>) -> Self {
        self.transform_tool_call = timeout;
        self
    }

    /// Overrides the `decide_tool_call` deadline.
    #[must_use]
    pub const fn with_decide_tool_call(mut self, timeout: Option<Duration>) -> Self {
        self.decide_tool_call = timeout;
        self
    }

    /// Overrides the `after_tool_call` deadline.
    #[must_use]
    pub const fn with_after_tool_call(mut self, timeout: Option<Duration>) -> Self {
        self.after_tool_call = timeout;
        self
    }

    /// Overrides the `prepare_next_turn` deadline.
    #[must_use]
    pub const fn with_prepare_next_turn(mut self, timeout: Option<Duration>) -> Self {
        self.prepare_next_turn = timeout;
        self
    }

    /// Returns the configured deadline for one hook point.
    pub(crate) fn for_point(&self, point: HookPoint) -> Option<Duration> {
        match point {
            HookPoint::TransformContext => self.transform_context,
            HookPoint::TransformToolCall => self.transform_tool_call,
            HookPoint::DecideToolCall => self.decide_tool_call,
            HookPoint::AfterToolCall => self.after_tool_call,
            HookPoint::PrepareNextTurn => self.prepare_next_turn,
            _ => Self::DEFAULT_TIMEOUT,
        }
    }
}

impl Default for HookTimeouts {
    fn default() -> Self {
        Self::new()
    }
}

/// Safety bounds applied before the loop starts additional work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoopLimits {
    /// Maximum number of model iterations in one run.
    pub max_iterations: usize,
    /// Maximum tool calls accepted from one model iteration.
    pub max_tool_calls_per_iteration: usize,
    /// Maximum streamed assistant text bytes in one model response.
    pub max_text_bytes: usize,
    /// Maximum streamed reasoning bytes in one model response.
    pub max_reasoning_bytes: usize,
    /// Maximum streamed argument bytes for one tool call.
    pub max_tool_argument_bytes: usize,
    /// Per-hook-point invocation deadlines.
    pub hook_timeouts: HookTimeouts,
}

impl LoopLimits {
    const DEFAULT_MAX_TEXT_BYTES: usize = 1024 * 1024;
    const DEFAULT_MAX_REASONING_BYTES: usize = 1024 * 1024;
    const DEFAULT_MAX_TOOL_ARGUMENT_BYTES: usize = 256 * 1024;

    /// Creates loop safety bounds.
    #[must_use]
    pub const fn new(max_iterations: usize, max_tool_calls_per_iteration: usize) -> Self {
        Self {
            max_iterations,
            max_tool_calls_per_iteration,
            max_text_bytes: Self::DEFAULT_MAX_TEXT_BYTES,
            max_reasoning_bytes: Self::DEFAULT_MAX_REASONING_BYTES,
            max_tool_argument_bytes: Self::DEFAULT_MAX_TOOL_ARGUMENT_BYTES,
            hook_timeouts: HookTimeouts::new(),
        }
    }

    /// Overrides the streamed response byte limits.
    #[must_use]
    pub const fn with_stream_byte_limits(
        mut self,
        max_text_bytes: usize,
        max_reasoning_bytes: usize,
        max_tool_argument_bytes: usize,
    ) -> Self {
        self.max_text_bytes = max_text_bytes;
        self.max_reasoning_bytes = max_reasoning_bytes;
        self.max_tool_argument_bytes = max_tool_argument_bytes;
        self
    }

    /// Overrides the per-hook-point invocation deadlines.
    #[must_use]
    pub const fn with_hook_timeouts(mut self, hook_timeouts: HookTimeouts) -> Self {
        self.hook_timeouts = hook_timeouts;
        self
    }
}

impl Default for LoopLimits {
    fn default() -> Self {
        Self::new(16, 16)
    }
}

/// Reason an agent loop run reached its terminal boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoopCompletionReason {
    /// The provider finished without executable tool calls.
    Model(FinishReason),
    /// A `prepare_next_turn` hook stopped the loop.
    HookStopped,
}

impl LoopCompletionReason {
    /// Returns the stable durable projection of this completion reason.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model(finish_reason) => finish_reason.as_str(),
            Self::HookStopped => "hook_stopped",
        }
    }
}

/// Successful terminal result returned by the agent loop kernel.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LoopOutcome {
    /// Messages committed during this loop run.
    pub new_messages: Vec<ChatMessage>,
    /// Why the loop reached its terminal boundary.
    pub completion: LoopCompletionReason,
    /// Token usage reported by the most recent model response in this run,
    /// zero-filled when no response reported usage.
    pub usage: TokenUsage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_preserve_context_and_default_limits() {
        let transcript = vec![ChatMessage::user("hello"), ChatMessage::assistant("hi")];
        let context = LoopContext::new("be helpful").with_messages(transcript);

        assert_eq!(context.system_prompt, "be helpful");
        assert_eq!(
            context.messages,
            vec![ChatMessage::user("hello"), ChatMessage::assistant("hi"),]
        );
        assert_eq!(LoopLimits::default(), LoopLimits::new(16, 16));
    }

    #[test]
    fn completion_reason_projects_stable_strings() {
        assert_eq!(
            LoopCompletionReason::Model(FinishReason::Stop).as_str(),
            "stop"
        );
        assert_eq!(LoopCompletionReason::HookStopped.as_str(), "hook_stopped");
    }

    #[test]
    fn hook_timeouts_default_per_point_and_accept_overrides() {
        let defaults = HookTimeouts::default();
        assert_eq!(defaults.transform_context, Some(Duration::from_secs(30)),);
        assert_eq!(defaults.decide_tool_call, None);
        assert_eq!(defaults.after_tool_call, Some(Duration::from_secs(30)));
        assert_eq!(LoopLimits::default().hook_timeouts, defaults);
        assert_eq!(defaults.for_point(HookPoint::DecideToolCall), None,);
        assert_eq!(
            defaults.for_point(HookPoint::TransformToolCall),
            Some(Duration::from_secs(30)),
        );

        let overridden = defaults
            .with_decide_tool_call(Some(Duration::from_millis(50)))
            .with_after_tool_call(None);
        assert_eq!(overridden.decide_tool_call, Some(Duration::from_millis(50)));
        assert_eq!(overridden.after_tool_call, None);
        assert_eq!(
            LoopLimits::default()
                .with_hook_timeouts(overridden)
                .hook_timeouts,
            overridden
        );
    }
}
