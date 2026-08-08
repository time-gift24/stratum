//! Concrete agent-loop runner.

use std::{borrow::Cow, collections::HashSet, future::Future, sync::Arc};

use stratum_core::{
    AgentTelemetryEvent, CallId, ChatMessage, ChatRole, ContextPatch, DurableAgentEvent,
    HookFailure, HookInvocationId, HookPoint, LlmCallId, TokenUsage, ToolCall, ToolName,
};
use stratum_infra::{DurableEventSink, TelemetryEventSink};
use stratum_llm::{ChatRequest, FinishReason, LlmProvider};
use stratum_tools::Tool;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::{
    AfterToolCallDecision, AfterToolCallInput, AuthorizationOverride, DecideToolCallDecision,
    DecideToolCallInput, HookControl, HookRuntime, HookSnapshot, NoopHookRuntime,
    PrepareNextTurnDecision, PrepareNextTurnInput, ToolExecutor, ToolExecutorError, ToolHookTarget,
    TransformContextDecision, TransformContextInput, TransformToolCallDecision,
    TransformToolCallInput,
};

use super::{
    AgentLoopBuildError, AgentLoopError, LoopCompletionReason, LoopContext, LoopLimits,
    LoopOutcome, ResumeError,
    journal::{HookAddress, HookInvocationSite, HookJournal, JournalDecision, JournalState},
    resume::{ResumeContinuation, replay_events},
    stream::consume_assistant_stream,
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

/// Starting state of one loop run: fresh or replayed from a durable stream.
struct RunStart {
    context: LoopContext,
    prompts: Vec<ChatMessage>,
    first_iteration: usize,
    continuation: Option<ResumeContinuation>,
    journal: HookJournal,
    /// Committed message index where the first (frontier) iteration's messages
    /// begin; compaction cuts must not reach past it.
    iteration_start: usize,
    /// Iterations whose compaction the replay already applied.
    compacted_iterations: HashSet<u64>,
}

/// Loop-carried state of one run: messages committed during the run and the
/// one-shot injection waiting for the next request view.
struct IterationState {
    new_messages: Vec<ChatMessage>,
    pending_inject: Option<Vec<ChatMessage>>,
    /// Committed message index where the current iteration's messages begin;
    /// compaction cuts must not reach past it.
    iteration_start: usize,
    /// Iterations whose compaction the replay already applied; their prepare
    /// boundary reuses the journaled decision without re-executing.
    compacted_iterations: HashSet<u64>,
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
    // Tracing skips every argument: prompts and context carry user content,
    // and no agent_id/turn_id exists at the kernel layer.
    #[tracing::instrument(skip_all)]
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
            .append(DurableAgentEvent::LoopStarted {
                extension_set_version_id: self.hook_runtime.extension_set_version(),
            })
            .await?;
        // `None` until a provider response reports usage; durable events and
        // the loop outcome project it back to a zero-filled `TokenUsage`.
        let mut usage: Option<TokenUsage> = None;
        let start = RunStart {
            iteration_start: context.messages.len() + prompts.len(),
            context,
            prompts,
            first_iteration: 0,
            continuation: None,
            journal: HookJournal::default(),
            compacted_iterations: HashSet::new(),
        };
        let result = self.run_started(start, &cancellation, &mut usage).await;
        self.finish_run(result, usage).await
    }

    /// Resumes one run from its durable event stream.
    ///
    /// The composing side reads the run's events and re-supplies the system
    /// prompt and run configuration (provider, tool executor, hook runtime,
    /// limits stay on this loop); the kernel never reads storage itself. The
    /// stream must start with [`DurableAgentEvent::LoopStarted`] and must not
    /// contain a terminal event. Replay rebuilds the committed context —
    /// applying every [`DurableAgentEvent::TranscriptCompacted`] in event
    /// order, so the baseline resumes already compacted — fixes the iteration
    /// frontier at one past the maximum committed
    /// [`DurableAgentEvent::IterationCompleted`], reconciles committed tool
    /// results, and consults the journaled hook invocations before calling
    /// the hook runtime again: digest-matching completed decisions are reused,
    /// pending invocations are retried under their original identity, and
    /// failures are reproduced. When the stream's `LoopStarted` recorded an
    /// extension set version and the injected runtime reports a different one,
    /// the resume fails closed before any model, tool, or hook action; a
    /// runtime reporting no version skips that check. The resumed run keeps
    /// appending to the same durable sink and does not record a second
    /// `LoopStarted`. The pure validation half of this method is available
    /// separately as [`AgentLoop::prepare_resume`].
    ///
    /// Tool execution stays at-least-once: a call started before the crash
    /// without a committed result re-executes as part of the missing result
    /// suffix.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ResumeError`] through [`AgentLoopError::Resume`] when
    /// the stream cannot be rebuilt into a consistent run state, and the same
    /// errors as [`AgentLoop::run`] once the continuation starts.
    ///
    /// # Cancellation safety
    ///
    /// Same contract as [`AgentLoop::run`].
    // Tracing skips every argument: the replayed event stream carries user
    // content, and no agent_id/turn_id exists at the kernel layer.
    #[tracing::instrument(skip_all)]
    pub async fn resume(
        &self,
        system_prompt: impl Into<String>,
        events: Vec<DurableAgentEvent>,
        cancellation: CancellationToken,
    ) -> Result<LoopOutcome, AgentLoopError> {
        if self.limits.max_iterations == 0 {
            return Err(AgentLoopError::IterationLimitExceeded { maximum: 0 });
        }
        let start = self.resume_start(system_prompt, events)?;
        // Usage is a volatile observation, not resume state: the first hook
        // boundary after a resume observes `None` until the next model
        // response reports usage.
        let mut usage: Option<TokenUsage> = None;
        let result = self.run_started(start, &cancellation, &mut usage).await;
        self.finish_run(result, usage).await
    }

    /// Validates one durable event stream into a prepared resume bound to
    /// this exact loop.
    ///
    /// This is the pure half of [`AgentLoop::resume`]: it performs exactly the
    /// replay validation, extension-set-version guard, and run-state
    /// construction that `resume` performs before any model, tool, or hook
    /// action — see its documentation for the full replay contract. It does
    /// no I/O: no durable append, no model, tool, or hook call. The returned
    /// [`PreparedResume`] is bound to this exact loop instance and can be run
    /// exactly once through its consuming [`PreparedResume::run`], so the
    /// composing side can validate the replay window before accepting external
    /// work and then run the continuation on the same runtime.
    ///
    /// # Errors
    ///
    /// Returns a typed [`ResumeError`] when the stream cannot be rebuilt into
    /// a consistent run state, or when the injected hook runtime reports a
    /// different extension set version than the stream recorded; every failure
    /// refuses the resume closed before any side effect.
    pub fn prepare_resume(
        self: &Arc<Self>,
        system_prompt: impl Into<String>,
        events: Vec<DurableAgentEvent>,
    ) -> Result<PreparedResume, ResumeError> {
        Ok(PreparedResume {
            agent_loop: Arc::clone(self),
            start: self.resume_start(system_prompt, events)?,
        })
    }

    /// Replay validation, extension-set-version guard, and run-state
    /// construction shared by [`AgentLoop::resume`] and
    /// [`AgentLoop::prepare_resume`]; performs no I/O and no model, tool, or
    /// hook call.
    fn resume_start(
        &self,
        system_prompt: impl Into<String>,
        events: Vec<DurableAgentEvent>,
    ) -> Result<RunStart, ResumeError> {
        let replay = replay_events(events)?;
        // A run that pinned its handler chain refuses to resume under a chain
        // reporting a different version (changed membership, order, or handler
        // version). A runtime without a pinned version skips the check.
        if let (Some(recorded), Some(current)) = (
            replay.extension_set_version_id,
            self.hook_runtime.extension_set_version(),
        ) && recorded != current
        {
            tracing::warn!(
                recorded = %recorded,
                current = %current,
                "refusing resume: hook extension set version mismatch"
            );
            return Err(ResumeError::ExtensionSetVersionMismatch { recorded, current });
        }
        Ok(RunStart {
            context: LoopContext::new(system_prompt).with_messages(replay.messages),
            prompts: Vec::new(),
            first_iteration: usize::try_from(replay.frontier).unwrap_or(usize::MAX),
            continuation: replay.continuation,
            journal: replay.journal,
            iteration_start: replay.iteration_start,
            compacted_iterations: replay.compacted_iterations,
        })
    }

    async fn finish_run(
        &self,
        result: Result<LoopOutcome, AgentLoopError>,
        usage: Option<TokenUsage>,
    ) -> Result<LoopOutcome, AgentLoopError> {
        match result {
            Ok(outcome) => Ok(outcome),
            Err(
                error @ (AgentLoopError::Durability { .. }
                | AgentLoopError::TerminalDurability { .. }),
            ) => Err(error),
            Err(error @ AgentLoopError::Cancelled) => Err(self
                .append_terminal(
                    DurableAgentEvent::LoopCancelled {
                        usage: usage.unwrap_or_default(),
                    },
                    error,
                )
                .await),
            Err(error) => Err(self
                .append_terminal(
                    DurableAgentEvent::LoopFailed {
                        error_text: error.to_string(),
                        usage: usage.unwrap_or_default(),
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

    /// Runs one hook invocation under the shared cancellation, deadline, and
    /// journal contract.
    ///
    /// Cancellation wins over every decision: a token cancelled before the
    /// call never reaches the runtime and is never journaled, and cancellation
    /// while waiting stops the wait. Hooks that gate a new external action
    /// (context transform, tool-call transform, tool-call decide) treat
    /// cancellation as loop cancellation; hooks on the recording path of an
    /// already-started tool cycle (after-tool, prepare-next-turn) degrade to
    /// their no-op decision so the loop can finish recording the outcome and
    /// then reach its regular cancellation boundary.
    ///
    /// Journaling orders every invocation as: consult the replayed journal
    /// first (a digest-matching completed decision is reused without calling
    /// the runtime, a pending entry is retried under its original invocation
    /// identity, a failed entry reproduces its typed failure, and a digest
    /// mismatch fails closed), then commit `HookInvocationPending` before
    /// calling the runtime, commit `HookInvocationCompleted` after the
    /// decision validates and before its affected action, or commit
    /// `HookInvocationFailed` for a typed failure, a missed deadline, or an
    /// invalid decision. Journal appends are durable boundaries: their
    /// failure fails closed like every other durable append.
    ///
    /// The deadline is configured per hook point through
    /// [`LoopLimits::hook_timeouts`]; a point without a deadline (the default
    /// for decide) is bounded by cancellation only. A missed deadline fails
    /// closed with [`HookFailure::TimedOut`]. Runtime failures are already safe
    /// classifications, so the mapped error only keeps the hook point and the
    /// typed failure.
    // Tracing fields stay at safe metadata: hook point, iteration, and call
    // identity. Inputs, digests, decisions, and closures are never recorded.
    #[tracing::instrument(
        level = "debug",
        skip_all,
        fields(
            hook_point = ?site.point,
            iteration = site.iteration,
            call_id = site.call_id.as_ref().map(tracing::field::display),
        )
    )]
    async fn execute_hook<D, F>(
        &self,
        site: HookInvocationSite<'_>,
        cancellation: &CancellationToken,
        invoke: impl FnOnce(HookControl) -> F,
        validate: impl FnOnce(&D) -> Result<(), HookFailure>,
    ) -> Result<HookInvocation<D>, AgentLoopError>
    where
        F: Future<Output = Result<D, HookFailure>>,
        D: JournalDecision,
    {
        if cancellation.is_cancelled() {
            return Ok(HookInvocation::Cancelled);
        }
        let HookInvocationSite {
            journal,
            point,
            iteration,
            call_id,
            input_digest,
        } = site;
        let address = HookAddress::new(iteration, point, call_id.clone());
        let mut pending_required = true;
        let invocation_id = match journal.lookup(&address) {
            Some(entry) => {
                if entry.input_digest != input_digest {
                    tracing::warn!(
                        invocation_id = %entry.invocation_id,
                        "refusing resume: journaled hook input digest does not match the rebuilt input"
                    );
                    return Err(ResumeError::HookDigestMismatch { point }.into());
                }
                match &entry.state {
                    JournalState::Completed(record) => {
                        let decision =
                            D::from_record(record).ok_or(ResumeError::HookRecordMismatch)?;
                        // A journaled decision already validated once; a
                        // mismatch against the rebuilt state is corruption.
                        validate(&decision).map_err(|_| ResumeError::HookRecordMismatch)?;
                        tracing::debug!(
                            invocation_id = %entry.invocation_id,
                            "reusing journaled hook decision without calling the runtime"
                        );
                        return Ok(HookInvocation::Decision(decision));
                    }
                    JournalState::Failed(failure) => {
                        return Err(AgentLoopError::Hook {
                            point,
                            failure: *failure,
                        });
                    }
                    // A pending entry is retried under its original identity;
                    // the pending record is already durable.
                    JournalState::Pending => {
                        tracing::debug!(
                            invocation_id = %entry.invocation_id,
                            "retrying journaled pending hook invocation under its original identity"
                        );
                        pending_required = false;
                        entry.invocation_id
                    }
                }
            }
            None => HookInvocationId::new(),
        };
        if pending_required {
            self.durable_events
                .append(DurableAgentEvent::HookInvocationPending {
                    invocation_id,
                    point,
                    iteration,
                    call_id,
                    input_digest,
                })
                .await?;
        }

        let deadline = self
            .limits
            .hook_timeouts
            .for_point(point)
            .map(|timeout| Instant::now() + timeout);
        let control = HookControl::new(cancellation.clone(), deadline);
        let invocation = invoke(control);
        let outcome = match deadline {
            Some(deadline) => {
                tokio::select! {
                    biased;
                    () = cancellation.cancelled() => HookOutcome::Cancelled,
                    () = tokio::time::sleep_until(deadline) => HookOutcome::Failed(HookFailure::TimedOut),
                    decision = invocation => HookOutcome::from(decision),
                }
            }
            None => {
                tokio::select! {
                    biased;
                    () = cancellation.cancelled() => HookOutcome::Cancelled,
                    decision = invocation => HookOutcome::from(decision),
                }
            }
        };
        match outcome {
            HookOutcome::Cancelled => Ok(HookInvocation::Cancelled),
            HookOutcome::Failed(failure) => {
                self.durable_events
                    .append(DurableAgentEvent::HookInvocationFailed {
                        invocation_id,
                        failure,
                    })
                    .await?;
                Err(AgentLoopError::Hook { point, failure })
            }
            HookOutcome::Decision(decision) => {
                if let Err(failure) = validate(&decision) {
                    self.durable_events
                        .append(DurableAgentEvent::HookInvocationFailed {
                            invocation_id,
                            failure,
                        })
                        .await?;
                    return Err(AgentLoopError::Hook { point, failure });
                }
                self.durable_events
                    .append(DurableAgentEvent::HookInvocationCompleted {
                        invocation_id,
                        decision: decision.to_record(),
                    })
                    .await?;
                Ok(HookInvocation::Decision(decision))
            }
        }
    }

    /// Runs one provider-authorized tool call through lookup, validation, the
    /// transform/decide/after hooks, and the executor.
    ///
    /// The fixed phase order makes the decide phase (for example an approval
    /// handler) see exactly the arguments that execute: lookup and
    /// authorization metadata first, original-argument validation, the
    /// transform hook, final-argument re-validation, the decide hook, then the
    /// durable start and the call. A missing tool or a failed validation
    /// produces the executor's structured error result without entering any
    /// tool hook. The transform hook may override the effective authorization;
    /// the kernel carries the effective value to the decide and after phases
    /// without interpreting it. Each hook observes a payload digest of the
    /// exact call it sees: the original call at transform, the final
    /// re-validated call at decide and after.
    async fn execute_authorized_tool_call(
        &self,
        iteration: u64,
        tool_call: &ToolCall,
        context: &LoopContext,
        usage: Option<TokenUsage>,
        journal: &HookJournal,
        cancellation: &CancellationToken,
    ) -> Result<ChatMessage, AgentLoopError> {
        let tool_name = ToolName::new(tool_call.name.clone());
        let (authorization, tool) = match self.tool_executor.hook_lookup(&tool_name) {
            Ok(target) => target,
            Err(error) => return Ok(crate::tool_executor::tool_error_result(tool_call, &error)),
        };
        let target = ToolHookTarget {
            authorization,
            spec: tool.spec(),
        };
        // One snapshot serves all three tool hooks of this call: the borrowed
        // context only gains the current result after this function returns,
        // so `after_tool_call` still excludes the uncommitted result.
        let snapshot = HookSnapshot {
            iteration,
            context,
            usage,
        };

        if let Err(error) = self.tool_executor.validate_call(&tool_name, tool_call) {
            return Ok(crate::tool_executor::tool_error_result(tool_call, &error));
        }

        let HookInvocation::Decision(transform) = self
            .execute_hook(
                HookInvocationSite {
                    journal,
                    point: HookPoint::TransformToolCall,
                    iteration,
                    call_id: Some(tool_call.call_id.clone()),
                    input_digest: super::journal::tool_call_digest(tool_call),
                },
                cancellation,
                |control| {
                    self.hook_runtime.transform_tool_call(
                        TransformToolCallInput {
                            snapshot,
                            tool_call,
                            tool: &target,
                        },
                        control,
                    )
                },
                TransformToolCallDecision::check,
            )
            .await?
        else {
            return Err(AgentLoopError::Cancelled);
        };
        // The kernel only transports the effective authorization: no override
        // keeps the registry default, overrides apply verbatim, and the kernel
        // never branches on or interprets the value.
        let (final_call, effective_authorization) = match transform {
            TransformToolCallDecision::Continue => (Cow::Borrowed(tool_call), authorization),
            TransformToolCallDecision::Modify(modification) => {
                let effective_authorization = match modification.authorization {
                    None => authorization,
                    Some(AuthorizationOverride::PreAuthorize) => None,
                    Some(AuthorizationOverride::Set { kind, danger }) => Some((kind, danger)),
                };
                let final_call = match modification.arguments {
                    Some(arguments) => {
                        let mut modified = tool_call.clone();
                        modified.arguments = arguments;
                        Cow::Owned(modified)
                    }
                    None => Cow::Borrowed(tool_call),
                };
                (final_call, effective_authorization)
            }
        };
        let effective_target = ToolHookTarget {
            authorization: effective_authorization,
            spec: target.spec,
        };

        // The decide phase only sees arguments that passed the final
        // re-validation, so an approval decision always covers the exact
        // payload that would execute.
        if let Err(error) = self.tool_executor.validate_call(&tool_name, &final_call) {
            return Ok(crate::tool_executor::tool_error_result(tool_call, &error));
        }

        let HookInvocation::Decision(decide) = self
            .execute_hook(
                HookInvocationSite {
                    journal,
                    point: HookPoint::DecideToolCall,
                    iteration,
                    call_id: Some(tool_call.call_id.clone()),
                    input_digest: super::journal::tool_call_digest(&final_call),
                },
                cancellation,
                |control| {
                    self.hook_runtime.decide_tool_call(
                        DecideToolCallInput {
                            snapshot,
                            tool_call: &final_call,
                            tool: &effective_target,
                        },
                        control,
                    )
                },
                DecideToolCallDecision::check,
            )
            .await?
        else {
            return Err(AgentLoopError::Cancelled);
        };

        let mut message = match decide {
            DecideToolCallDecision::Execute => {
                self.execute_tool(&tool, &final_call, cancellation).await?
            }
            DecideToolCallDecision::Block { reason } => hook_blocked_result(tool_call, &reason),
        };

        // Cancellation here means the started tool's outcome must still be
        // recorded: the after hook degrades to Keep and the loop reaches its
        // regular cancellation boundary after the durable commits.
        let after = match self
            .execute_hook(
                HookInvocationSite {
                    journal,
                    point: HookPoint::AfterToolCall,
                    iteration,
                    call_id: Some(tool_call.call_id.clone()),
                    input_digest: super::journal::tool_call_digest(&final_call),
                },
                cancellation,
                |control| {
                    self.hook_runtime.after_tool_call(
                        AfterToolCallInput {
                            snapshot,
                            tool_call: &final_call,
                            tool: &effective_target,
                            result: &message,
                        },
                        control,
                    )
                },
                |_| Ok(()),
            )
            .await?
        {
            HookInvocation::Decision(decision) => decision,
            HookInvocation::Cancelled => AfterToolCallDecision::Keep,
        };
        if let AfterToolCallDecision::ReplaceResult { result } = after {
            message = ChatMessage::tool(final_call.call_id.clone(), result);
        }
        Ok(message)
    }

    async fn execute_tool(
        &self,
        tool: &Arc<dyn Tool>,
        tool_call: &ToolCall,
        cancellation: &CancellationToken,
    ) -> Result<ChatMessage, AgentLoopError> {
        match self
            .tool_executor
            .execute(tool, tool_call, cancellation)
            .await
        {
            Ok(message) => Ok(message),
            Err(ToolExecutorError::Durability { source }) => {
                Err(AgentLoopError::Durability { source })
            }
            Err(ToolExecutorError::Cancelled) => Err(AgentLoopError::Cancelled),
        }
    }

    /// Runs the prepare-next-turn hook and commits the iteration boundary of
    /// one completed tool cycle, applying the hook decision.
    ///
    /// The journaled `HookInvocationCompleted` always precedes the durable
    /// `IterationCompleted` boundary. A compact decision additionally executes
    /// in between: the kernel validates the cut in committed-context
    /// coordinates (see the `compaction` module), commits
    /// `TranscriptCompacted`, and rewrites the committed prefix to the summary
    /// marker message before the boundary commits. When replay already applied
    /// this iteration's compaction (the boundary crash window), the journaled
    /// decision is reused without re-executing or re-committing the
    /// compaction. Returns the finished outcome when the hook stopped the
    /// loop.
    async fn close_tool_cycle(
        &self,
        iteration_index: u64,
        context: &mut LoopContext,
        usage: Option<TokenUsage>,
        state: &mut IterationState,
        journal: &HookJournal,
        cancellation: &CancellationToken,
    ) -> Result<Option<LoopOutcome>, AgentLoopError> {
        let cut_base = state.iteration_start;
        let already_compacted = state.compacted_iterations.contains(&iteration_index);
        let committed = &*context;
        // Cancellation degrades the decision to Continue so the iteration
        // boundary is committed before the loop reaches its regular
        // cancellation check.
        let prepare = match self
            .execute_hook(
                HookInvocationSite {
                    journal,
                    point: HookPoint::PrepareNextTurn,
                    iteration: iteration_index,
                    call_id: None,
                    input_digest: super::journal::hook_address_digest(
                        iteration_index,
                        HookPoint::PrepareNextTurn,
                    ),
                },
                cancellation,
                |control| {
                    self.hook_runtime.prepare_next_turn(
                        PrepareNextTurnInput {
                            snapshot: HookSnapshot {
                                iteration: iteration_index,
                                context: committed,
                                usage,
                            },
                        },
                        control,
                    )
                },
                |decision| {
                    validate_prepare_next_turn(
                        decision,
                        &committed.messages,
                        cut_base,
                        already_compacted,
                    )
                },
            )
            .await?
        {
            HookInvocation::Decision(decision) => decision,
            HookInvocation::Cancelled => PrepareNextTurnDecision::Continue,
        };

        // A compact decision executes before the iteration boundary: the
        // compaction must be durable first so replay never rebuilds a
        // boundary whose baseline rewrite is missing. The summary marker is
        // a committed message of this run, so it joins the loop outcome.
        if let PrepareNextTurnDecision::Compact { upto, summary } = &prepare
            && !already_compacted
        {
            let marker = super::compaction::compaction_marker(summary);
            self.durable_events
                .append(DurableAgentEvent::TranscriptCompacted {
                    upto: u64::try_from(*upto).unwrap_or(u64::MAX),
                    summary: marker.clone(),
                    compacted_iteration: iteration_index,
                })
                .await?;
            context
                .messages
                .splice(..*upto, std::iter::once(marker.clone()));
            state.new_messages.push(marker);
        }

        self.durable_events
            .append(DurableAgentEvent::IterationCompleted {
                iteration: iteration_index,
                usage: usage.unwrap_or_default(),
            })
            .await?;
        state.iteration_start = context.messages.len();

        match prepare {
            PrepareNextTurnDecision::Continue | PrepareNextTurnDecision::Compact { .. } => Ok(None),
            PrepareNextTurnDecision::Inject { messages } => {
                state.pending_inject = Some(messages);
                Ok(None)
            }
            PrepareNextTurnDecision::Stop => {
                self.durable_events
                    .append(DurableAgentEvent::LoopFinished {
                        finish_reason: LoopCompletionReason::HookStopped.as_str().to_owned(),
                        usage: usage.unwrap_or_default(),
                    })
                    .await?;
                Ok(Some(LoopOutcome {
                    new_messages: std::mem::take(&mut state.new_messages),
                    completion: LoopCompletionReason::HookStopped,
                    usage: usage.unwrap_or_default(),
                }))
            }
        }
    }

    async fn run_started(
        &self,
        start: RunStart,
        cancellation: &CancellationToken,
        usage: &mut Option<TokenUsage>,
    ) -> Result<LoopOutcome, AgentLoopError> {
        let RunStart {
            mut context,
            prompts,
            first_iteration,
            continuation,
            journal,
            iteration_start,
            compacted_iterations,
        } = start;
        let mut seen_tool_call_ids = committed_tool_call_ids(&context.messages);
        if cancellation.is_cancelled() {
            return Err(AgentLoopError::Cancelled);
        }
        let mut state = IterationState {
            new_messages: Vec::with_capacity(prompts.len() + 1),
            pending_inject: None,
            iteration_start,
            compacted_iterations,
        };
        for prompt in prompts {
            self.durable_events
                .append(DurableAgentEvent::MessageAppended {
                    message: prompt.clone(),
                })
                .await?;
            context.messages.push(prompt.clone());
            state.new_messages.push(prompt);
        }

        let mut next_iteration = first_iteration;
        // A resumed run first finishes the work its frontier iteration had
        // already started: the missing tool-result suffix and/or the iteration
        // boundary. No transform-context hook or model request belongs to this
        // iteration; its hooks are resolved through the replayed journal.
        if let Some(continuation) = continuation {
            let iteration_index = u64::try_from(next_iteration).unwrap_or(u64::MAX);
            match continuation {
                ResumeContinuation::ToolSuffix(tool_calls) => {
                    context.messages.reserve(tool_calls.len());
                    state.new_messages.reserve(tool_calls.len());
                    for tool_call in &tool_calls {
                        if cancellation.is_cancelled() {
                            return Err(AgentLoopError::Cancelled);
                        }
                        let message = self
                            .execute_authorized_tool_call(
                                iteration_index,
                                tool_call,
                                &context,
                                *usage,
                                &journal,
                                cancellation,
                            )
                            .await?;
                        self.durable_events
                            .append(DurableAgentEvent::MessageAppended {
                                message: message.clone(),
                            })
                            .await?;
                        context.messages.push(message.clone());
                        state.new_messages.push(message);
                    }
                    if let Some(outcome) = self
                        .close_tool_cycle(
                            iteration_index,
                            &mut context,
                            *usage,
                            &mut state,
                            &journal,
                            cancellation,
                        )
                        .await?
                    {
                        return Ok(outcome);
                    }
                }
                ResumeContinuation::CloseIteration => {
                    if let Some(outcome) = self
                        .close_tool_cycle(
                            iteration_index,
                            &mut context,
                            *usage,
                            &mut state,
                            &journal,
                            cancellation,
                        )
                        .await?
                    {
                        return Ok(outcome);
                    }
                }
                ResumeContinuation::FinishLoop => {
                    self.durable_events
                        .append(DurableAgentEvent::IterationCompleted {
                            iteration: iteration_index,
                            usage: usage.unwrap_or_default(),
                        })
                        .await?;
                    // The finish reason of the committed final response is not
                    // part of the durable stream; it is projected as `stop`.
                    self.durable_events
                        .append(DurableAgentEvent::LoopFinished {
                            finish_reason: FinishReason::Stop.as_str().to_owned(),
                            usage: usage.unwrap_or_default(),
                        })
                        .await?;
                    return Ok(LoopOutcome {
                        new_messages: state.new_messages,
                        completion: LoopCompletionReason::Model(FinishReason::Stop),
                        usage: usage.unwrap_or_default(),
                    });
                }
            }
            next_iteration = next_iteration.saturating_add(1);
        }

        for iteration in next_iteration..self.limits.max_iterations {
            if cancellation.is_cancelled() {
                return Err(AgentLoopError::Cancelled);
            }
            let iteration_index = u64::try_from(iteration).unwrap_or(u64::MAX);
            let request_view = match state.pending_inject.take() {
                Some(injected) => {
                    let mut view = context.clone();
                    view.messages.extend(injected);
                    Cow::Owned(view)
                }
                None => Cow::Borrowed(&context),
            };
            let HookInvocation::Decision(transform) = self
                .execute_hook(
                    HookInvocationSite {
                        journal: &journal,
                        point: HookPoint::TransformContext,
                        iteration: iteration_index,
                        call_id: None,
                        input_digest: super::journal::hook_address_digest(
                            iteration_index,
                            HookPoint::TransformContext,
                        ),
                    },
                    cancellation,
                    |control| {
                        self.hook_runtime.transform_context(
                            TransformContextInput {
                                snapshot: HookSnapshot {
                                    iteration: iteration_index,
                                    context: &request_view,
                                    usage: *usage,
                                },
                            },
                            control,
                        )
                    },
                    |decision| match decision {
                        TransformContextDecision::Unchanged => Ok(()),
                        TransformContextDecision::Patch(patch) => {
                            validate_context_patch(&context.messages, patch)
                        }
                    },
                )
                .await?
            else {
                return Err(AgentLoopError::Cancelled);
            };
            let request_view = match transform {
                TransformContextDecision::Unchanged => request_view,
                TransformContextDecision::Patch(patch) => {
                    let mut view = request_view.into_owned();
                    apply_context_patch(&mut view, &patch);
                    Cow::Owned(view)
                }
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
            state.new_messages.push(assistant.message);

            if !tool_calls.is_empty() {
                context.messages.reserve(tool_calls.len());
                state.new_messages.reserve(tool_calls.len());
                for tool_call in &tool_calls {
                    if cancellation.is_cancelled() {
                        return Err(AgentLoopError::Cancelled);
                    }
                    let message = if finish_reason != FinishReason::ToolCalls {
                        unexecutable_tool_result(tool_call, finish_reason)
                    } else {
                        self.execute_authorized_tool_call(
                            iteration_index,
                            tool_call,
                            &context,
                            *usage,
                            &journal,
                            cancellation,
                        )
                        .await?
                    };
                    self.durable_events
                        .append(DurableAgentEvent::MessageAppended {
                            message: message.clone(),
                        })
                        .await?;
                    context.messages.push(message.clone());
                    state.new_messages.push(message);
                }

                if let Some(outcome) = self
                    .close_tool_cycle(
                        iteration_index,
                        &mut context,
                        *usage,
                        &mut state,
                        &journal,
                        cancellation,
                    )
                    .await?
                {
                    return Ok(outcome);
                }
                continue;
            }

            self.durable_events
                .append(DurableAgentEvent::IterationCompleted {
                    iteration: iteration_index,
                    usage: usage.unwrap_or_default(),
                })
                .await?;

            self.durable_events
                .append(DurableAgentEvent::LoopFinished {
                    finish_reason: finish_reason.as_str().to_owned(),
                    usage: usage.unwrap_or_default(),
                })
                .await?;
            return Ok(LoopOutcome {
                new_messages: state.new_messages,
                completion: LoopCompletionReason::Model(finish_reason),
                usage: usage.unwrap_or_default(),
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

/// A validated resume bound to the exact [`AgentLoop`] that prepared it.
///
/// Produced only by [`AgentLoop::prepare_resume`], which completes all replay
/// validation before any side effect. The value is opaque and intentionally
/// not `Clone` or `Serialize`: it can be run exactly once, on the runtime it
/// was prepared from, through [`PreparedResume::run`]. There is no entry
/// point that hands the prepared run state to another loop.
pub struct PreparedResume {
    agent_loop: Arc<AgentLoop>,
    start: RunStart,
}

impl PreparedResume {
    /// Runs the prepared continuation once, consuming this value.
    ///
    /// The run continues through the same path as [`AgentLoop::resume`]: it
    /// appends continuation events to the same durable sink and never records
    /// a second [`DurableAgentEvent::LoopStarted`].
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`AgentLoop::resume`] once the continuation
    /// starts.
    ///
    /// # Cancellation safety
    ///
    /// Same contract as [`AgentLoop::run`].
    // Tracing skips the consumed prepared state: it carries user content.
    #[tracing::instrument(skip_all)]
    pub async fn run(self, cancellation: CancellationToken) -> Result<LoopOutcome, AgentLoopError> {
        if self.agent_loop.limits.max_iterations == 0 {
            return Err(AgentLoopError::IterationLimitExceeded { maximum: 0 });
        }
        // Usage is a volatile observation, not resume state: the first hook
        // boundary after a resume observes `None` until the next model
        // response reports usage.
        let mut usage: Option<TokenUsage> = None;
        let result = self
            .agent_loop
            .run_started(self.start, &cancellation, &mut usage)
            .await;
        self.agent_loop.finish_run(result, usage).await
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

    /// Sets the tool executor.
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

/// Validates a prepare-next-turn decision: the decision contract itself,
/// plus the committed-coordinate cut check for a compaction that still needs
/// to execute. A compaction whose `TranscriptCompacted` already replayed (the
/// boundary crash window) skips the cut check: its prefix coordinates no
/// longer exist in the rebuilt context.
fn validate_prepare_next_turn(
    decision: &PrepareNextTurnDecision,
    committed: &[ChatMessage],
    iteration_start: usize,
    already_compacted: bool,
) -> Result<(), HookFailure> {
    decision.check()?;
    if let PrepareNextTurnDecision::Compact { upto, .. } = decision
        && !already_compacted
    {
        super::compaction::validate_compaction_cut(committed, *upto, iteration_start)?;
    }
    Ok(())
}

/// Validates a context patch against the committed messages: `upto` is a
/// zero-based, left-closed/right-open prefix end that must stay in bounds and
/// must not cut a tool_call/tool_result pair (a dropped assistant message's
/// results must be dropped with it). A rewrite summary must not itself open a
/// tool-call pair or pose as a tool result. A `Composite` validates its
/// sub-patches in order against the evolving view each one produces; a
/// sub-patch that is itself a `Composite` is rejected.
pub(crate) fn validate_context_patch(
    committed: &[ChatMessage],
    patch: &ContextPatch,
) -> Result<(), HookFailure> {
    let upto = match patch {
        ContextPatch::ReplaceSystemPrompt(_) => return Ok(()),
        ContextPatch::Composite(patches) => {
            return validate_composite_patch(committed, patches);
        }
        ContextPatch::DropHistory { upto } => *upto,
        ContextPatch::RewriteHistory { upto, summary } => {
            if !summary.tool_calls.is_empty() || summary.tool_call_id.is_some() {
                return Err(HookFailure::InvalidOutput);
            }
            *upto
        }
        _ => return Err(HookFailure::InvalidOutput),
    };
    if upto > committed.len() {
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

/// Validates a non-empty patch composition sequentially: every sub-patch must
/// be valid against the view produced by the sub-patches before it. A nested
/// composition is rejected: its inner message rewrites would not advance the
/// scratch view, so later `upto` checks would run against a stale view.
fn validate_composite_patch(
    committed: &[ChatMessage],
    patches: &[ContextPatch],
) -> Result<(), HookFailure> {
    if patches.is_empty() {
        return Err(HookFailure::InvalidOutput);
    }
    let mut scratch = committed.to_vec();
    for patch in patches {
        if matches!(patch, ContextPatch::Composite(_)) {
            return Err(HookFailure::InvalidOutput);
        }
        validate_context_patch(&scratch, patch)?;
        apply_messages_patch(&mut scratch, patch);
    }
    Ok(())
}

/// Applies one validated patch's message rewrite to a scratch message list.
fn apply_messages_patch(messages: &mut Vec<ChatMessage>, patch: &ContextPatch) {
    match patch {
        ContextPatch::DropHistory { upto } => {
            messages.drain(..*upto);
        }
        ContextPatch::RewriteHistory { upto, summary } => {
            messages.splice(..*upto, std::iter::once(summary.clone()));
        }
        // System-prompt replacements do not touch messages.
        ContextPatch::ReplaceSystemPrompt(_) => {}
        // Validation rejects nested compositions, so one never reaches the
        // scratch view.
        ContextPatch::Composite(_) => {
            debug_assert!(false, "validation rejects nested composite patches");
        }
        // Unknown future variants were already rejected by validation.
        _ => {}
    }
}

/// Applies a validated context patch to the request view. The patch only ever
/// rewrites the view: the committed transcript, durable messages, and the loop
/// outcome never observe it.
pub(crate) fn apply_context_patch(view: &mut LoopContext, patch: &ContextPatch) {
    match patch {
        ContextPatch::ReplaceSystemPrompt(prompt) => {
            view.system_prompt.clone_from(prompt);
        }
        ContextPatch::Composite(patches) => {
            for sub_patch in patches {
                apply_context_patch(view, sub_patch);
            }
        }
        ContextPatch::DropHistory { upto } => {
            view.messages.drain(..*upto);
        }
        ContextPatch::RewriteHistory { upto, summary } => {
            view.messages
                .splice(..*upto, std::iter::once(summary.clone()));
        }
        // Unknown future variants were already rejected by validation.
        _ => {}
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

/// Terminal outcome of one awaited hook invocation.
enum HookOutcome<D> {
    /// The runtime returned a decision in time.
    Decision(D),
    /// The invocation failed typed or missed its deadline.
    Failed(HookFailure),
    /// Cancellation won before or during the invocation.
    Cancelled,
}

impl<D> From<Result<D, HookFailure>> for HookOutcome<D> {
    fn from(decision: Result<D, HookFailure>) -> Self {
        match decision {
            Ok(decision) => Self::Decision(decision),
            Err(failure) => Self::Failed(failure),
        }
    }
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
