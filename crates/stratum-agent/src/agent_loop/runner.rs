//! Concrete agent-loop runner.

use std::{borrow::Cow, collections::HashSet, future::Future, sync::Arc};

use stratum_core::{
    AgentTelemetryEvent, CallId, ChatMessage, ChatRole, DurableAgentEvent, HookFailure, HookPoint,
    LlmCallId, TokenUsage, ToolCall,
};
use stratum_infra::{DurableEventSink, TelemetryEventSink};
use stratum_llm::{ChatRequest, FinishReason, LlmProvider};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::{
    AfterToolCallDecision, AfterToolCallInput, BeforeToolCallDecision, BeforeToolCallInput,
    HookControl, HookRuntime, NoopHookRuntime, PrepareNextTurnDecision, PrepareNextTurnInput,
    ToolApprovalError, ToolExecutor, ToolExecutorError, TransformContextDecision,
    TransformContextInput,
};

use super::{
    AgentLoopBuildError, AgentLoopError, LoopCompletionReason, LoopContext, LoopLimits,
    LoopOutcome, stream::consume_assistant_stream,
};

/// Executes the foundational LLM and tool control flow without owning session state.
pub struct AgentLoop {
    llm_provider: Arc<dyn LlmProvider>,
    tool_executor: ToolExecutor,
    hook_runtime: Arc<dyn HookRuntime>,
    durable_events: Arc<dyn DurableEventSink>,
    telemetry: Arc<dyn TelemetryEventSink>,
    limits: LoopLimits,
}

impl AgentLoop {
    /// Starts construction of an agent loop.
    #[must_use]
    pub fn builder() -> AgentLoopBuilder {
        AgentLoopBuilder::default()
    }

    /// Runs streamed assistant and sequential tool iterations against committed context.
    ///
    /// # Errors
    ///
    /// Returns an error when cancellation, model streaming, protocol validation, or a required
    /// durable acknowledgement prevents the loop from reaching a terminal boundary.
    ///
    /// # Cancellation safety
    ///
    /// Request cancellation through the supplied [`CancellationToken`], then continue polling
    /// this future to completion. Do not race, drop, or abort it: after
    /// [`DurableAgentEvent::ToolExecutionStarted`] is acknowledged, an external side effect may
    /// be in flight and the loop must finish recording the tool outcome. A durable start without
    /// a corresponding result has an unknown outcome and must not be retried automatically unless
    /// the tool has an explicit idempotency guarantee.
    pub async fn run(
        &self,
        context: LoopContext,
        prompts: Vec<ChatMessage>,
        cancellation: CancellationToken,
    ) -> Result<LoopOutcome, AgentLoopError> {
        if prompts.is_empty() {
            return Err(super::ProtocolError::EmptyPrompts.into());
        }
        if let Some(role) = prompts
            .iter()
            .map(|prompt| prompt.role)
            .find(|role| *role != ChatRole::User)
        {
            return Err(super::ProtocolError::InvalidPromptRole { role }.into());
        }
        if self.limits.max_iterations == 0 {
            return Err(AgentLoopError::IterationLimitExceeded { maximum: 0 });
        }

        self.durable_events
            .append(DurableAgentEvent::LoopStarted)
            .await?;
        let mut usage = TokenUsage::default();
        let result = self
            .run_started(context, prompts, &cancellation, &mut usage)
            .await;
        match result {
            Ok(outcome) => Ok(outcome),
            Err(
                error @ (AgentLoopError::Durability { .. }
                | AgentLoopError::TerminalDurability { .. }),
            ) => Err(error),
            Err(error @ AgentLoopError::Cancelled) => Err(self
                .append_terminal(DurableAgentEvent::LoopCancelled { usage }, error)
                .await),
            Err(error) => Err(self
                .append_terminal(
                    DurableAgentEvent::LoopFailed {
                        error_text: error.to_string(),
                        usage,
                    },
                    error,
                )
                .await),
        }
    }

    async fn append_terminal(
        &self,
        event: DurableAgentEvent,
        operation: AgentLoopError,
    ) -> AgentLoopError {
        match self.durable_events.append(event).await {
            Ok(()) => operation,
            Err(source) => AgentLoopError::TerminalDurability {
                operation: Box::new(operation),
                source,
            },
        }
    }

    /// Runs one hook invocation under the shared cancellation and deadline contract.
    ///
    /// Cancellation wins over every decision: a token cancelled before the call
    /// never reaches the runtime, and cancellation while waiting stops the wait.
    /// Hooks that gate a new external action (context transform, before-tool)
    /// treat cancellation as loop cancellation; hooks on the recording path of
    /// an already-started tool cycle (after-tool, prepare-next-turn) degrade to
    /// their no-op decision so the loop can finish recording the outcome and
    /// then reach its regular cancellation boundary. A missed deadline fails
    /// closed with [`HookFailure::TimedOut`]. Runtime failures are already safe
    /// classifications, so the mapped error only keeps the hook point and the
    /// typed failure.
    async fn execute_hook<D, F>(
        &self,
        point: HookPoint,
        cancellation: &CancellationToken,
        invoke: impl FnOnce(HookControl) -> F,
    ) -> Result<HookInvocation<D>, AgentLoopError>
    where
        F: Future<Output = Result<D, HookFailure>>,
    {
        if cancellation.is_cancelled() {
            return Ok(HookInvocation::Cancelled);
        }
        let control = HookControl::new(
            cancellation.clone(),
            Instant::now() + self.limits.hook_timeout,
        );
        let deadline = tokio::time::sleep_until(control.deadline());
        tokio::select! {
            biased;
            () = cancellation.cancelled() => Ok(HookInvocation::Cancelled),
            () = deadline => Err(AgentLoopError::Hook {
                point,
                failure: HookFailure::TimedOut,
            }),
            decision = invoke(control) => decision
                .map(HookInvocation::Decision)
                .map_err(|failure| AgentLoopError::Hook { point, failure }),
        }
    }

    /// Runs one provider-authorized tool call through the before/after hooks.
    async fn execute_authorized_tool_call(
        &self,
        iteration: u64,
        tool_call: &ToolCall,
        cancellation: &CancellationToken,
    ) -> Result<ChatMessage, AgentLoopError> {
        let HookInvocation::Decision(before) = self
            .execute_hook(HookPoint::BeforeToolCall, cancellation, |control| {
                self.hook_runtime.before_tool_call(
                    BeforeToolCallInput {
                        iteration,
                        tool_call,
                    },
                    control,
                )
            })
            .await?
        else {
            return Err(AgentLoopError::Cancelled);
        };
        let before = before.validate().map_err(|failure| AgentLoopError::Hook {
            point: HookPoint::BeforeToolCall,
            failure,
        })?;

        let (executed_call, mut message) = match before {
            BeforeToolCallDecision::Continue => (
                Cow::Borrowed(tool_call),
                self.execute_tool(tool_call, cancellation).await?,
            ),
            BeforeToolCallDecision::ModifyArguments { arguments } => {
                let mut modified = tool_call.clone();
                modified.arguments = arguments;
                let message = self.execute_tool(&modified, cancellation).await?;
                (Cow::Owned(modified), message)
            }
            BeforeToolCallDecision::Block { reason } => (
                Cow::Borrowed(tool_call),
                hook_blocked_result(tool_call, &reason),
            ),
        };

        // Cancellation here means the started tool's outcome must still be
        // recorded: the after hook degrades to Keep and the loop reaches its
        // regular cancellation boundary after the durable commits.
        let after = match self
            .execute_hook(HookPoint::AfterToolCall, cancellation, |control| {
                self.hook_runtime.after_tool_call(
                    AfterToolCallInput {
                        iteration,
                        tool_call: &executed_call,
                        result: &message,
                    },
                    control,
                )
            })
            .await?
        {
            HookInvocation::Decision(decision) => decision,
            HookInvocation::Cancelled => AfterToolCallDecision::Keep,
        };
        if let AfterToolCallDecision::ReplaceResult { result } = after {
            message = ChatMessage::tool(executed_call.call_id.clone(), result);
        }
        Ok(message)
    }

    async fn execute_tool(
        &self,
        tool_call: &ToolCall,
        cancellation: &CancellationToken,
    ) -> Result<ChatMessage, AgentLoopError> {
        match self.tool_executor.execute(tool_call, cancellation).await {
            Ok(message) => Ok(message),
            Err(ToolExecutorError::Durability { source }) => {
                Err(AgentLoopError::Durability { source })
            }
            Err(ToolExecutorError::Approval {
                source: ToolApprovalError::Cancelled,
            }) => Err(AgentLoopError::Cancelled),
            Err(source) => Err(AgentLoopError::ToolExecution { source }),
        }
    }

    async fn run_started(
        &self,
        mut context: LoopContext,
        prompts: Vec<ChatMessage>,
        cancellation: &CancellationToken,
        usage: &mut TokenUsage,
    ) -> Result<LoopOutcome, AgentLoopError> {
        let mut seen_tool_call_ids = committed_tool_call_ids(&context.messages);
        if cancellation.is_cancelled() {
            return Err(AgentLoopError::Cancelled);
        }
        let mut new_messages = Vec::with_capacity(prompts.len() + 1);
        for prompt in prompts {
            self.durable_events
                .append(DurableAgentEvent::MessageAppended {
                    message: prompt.clone(),
                })
                .await?;
            context.messages.push(prompt.clone());
            new_messages.push(prompt);
        }

        let mut pending_inject: Option<Vec<ChatMessage>> = None;
        for iteration in 0..self.limits.max_iterations {
            if cancellation.is_cancelled() {
                return Err(AgentLoopError::Cancelled);
            }
            let iteration_index = u64::try_from(iteration).unwrap_or(u64::MAX);
            let request_view = match pending_inject.take() {
                Some(injected) => {
                    let mut view = context.clone();
                    view.messages.extend(injected);
                    Cow::Owned(view)
                }
                None => Cow::Borrowed(&context),
            };
            let HookInvocation::Decision(transform) = self
                .execute_hook(HookPoint::TransformContext, cancellation, |control| {
                    self.hook_runtime.transform_context(
                        TransformContextInput {
                            iteration: iteration_index,
                            context: &request_view,
                        },
                        control,
                    )
                })
                .await?
            else {
                return Err(AgentLoopError::Cancelled);
            };
            let request_view = match transform {
                TransformContextDecision::Unchanged => request_view,
                TransformContextDecision::Replace {
                    context: replacement,
                } => Cow::Owned(replacement),
            };

            let llm_call_id = LlmCallId::from(uuid::Uuid::now_v7().to_string());
            self.telemetry.emit(AgentTelemetryEvent::LlmStarted {
                llm_call_id: llm_call_id.clone(),
            });
            let request = ChatRequest {
                model: self.llm_provider.model_id(),
                messages: request_messages(&request_view.system_prompt, &request_view.messages),
                tools: self.tool_executor.specs(),
                structured_output: None,
            };
            let stream = tokio::select! {
                biased;
                () = cancellation.cancelled() => return Err(AgentLoopError::Cancelled),
                stream = self.llm_provider.chat_stream(request) => stream?,
            };
            let assistant = consume_assistant_stream(
                stream,
                &llm_call_id,
                self.telemetry.as_ref(),
                cancellation,
                self.limits,
                usage,
            )
            .await?;
            let finish_reason = assistant.finish_reason;
            let tool_calls = assistant.message.tool_calls.clone();
            let new_tool_call_ids = validate_new_tool_call_ids(&tool_calls, &seen_tool_call_ids)?;

            self.durable_events
                .append(DurableAgentEvent::MessageAppended {
                    message: assistant.message.clone(),
                })
                .await?;
            seen_tool_call_ids.extend(new_tool_call_ids);
            context.messages.push(assistant.message.clone());
            new_messages.push(assistant.message);

            if !tool_calls.is_empty() {
                context.messages.reserve(tool_calls.len());
                new_messages.reserve(tool_calls.len());
                for tool_call in &tool_calls {
                    if cancellation.is_cancelled() {
                        return Err(AgentLoopError::Cancelled);
                    }
                    let message = if finish_reason != FinishReason::ToolCalls {
                        unexecutable_tool_result(tool_call, finish_reason)
                    } else {
                        self.execute_authorized_tool_call(iteration_index, tool_call, cancellation)
                            .await?
                    };
                    self.durable_events
                        .append(DurableAgentEvent::MessageAppended {
                            message: message.clone(),
                        })
                        .await?;
                    context.messages.push(message.clone());
                    new_messages.push(message);
                }

                // Cancellation degrades the decision to Continue so the
                // iteration boundary is committed before the loop reaches its
                // regular cancellation check.
                let prepare = match self
                    .execute_hook(HookPoint::PrepareNextTurn, cancellation, |control| {
                        self.hook_runtime.prepare_next_turn(
                            PrepareNextTurnInput {
                                iteration: iteration_index,
                                context: &context,
                            },
                            control,
                        )
                    })
                    .await?
                {
                    HookInvocation::Decision(decision) => {
                        decision
                            .validate()
                            .map_err(|failure| AgentLoopError::Hook {
                                point: HookPoint::PrepareNextTurn,
                                failure,
                            })?
                    }
                    HookInvocation::Cancelled => PrepareNextTurnDecision::Continue,
                };

                self.durable_events
                    .append(DurableAgentEvent::IterationCompleted {
                        iteration: iteration_index,
                        usage: *usage,
                    })
                    .await?;

                match prepare {
                    PrepareNextTurnDecision::Continue => {}
                    PrepareNextTurnDecision::Inject { messages } => {
                        pending_inject = Some(messages);
                    }
                    PrepareNextTurnDecision::Stop => {
                        self.durable_events
                            .append(DurableAgentEvent::LoopFinished {
                                finish_reason: LoopCompletionReason::HookStopped
                                    .as_str()
                                    .to_owned(),
                                usage: *usage,
                            })
                            .await?;
                        return Ok(LoopOutcome {
                            new_messages,
                            completion: LoopCompletionReason::HookStopped,
                            usage: *usage,
                        });
                    }
                }
                continue;
            }

            self.durable_events
                .append(DurableAgentEvent::IterationCompleted {
                    iteration: iteration_index,
                    usage: *usage,
                })
                .await?;

            self.durable_events
                .append(DurableAgentEvent::LoopFinished {
                    finish_reason: finish_reason.as_str().to_owned(),
                    usage: *usage,
                })
                .await?;
            return Ok(LoopOutcome {
                new_messages,
                completion: LoopCompletionReason::Model(finish_reason),
                usage: *usage,
            });
        }

        if cancellation.is_cancelled() {
            return Err(AgentLoopError::Cancelled);
        }
        Err(AgentLoopError::IterationLimitExceeded {
            maximum: self.limits.max_iterations,
        })
    }
}

/// Builder for [`AgentLoop`].
#[derive(Default)]
pub struct AgentLoopBuilder {
    llm_provider: Option<Arc<dyn LlmProvider>>,
    tool_executor: Option<ToolExecutor>,
    hook_runtime: Option<Arc<dyn HookRuntime>>,
    telemetry: Option<Arc<dyn TelemetryEventSink>>,
    limits: LoopLimits,
}

impl AgentLoopBuilder {
    /// Sets the bound model provider.
    #[must_use]
    pub fn llm_provider(mut self, llm_provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(llm_provider);
        self
    }

    /// Sets the approval-aware tool executor.
    #[must_use]
    pub fn tool_executor(mut self, tool_executor: ToolExecutor) -> Self {
        self.tool_executor = Some(tool_executor);
        self
    }

    /// Sets the composed hook runtime; defaults to [`NoopHookRuntime`].
    #[must_use]
    pub fn hook_runtime(mut self, hook_runtime: Arc<dyn HookRuntime>) -> Self {
        self.hook_runtime = Some(hook_runtime);
        self
    }

    /// Sets the best-effort telemetry sink.
    #[must_use]
    pub fn telemetry(mut self, telemetry: Arc<dyn TelemetryEventSink>) -> Self {
        self.telemetry = Some(telemetry);
        self
    }

    /// Sets safety limits for one run.
    #[must_use]
    pub const fn limits(mut self, limits: LoopLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Builds the agent loop.
    ///
    /// # Errors
    ///
    /// Returns the corresponding [`AgentLoopBuildError`] variant for the first required field not
    /// supplied.
    pub fn build(self) -> Result<AgentLoop, AgentLoopBuildError> {
        let llm_provider = self
            .llm_provider
            .ok_or(AgentLoopBuildError::MissingLlmProvider)?;
        let tool_executor = self
            .tool_executor
            .ok_or(AgentLoopBuildError::MissingToolExecutor)?;
        let durable_events = tool_executor.durable_events();
        Ok(AgentLoop {
            llm_provider,
            tool_executor,
            hook_runtime: self
                .hook_runtime
                .unwrap_or_else(|| Arc::new(NoopHookRuntime)),
            durable_events,
            telemetry: self
                .telemetry
                .ok_or(AgentLoopBuildError::MissingTelemetry)?,
            limits: self.limits,
        })
    }
}

fn request_messages(system_prompt: &str, history: &[ChatMessage]) -> Vec<ChatMessage> {
    let mut messages = Vec::with_capacity(history.len() + 1);
    messages.push(ChatMessage::system(system_prompt));
    messages.extend_from_slice(history);
    messages
}

fn unexecutable_tool_result(tool_call: &ToolCall, finish_reason: FinishReason) -> ChatMessage {
    let (code, message) = if finish_reason == FinishReason::Length {
        (
            "tool_call_truncated",
            "tool call was not executed because the model response reached its length limit",
        )
    } else {
        (
            "tool_call_not_authorized",
            "tool call was not executed because the model did not finish with tool_calls",
        )
    };
    ChatMessage::tool(
        tool_call.call_id.clone(),
        serde_json::json!({
            "error": {
                "code": code,
                "message": message,
            }
        }),
    )
}

/// Outcome of one hook invocation under the loop's cancellation contract.
enum HookInvocation<D> {
    /// The runtime returned a decision in time.
    Decision(D),
    /// Cancellation won before or during the invocation; the hook has no
    /// decision and the loop follows its regular cancellation path.
    Cancelled,
}

fn hook_blocked_result(tool_call: &ToolCall, reason: &str) -> ChatMessage {
    ChatMessage::tool(
        tool_call.call_id.clone(),
        serde_json::json!({
            "error": {
                "code": "hook_blocked",
                "message": reason,
            }
        }),
    )
}

fn committed_tool_call_ids(messages: &[ChatMessage]) -> HashSet<CallId> {
    messages
        .iter()
        .filter(|message| message.role == ChatRole::Assistant)
        .flat_map(|message| message.tool_calls.iter())
        .map(|tool_call| tool_call.call_id.clone())
        .collect()
}

fn validate_new_tool_call_ids(
    tool_calls: &[ToolCall],
    seen: &HashSet<CallId>,
) -> Result<HashSet<CallId>, super::ProtocolError> {
    let mut new_call_ids = HashSet::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        if seen.contains(&tool_call.call_id) || !new_call_ids.insert(tool_call.call_id.clone()) {
            return Err(super::ProtocolError::DuplicateToolCallId {
                call_id: tool_call.call_id.clone(),
            });
        }
    }
    Ok(new_call_ids)
}
