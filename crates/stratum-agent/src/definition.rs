//! Public agent runtime definitions.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use stratum_core::{
    AgentEvent, AgentId, AgentRuntimeContext, ApprovalDecision, ApprovalId, ChatMessage, ChatRole,
    HistoryQuery, ModelConfig, RuntimeEvent, SessionId, TokenUsage, TurnId, TurnRuntimeSnapshot,
};
use stratum_infra::event_stream_bus::EventStreamBus;
use stratum_llm::LlmProvider;
use stratum_store::{AgentStatus, AgentStore, MAX_HISTORY_PAGE_SIZE};
use stratum_tools::ToolRegistry;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::AgentError;

pub(crate) struct ApprovalResolution {
    pub(crate) decision: ApprovalDecision,
    pub(crate) response: oneshot::Sender<Result<(), AgentError>>,
}

pub(crate) struct PendingApproval {
    approval_id: ApprovalId,
    decision: oneshot::Sender<ApprovalResolution>,
}

/// Runtime tuning for an agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentConfig {
    /// Maximum LLM turns in one run.
    pub max_turns: usize,
    /// Maximum tool calls accepted from one assistant turn.
    pub max_tool_calls_per_turn: usize,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: 16,
            max_tool_calls_per_turn: 16,
        }
    }
}

/// Stateful agent that owns conversation history.
#[derive(Clone)]
pub struct Agent {
    pub(crate) id: AgentId,
    pub(crate) name: String,
    pub(crate) system_prompt: String,
    pub(crate) llm_provider: Arc<dyn LlmProvider>,
    pub(crate) model_config: ModelConfig,
    pub(crate) tool_registry: Arc<dyn ToolRegistry>,
    pub(crate) event_bus: Arc<dyn EventStreamBus>,
    pub(crate) store: Arc<dyn AgentStore>,
    pub(crate) config: AgentConfig,
    pub(crate) history: Arc<Mutex<Vec<ChatMessage>>>,
    pub(crate) usage: Arc<Mutex<TokenUsage>>,
    pub(crate) active: Arc<AtomicBool>,
    current_context: Arc<Mutex<Option<AgentRuntimeContext>>>,
    current_turn_id: Arc<Mutex<Option<TurnId>>>,
    pub(crate) cancel: Arc<Mutex<Option<CancellationToken>>>,
    pub(crate) active_approval: Arc<Mutex<Option<PendingApproval>>>,
}

struct ActiveGuard<'a> {
    active: &'a AtomicBool,
    armed: bool,
}

pub(crate) struct ActiveApprovalGuard<'a> {
    active_approval: &'a Mutex<Option<PendingApproval>>,
    approval_id: ApprovalId,
}

struct ResumeState {
    context: AgentRuntimeContext,
    turn_id: TurnId,
    runtime_snapshot: TurnRuntimeSnapshot,
    next_iteration: u64,
    usage: TokenUsage,
    history: Vec<ChatMessage>,
    active_turn_start: usize,
}

impl<'a> ActiveGuard<'a> {
    fn new(active: &'a AtomicBool) -> Self {
        Self {
            active,
            armed: true,
        }
    }

    fn disarm(mut self) {
        self.armed = false;
    }
}

impl Drop for ActiveGuard<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.active.store(false, Ordering::SeqCst);
        }
    }
}

impl<'a> ActiveApprovalGuard<'a> {
    pub(crate) fn new(
        active_approval: &'a Mutex<Option<PendingApproval>>,
        approval_id: ApprovalId,
        decision: oneshot::Sender<ApprovalResolution>,
    ) -> Self {
        *active_approval
            .lock()
            .expect("active approval mutex should not be poisoned") = Some(PendingApproval {
            approval_id,
            decision,
        });
        Self {
            active_approval,
            approval_id,
        }
    }

    pub(crate) fn clear(&mut self) {
        let mut active_approval = self
            .active_approval
            .lock()
            .expect("active approval mutex should not be poisoned");
        if active_approval
            .as_ref()
            .is_some_and(|approval| approval.approval_id == self.approval_id)
        {
            *active_approval = None;
        }
    }
}

impl Drop for ActiveApprovalGuard<'_> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl Agent {
    /// Creates an agent builder.
    #[must_use]
    pub fn builder() -> AgentBuilder {
        AgentBuilder::default()
    }

    /// Returns whether this process is currently executing or resuming a Turn.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Starts one user turn in a host-supplied runtime context.
    ///
    /// # Errors
    ///
    /// Returns an error if the input message role is not `User`, another operation is
    /// active, or the exact runtime snapshot and required turn preamble cannot be committed.
    pub async fn run_turn(
        &self,
        context: AgentRuntimeContext,
        message: ChatMessage,
    ) -> Result<TurnId, AgentError> {
        if message.role != ChatRole::User {
            return Err(AgentError::InvalidInputMessageRole { role: message.role });
        }

        if self.active.swap(true, Ordering::SeqCst) {
            return Err(AgentError::OperationAlreadyActive);
        }
        let active_guard = ActiveGuard::new(&self.active);

        let state = self.store.load_agent().await?;
        if state.agent_id != self.id {
            return Err(AgentError::ResumeAgentMismatch {
                expected: self.id,
                actual: state.agent_id,
            });
        }
        if state.status == AgentStatus::Running {
            return Err(AgentError::PersistedTurnRequiresResume {
                session_id: state.session_id.ok_or(AgentError::ResumeSessionMissing)?,
                turn_id: state.turn_id.ok_or(AgentError::ResumeTurnMissing)?,
            });
        }
        let history = self.load_complete_history(state.last_seq).await?;
        self.commit_history(history.clone());

        let turn_id = TurnId::new();
        let runtime_snapshot = TurnRuntimeSnapshot::new(
            state.agent_version_id,
            self.model_config.clone(),
            self.tool_registry.fingerprint()?,
            state.skill_set_version_id,
            state.extension_set_version_id,
            state.hook_handler_versions,
        );
        self.store
            .start_turn(&context, turn_id, runtime_snapshot)
            .await?;
        let cancel = CancellationToken::new();
        *self
            .current_context
            .lock()
            .expect("current context mutex should not be poisoned") = Some(context.clone());
        *self
            .current_turn_id
            .lock()
            .expect("current turn mutex should not be poisoned") = Some(turn_id);
        *self
            .cancel
            .lock()
            .expect("cancel mutex should not be poisoned") = Some(cancel);
        self.set_usage(TokenUsage::default());

        let mut history = history;
        self.publish_required_agent_event(AgentEvent::Started, None)
            .await?;
        self.commit_message(message.clone()).await?;
        history.push(message);

        let agent = self.clone();
        let active = Arc::clone(&self.active);

        tokio::spawn(async move {
            let result = agent.clone().continue_turn_loop(history, 0).await;
            active.store(false, Ordering::SeqCst);
            agent
                .finish_background_continuation(context.session_id, result)
                .await;
        });
        active_guard.disarm();

        Ok(turn_id)
    }

    /// Resumes the persisted running turn at its last durable iteration boundary.
    ///
    /// # Errors
    ///
    /// Returns an error when another operation is active, persisted state is not
    /// resumable, history is invalid, or the store cannot be read.
    pub async fn resume(&self) -> Result<TurnId, AgentError> {
        if self.active.swap(true, Ordering::SeqCst) {
            return Err(AgentError::OperationAlreadyActive);
        }
        let active_guard = ActiveGuard::new(&self.active);

        let resumed = self.initialize_resume().await?;
        self.validate_runtime_snapshot(&resumed.runtime_snapshot)?;
        let continuation = self.prepare_resume_continuation(
            resumed.history,
            resumed.active_turn_start,
            resumed.next_iteration,
        )?;

        let cancel = CancellationToken::new();
        *self
            .current_context
            .lock()
            .expect("current context mutex should not be poisoned") = Some(resumed.context.clone());
        *self
            .current_turn_id
            .lock()
            .expect("current turn mutex should not be poisoned") = Some(resumed.turn_id);
        *self
            .cancel
            .lock()
            .expect("cancel mutex should not be poisoned") = Some(cancel);
        self.set_usage(resumed.usage);
        self.commit_history(continuation.history().to_vec());

        let agent = self.clone();
        let active = Arc::clone(&self.active);
        tokio::spawn(async move {
            let result = agent.clone().continue_resumed_turn_loop(continuation).await;
            active.store(false, Ordering::SeqCst);
            agent
                .finish_background_continuation(resumed.context.session_id, result)
                .await;
        });
        active_guard.disarm();

        Ok(resumed.turn_id)
    }

    /// Loads the durable complete message history into this inactive agent.
    ///
    /// # Errors
    ///
    /// Returns an error when an operation is active, the persisted turn must be resumed,
    /// the store identity differs, or the persisted history cannot be read as contiguous
    /// agent messages.
    pub async fn load_history(&self) -> Result<(), AgentError> {
        if self
            .active
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Err(AgentError::OperationAlreadyActive);
        }
        let _active_guard = ActiveGuard::new(&self.active);
        let state = self.store.load_agent().await?;
        if state.status == AgentStatus::Running {
            return Err(AgentError::LoadHistoryRunning);
        }
        if state.agent_id != self.id {
            return Err(AgentError::ResumeAgentMismatch {
                expected: self.id,
                actual: state.agent_id,
            });
        }
        let history = self.load_complete_history(state.last_seq).await?;
        self.commit_history(history);
        Ok(())
    }

    async fn load_complete_history(&self, last_seq: u64) -> Result<Vec<ChatMessage>, AgentError> {
        let mut history = Vec::new();
        let mut after_seq = 0;
        while after_seq < last_seq {
            let page = self
                .store
                .history_page(HistoryQuery {
                    after_seq,
                    through_seq: Some(last_seq),
                    limit: MAX_HISTORY_PAGE_SIZE,
                })
                .await?;
            if page.through_seq != last_seq
                || page.events.is_empty()
                || page.next_front_seq <= after_seq
                || page.next_front_seq > last_seq
            {
                return Err(AgentError::InvalidResumeHistory);
            }

            let mut expected_seq = after_seq;
            for envelope in page.events {
                expected_seq = expected_seq
                    .checked_add(1)
                    .ok_or(AgentError::InvalidResumeHistory)?;
                if envelope.message_seq() != Some(expected_seq) {
                    return Err(AgentError::InvalidResumeHistory);
                }
                let RuntimeEvent::Agent {
                    agent_id, event, ..
                } = envelope.event
                else {
                    return Err(AgentError::InvalidResumeHistory);
                };
                if agent_id != self.id {
                    return Err(AgentError::ResumeAgentMismatch {
                        expected: self.id,
                        actual: agent_id,
                    });
                }
                let AgentEvent::Message { message, .. } = event else {
                    return Err(AgentError::InvalidResumeHistory);
                };
                history.push(message);
            }
            if expected_seq != page.next_front_seq
                || page.has_more != (page.next_front_seq < last_seq)
            {
                return Err(AgentError::InvalidResumeHistory);
            }
            after_seq = page.next_front_seq;
        }

        Ok(history)
    }

    async fn finish_background_continuation(
        &self,
        session_id: SessionId,
        result: Result<(), AgentError>,
    ) {
        match &result {
            Err(AgentError::Cancelled) => tracing::debug!(
                agent_id = %self.id,
                session_id = %session_id,
                "agent continuation cancelled"
            ),
            Err(error) => tracing::error!(
                agent_id = %self.id,
                session_id = %session_id,
                error_kind = continuation_error_kind(error),
                "agent continuation failed"
            ),
            Ok(()) => {}
        }
        let history = match self.store.load_agent().await {
            Ok(state) => self.load_complete_history(state.last_seq).await,
            Err(error) => Err(error.into()),
        };
        match history {
            Ok(history) => self.commit_history(history),
            Err(error) => tracing::error!(
                agent_id = %self.id,
                session_id = %session_id,
                error_kind = continuation_error_kind(&error),
                "agent history refresh failed"
            ),
        }
    }

    async fn initialize_resume(&self) -> Result<ResumeState, AgentError> {
        let state = self.store.load_agent().await?;
        if state.status != AgentStatus::Running {
            return Err(AgentError::ResumeNotRunning {
                actual: state.status,
            });
        }
        if state.agent_id != self.id {
            return Err(AgentError::ResumeAgentMismatch {
                expected: self.id,
                actual: state.agent_id,
            });
        }
        let session_id = state.session_id.ok_or(AgentError::ResumeSessionMissing)?;
        let turn_id = state.turn_id.ok_or(AgentError::ResumeTurnMissing)?;
        let location = state
            .location
            .clone()
            .ok_or(AgentError::ResumeLocationMissing)?;
        let runtime_snapshot = state
            .turn_runtime_snapshot
            .clone()
            .ok_or(AgentError::ResumeSnapshotMissing)?;
        let component = if runtime_snapshot.agent_version_id != state.agent_version_id {
            Some("agent_version")
        } else if runtime_snapshot.skill_set_version_id != state.skill_set_version_id {
            Some("skill_set_version")
        } else if runtime_snapshot.extension_set_version_id != state.extension_set_version_id {
            Some("extension_set_version")
        } else if runtime_snapshot.hook_handler_versions != state.hook_handler_versions {
            Some("hook_handler_order")
        } else {
            None
        };
        if let Some(component) = component {
            return Err(AgentError::ResumeSnapshotMismatch { component });
        }

        let mut history = Vec::new();
        let mut after_seq = 0;
        let mut has_active_user_message = false;
        let mut active_turn_start = None;
        while after_seq < state.last_seq {
            let page = self
                .store
                .history_page(HistoryQuery {
                    after_seq,
                    through_seq: Some(state.last_seq),
                    limit: MAX_HISTORY_PAGE_SIZE,
                })
                .await?;
            if page.through_seq != state.last_seq
                || page.events.is_empty()
                || page.next_front_seq <= after_seq
                || page.next_front_seq > state.last_seq
            {
                return Err(AgentError::InvalidResumeHistory);
            }

            let mut expected_seq = after_seq;
            for envelope in page.events {
                expected_seq = expected_seq
                    .checked_add(1)
                    .ok_or(AgentError::InvalidResumeHistory)?;
                if envelope.message_seq() != Some(expected_seq) {
                    return Err(AgentError::InvalidResumeHistory);
                }
                let RuntimeEvent::Agent {
                    agent_id,
                    turn_id: message_turn_id,
                    event,
                    ..
                } = envelope.event
                else {
                    return Err(AgentError::InvalidResumeHistory);
                };
                if agent_id != self.id {
                    return Err(AgentError::ResumeAgentMismatch {
                        expected: self.id,
                        actual: agent_id,
                    });
                }
                let AgentEvent::Message { message, .. } = event else {
                    return Err(AgentError::InvalidResumeHistory);
                };
                if message_turn_id == turn_id {
                    if active_turn_start.is_none() {
                        active_turn_start = Some(history.len());
                    }
                    if message.role == ChatRole::User {
                        has_active_user_message = true;
                    }
                } else if active_turn_start.is_some() {
                    return Err(AgentError::InvalidResumeHistory);
                }
                history.push(message);
            }
            if expected_seq != page.next_front_seq
                || page.has_more != (page.next_front_seq < state.last_seq)
            {
                return Err(AgentError::InvalidResumeHistory);
            }
            after_seq = page.next_front_seq;
        }

        if !has_active_user_message {
            return Err(AgentError::InvalidResumeHistory);
        }

        Ok(ResumeState {
            context: AgentRuntimeContext::new(session_id, location),
            turn_id,
            runtime_snapshot,
            next_iteration: state.next_iteration,
            usage: state.usage,
            history,
            active_turn_start: active_turn_start.ok_or(AgentError::InvalidResumeHistory)?,
        })
    }

    /// Cancels the current run, if any.
    pub fn stop(&self) {
        if let Some(cancel) = self
            .cancel
            .lock()
            .expect("cancel mutex should not be poisoned")
            .as_ref()
        {
            cancel.cancel();
        }
    }

    /// Resolves the active tool approval request.
    ///
    /// # Errors
    ///
    /// Returns an error when no turn is active, the approval id is not active, or
    /// the turn ends before accepting the command.
    pub async fn resolve_tool_approval(
        &self,
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    ) -> Result<(), AgentError> {
        if !self.active.load(Ordering::SeqCst) {
            return Err(AgentError::NoActiveTurn);
        }
        let pending = {
            let mut active_approval = self
                .active_approval
                .lock()
                .expect("active approval mutex should not be poisoned");
            if active_approval
                .as_ref()
                .is_none_or(|approval| approval.approval_id != approval_id)
            {
                return Err(AgentError::ApprovalNotFound { approval_id });
            }
            active_approval
                .take()
                .expect("matching active approval should be present")
        };
        let (response, receiver) = oneshot::channel();
        pending
            .decision
            .send(ApprovalResolution { decision, response })
            .map_err(|_| AgentError::NoActiveTurn)?;
        receiver.await.map_err(|_| AgentError::NoActiveTurn)?
    }

    /// Returns the current session id, if one has been started or resumed.
    pub fn current_session(&self) -> Option<SessionId> {
        self.current_context
            .lock()
            .expect("current context mutex should not be poisoned")
            .as_ref()
            .map(|context| context.session_id)
    }

    /// Returns the current immutable Agent runtime context.
    pub fn current_context(&self) -> Option<AgentRuntimeContext> {
        self.current_context
            .lock()
            .expect("current context mutex should not be poisoned")
            .clone()
    }

    /// Returns the current turn id, if one has been started.
    pub fn current_turn(&self) -> Option<TurnId> {
        *self
            .current_turn_id
            .lock()
            .expect("current turn mutex should not be poisoned")
    }

    /// Returns the configured event bus.
    pub fn event_bus(&self) -> Arc<dyn EventStreamBus> {
        Arc::clone(&self.event_bus)
    }

    fn set_usage(&self, usage: TokenUsage) {
        *self
            .usage
            .lock()
            .expect("usage mutex should not be poisoned") = usage;
    }

    pub(crate) fn commit_history(&self, history: Vec<ChatMessage>) {
        *self
            .history
            .lock()
            .expect("agent history mutex should not be poisoned") = history;
    }

    fn validate_runtime_snapshot(&self, snapshot: &TurnRuntimeSnapshot) -> Result<(), AgentError> {
        if snapshot.model != self.model_config {
            return Err(AgentError::ResumeSnapshotMismatch { component: "model" });
        }
        if snapshot.tool_set_fingerprint != self.tool_registry.fingerprint()? {
            return Err(AgentError::ResumeSnapshotMismatch { component: "tools" });
        }
        Ok(())
    }
}

fn continuation_error_kind(error: &AgentError) -> &'static str {
    match error {
        AgentError::InvalidInputMessageRole { .. } => "invalid_input_message_role",
        AgentError::OperationAlreadyActive => "operation_already_active",
        AgentError::PersistedTurnRequiresResume { .. } => "persisted_turn_requires_resume",
        AgentError::NoActiveTurn => "no_active_turn",
        AgentError::ApprovalNotFound { .. } => "approval_not_found",
        AgentError::UnsupportedApprovalDecision => "unsupported_approval_decision",
        AgentError::Llm { .. } => "llm",
        AgentError::EventBus { .. } => "event_bus",
        AgentError::Store { .. } => "store",
        AgentError::ToolRuntime { .. } => "tool_runtime",
        AgentError::ResumeNotRunning { .. } => "resume_not_running",
        AgentError::LoadHistoryRunning => "load_history_running",
        AgentError::ResumeSessionMissing => "resume_session_missing",
        AgentError::ResumeTurnMissing => "resume_turn_missing",
        AgentError::ResumeLocationMissing => "resume_location_missing",
        AgentError::ResumeSnapshotMissing => "resume_snapshot_missing",
        AgentError::ResumeSnapshotMismatch { .. } => "resume_snapshot_mismatch",
        AgentError::ResumeAgentMismatch { .. } => "resume_agent_mismatch",
        AgentError::InvalidResumeHistory => "invalid_resume_history",
        AgentError::IterationOutOfRange { .. } => "iteration_out_of_range",
        AgentError::MissingBuilderField { .. } => "missing_builder_field",
        AgentError::ToolCallLimitExceeded { .. } => "tool_call_limit_exceeded",
        AgentError::TurnLimitExceeded { .. } => "turn_limit_exceeded",
        AgentError::IncompleteToolCall { .. } => "incomplete_tool_call",
        AgentError::Cancelled => "cancelled",
    }
}

/// Builder for [`Agent`].
#[derive(Default)]
pub struct AgentBuilder {
    id: Option<AgentId>,
    name: Option<String>,
    system_prompt: Option<String>,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    model_config: Option<ModelConfig>,
    tool_registry: Option<Arc<dyn ToolRegistry>>,
    event_bus: Option<Arc<dyn EventStreamBus>>,
    store: Option<Arc<dyn AgentStore>>,
    config: Option<AgentConfig>,
}

impl AgentBuilder {
    /// Sets the agent id.
    #[must_use]
    pub fn id(mut self, id: AgentId) -> Self {
        self.id = Some(id);
        self
    }

    /// Sets the agent name.
    #[must_use]
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Sets the system prompt.
    #[must_use]
    pub fn system_prompt(mut self, system_prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(system_prompt.into());
        self
    }

    /// Sets the LLM provider.
    #[must_use]
    pub fn llm_provider(mut self, llm_provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(llm_provider);
        self
    }

    /// Sets the fully resolved model configuration pinned in new Turn snapshots.
    #[must_use]
    pub fn model_config(mut self, model_config: ModelConfig) -> Self {
        self.model_config = Some(model_config);
        self
    }

    /// Sets the tool registry.
    #[must_use]
    pub fn tool_registry(mut self, tool_registry: Arc<dyn ToolRegistry>) -> Self {
        self.tool_registry = Some(tool_registry);
        self
    }

    /// Sets the event bus.
    #[must_use]
    pub fn event_bus(mut self, event_bus: Arc<dyn EventStreamBus>) -> Self {
        self.event_bus = Some(event_bus);
        self
    }

    /// Sets the durable agent store.
    #[must_use]
    pub fn store(mut self, store: Arc<dyn AgentStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// Sets runtime config.
    #[must_use]
    pub fn config(mut self, config: AgentConfig) -> Self {
        self.config = Some(config);
        self
    }

    /// Builds an [`Agent`].
    ///
    /// # Errors
    ///
    /// Returns an error when a required builder field is missing.
    pub fn build(self) -> Result<Agent, AgentError> {
        let llm_provider = self.llm_provider.ok_or(AgentError::MissingBuilderField {
            field: "llm_provider",
        })?;
        let model_config = self
            .model_config
            .unwrap_or_else(|| ModelConfig::new(llm_provider.model_id(), serde_json::Map::new()));
        Ok(Agent {
            id: self.id.unwrap_or_default(),
            name: self
                .name
                .ok_or(AgentError::MissingBuilderField { field: "name" })?,
            system_prompt: self.system_prompt.ok_or(AgentError::MissingBuilderField {
                field: "system_prompt",
            })?,
            llm_provider,
            model_config,
            tool_registry: self.tool_registry.ok_or(AgentError::MissingBuilderField {
                field: "tool_registry",
            })?,
            event_bus: self
                .event_bus
                .ok_or(AgentError::MissingBuilderField { field: "event_bus" })?,
            store: self
                .store
                .ok_or(AgentError::MissingBuilderField { field: "store" })?,
            config: self.config.unwrap_or_default(),
            history: Arc::new(Mutex::new(Vec::new())),
            usage: Arc::new(Mutex::new(TokenUsage::default())),
            active: Arc::new(AtomicBool::new(false)),
            current_context: Arc::new(Mutex::new(None)),
            current_turn_id: Arc::new(Mutex::new(None)),
            cancel: Arc::new(Mutex::new(None)),
            active_approval: Arc::new(Mutex::new(None)),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    use async_trait::async_trait;
    use futures_util::{StreamExt, stream};
    use stratum_core::{
        ChatMessage, HistoryPage, HistoryQuery, ModelConfig, ModelId, NewAgentMessage, ReplayStart,
        StreamEnvelope, TurnRuntimeSnapshot,
    };
    use stratum_infra::event_stream_bus::{
        EventStream, EventStreamBusError, InMemoryEventStreamBus,
    };
    use stratum_llm::{
        ChatRequest, ChatResponse, ChatStream, ChatStreamEvent, FinishReason, LlmError,
        LlmProvider, MockLlmProvider,
    };
    use stratum_store::{AgentState, AgentStatus, AgentStore, StoreError};
    use stratum_tools::BuiltinToolRegistry;
    use tokio::{
        sync::watch,
        time::{Duration, timeout},
    };

    use super::*;

    struct UnitTestStore {
        state: Mutex<AgentState>,
        history: Mutex<Vec<StreamEnvelope>>,
    }

    impl UnitTestStore {
        fn new() -> Self {
            Self {
                state: Mutex::new(AgentState::new_configured(
                    test_agent_id(),
                    "test-agent".to_owned(),
                    ModelConfig::new(
                        "mock:mock-model".parse().expect("model id parses"),
                        serde_json::Map::new(),
                    ),
                )),
                history: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl AgentStore for UnitTestStore {
        async fn load_agent(&self) -> Result<AgentState, StoreError> {
            Ok(self.state.lock().expect("state lock").clone())
        }

        async fn update_state(
            &self,
            status: AgentStatus,
            session_id: Option<SessionId>,
            turn_id: Option<TurnId>,
            usage: TokenUsage,
        ) -> Result<AgentState, StoreError> {
            let mut state = self.state.lock().expect("state lock");
            state.status = status;
            state.session_id = session_id;
            state.turn_id = turn_id;
            state.usage = usage;
            Ok(state.clone())
        }

        async fn start_turn(
            &self,
            context: &AgentRuntimeContext,
            turn_id: TurnId,
            runtime_snapshot: TurnRuntimeSnapshot,
        ) -> Result<AgentState, StoreError> {
            let mut state = self.state.lock().expect("state lock");
            state.status = AgentStatus::Running;
            state.session_id = Some(context.session_id);
            state.turn_id = Some(turn_id);
            state.location = Some(context.location.clone());
            state.turn_runtime_snapshot = Some(runtime_snapshot);
            state.next_iteration = 0;
            Ok(state.clone())
        }

        async fn complete_iteration(
            &self,
            _session_id: SessionId,
            _turn_id: TurnId,
            iteration: u64,
            usage: TokenUsage,
        ) -> Result<AgentState, StoreError> {
            let mut state = self.state.lock().expect("state lock");
            state.next_iteration = iteration
                .checked_add(1)
                .ok_or(StoreError::IterationOverflow)?;
            state.usage = usage;
            Ok(state.clone())
        }

        async fn append_message(
            &self,
            message: NewAgentMessage,
        ) -> Result<StreamEnvelope, StoreError> {
            let mut history = self.history.lock().expect("history lock");
            let seq = u64::try_from(history.len()).expect("history length fits u64") + 1;
            let committed = message.into_envelope(seq);
            history.push(committed.clone());
            self.state.lock().expect("state lock").last_seq = seq;
            Ok(committed)
        }

        async fn history_page(&self, query: HistoryQuery) -> Result<HistoryPage, StoreError> {
            let history = self.history.lock().expect("history lock");
            let through_seq = query
                .through_seq
                .unwrap_or(u64::try_from(history.len()).expect("history length fits u64"));
            let events = history
                .iter()
                .skip(usize::try_from(query.after_seq).expect("sequence fits usize"))
                .take(query.limit)
                .cloned()
                .collect::<Vec<_>>();
            let next_front_seq = events
                .last()
                .and_then(StreamEnvelope::message_seq)
                .unwrap_or(query.after_seq);
            Ok(HistoryPage {
                through_seq,
                events,
                next_front_seq,
                has_more: next_front_seq < through_seq,
            })
        }
    }

    fn test_agent_id() -> AgentId {
        "01900000-0000-7000-8000-000000000000"
            .parse()
            .expect("test agent id parses")
    }

    fn test_store() -> Arc<dyn AgentStore> {
        Arc::new(UnitTestStore::new())
    }

    fn test_agent() -> Agent {
        Agent::builder()
            .id(test_agent_id())
            .name("test-agent")
            .system_prompt("be helpful")
            .llm_provider(Arc::new(MockLlmProvider::new()))
            .tool_registry(Arc::new(BuiltinToolRegistry::default()))
            .event_bus(Arc::new(InMemoryEventStreamBus::default()))
            .store(test_store())
            .build()
            .expect("agent should build")
    }

    fn test_agent_with_bus(event_bus: Arc<dyn EventStreamBus>) -> Agent {
        Agent::builder()
            .id(test_agent_id())
            .name("test-agent")
            .system_prompt("be helpful")
            .llm_provider(Arc::new(MockLlmProvider::new()))
            .tool_registry(Arc::new(BuiltinToolRegistry::default()))
            .event_bus(event_bus)
            .store(test_store())
            .build()
            .expect("agent should build")
    }

    #[derive(Default)]
    struct RecordingBus {
        agent_event_types: Mutex<Vec<&'static str>>,
    }

    impl RecordingBus {
        fn agent_event_types(&self) -> Vec<&'static str> {
            self.agent_event_types
                .lock()
                .expect("recording bus mutex should not be poisoned")
                .clone()
        }
    }

    #[async_trait]
    impl EventStreamBus for RecordingBus {
        async fn publish(&self, envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
            let RuntimeEvent::Agent { event, .. } = envelope.event else {
                return Ok(());
            };
            let event_type = match event {
                AgentEvent::Started => "started",
                AgentEvent::Message { .. } => "message",
                _ => return Ok(()),
            };
            self.agent_event_types
                .lock()
                .expect("recording bus mutex should not be poisoned")
                .push(event_type);
            Ok(())
        }

        async fn subscribe_session(
            &self,
            _session_id: SessionId,
            _replay_start: ReplayStart,
        ) -> Result<EventStream, EventStreamBusError> {
            Err(EventStreamBusError::CursorOverflow)
        }
    }

    #[derive(Default)]
    struct FailingSecondPublishBus {
        publish_count: AtomicUsize,
    }

    #[async_trait]
    impl EventStreamBus for FailingSecondPublishBus {
        async fn publish(&self, _envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
            if self.publish_count.fetch_add(1, Ordering::SeqCst) == 1 {
                return Err(EventStreamBusError::CursorOverflow);
            }
            Ok(())
        }

        async fn subscribe_session(
            &self,
            _session_id: SessionId,
            _replay_start: ReplayStart,
        ) -> Result<EventStream, EventStreamBusError> {
            Err(EventStreamBusError::CursorOverflow)
        }
    }

    #[test]
    fn builder_uses_provider_model() {
        let agent = Agent::builder()
            .id(test_agent_id())
            .name("test-agent")
            .system_prompt("be helpful")
            .llm_provider(Arc::new(MockLlmProvider::new()))
            .tool_registry(Arc::new(BuiltinToolRegistry::default()))
            .event_bus(Arc::new(InMemoryEventStreamBus::default()))
            .store(test_store())
            .build();

        assert!(agent.is_ok());
    }

    #[test]
    fn builder_reports_missing_store() {
        let result = Agent::builder()
            .name("test-agent")
            .system_prompt("be helpful")
            .llm_provider(Arc::new(MockLlmProvider::new()))
            .tool_registry(Arc::new(BuiltinToolRegistry::default()))
            .event_bus(Arc::new(InMemoryEventStreamBus::default()))
            .build();
        let error = match result {
            Ok(_) => panic!("store should be required"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            AgentError::MissingBuilderField { field: "store" }
        ));
    }

    #[tokio::test]
    async fn run_turn_uses_host_session_and_sets_current_turn() {
        let provider = Arc::new(BlockingStartProvider::new());
        let bus = Arc::new(InMemoryEventStreamBus::default());
        let agent = Agent::builder()
            .id(test_agent_id())
            .name("test-agent")
            .system_prompt("be helpful")
            .llm_provider(provider.clone())
            .tool_registry(Arc::new(BuiltinToolRegistry::default()))
            .event_bus(bus)
            .store(test_store())
            .build()
            .expect("agent should build");

        let session_id = SessionId::new();
        let turn_id = agent
            .run_turn(
                AgentRuntimeContext::direct(session_id),
                ChatMessage::user("hello"),
            )
            .await
            .expect("run should start");

        assert_eq!(agent.current_session(), Some(session_id));
        assert_eq!(agent.current_turn(), Some(turn_id));
        agent.stop();
    }

    #[tokio::test]
    async fn run_turn_returns_after_required_preamble_is_published() {
        let bus = Arc::new(RecordingBus::default());
        let agent = test_agent_with_bus(bus.clone());

        agent
            .run_turn(
                AgentRuntimeContext::direct(SessionId::new()),
                ChatMessage::user("hello"),
            )
            .await
            .expect("run starts");

        assert_eq!(bus.agent_event_types(), vec!["started", "message"]);
    }

    #[tokio::test]
    async fn run_turn_releases_active_when_preamble_fails() {
        let agent = test_agent_with_bus(Arc::new(FailingSecondPublishBus::default()));

        assert!(
            agent
                .run_turn(
                    AgentRuntimeContext::direct(SessionId::new()),
                    ChatMessage::user("hello"),
                )
                .await
                .is_err()
        );
        assert!(!agent.active.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn stream_rejects_non_user_message() {
        let agent = test_agent();

        let error = match agent
            .run_turn(
                AgentRuntimeContext::direct(SessionId::new()),
                ChatMessage::assistant("nope"),
            )
            .await
        {
            Ok(_) => panic!("assistant message should be rejected"),
            Err(error) => error,
        };

        assert!(matches!(
            error,
            AgentError::InvalidInputMessageRole {
                role: ChatRole::Assistant
            }
        ));
    }

    struct BlockingStartProvider {
        started_tx: watch::Sender<bool>,
        release: watch::Receiver<bool>,
    }

    impl BlockingStartProvider {
        fn new() -> Self {
            let (started_tx, _started_rx) = watch::channel(false);
            let (_release_tx, release_rx) = watch::channel(false);
            Self {
                started_tx,
                release: release_rx,
            }
        }
    }

    #[async_trait]
    impl LlmProvider for BlockingStartProvider {
        fn model_id(&self) -> ModelId {
            "blocking:mock-model".parse().expect("model id parses")
        }

        async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, LlmError> {
            Err(LlmError::UnsupportedCapability("chat"))
        }

        async fn chat_stream(&self, _request: ChatRequest) -> Result<ChatStream, LlmError> {
            self.started_tx
                .send(true)
                .expect("started signal should send");
            let release = self.release.clone();

            Ok(Box::pin(stream::unfold(
                Some(release),
                |release| async move {
                    let mut release = release?;
                    while !*release.borrow() {
                        if release.changed().await.is_err() {
                            return None;
                        }
                    }
                    Some((
                        Ok(ChatStreamEvent::Finished {
                            finish_reason: FinishReason::Stop,
                            usage: None,
                        }),
                        None,
                    ))
                },
            )))
        }
    }

    #[tokio::test]
    async fn stream_rejects_second_run_while_background_loop_is_active() {
        let (started_tx, started_rx) = watch::channel(false);
        let (release, release_rx) = watch::channel(false);
        let provider = Arc::new(BlockingStartProvider {
            started_tx,
            release: release_rx,
        });
        let agent = Agent::builder()
            .id(test_agent_id())
            .name("test-agent")
            .system_prompt("be helpful")
            .llm_provider(provider.clone())
            .tool_registry(Arc::new(BuiltinToolRegistry::default()))
            .event_bus(Arc::new(InMemoryEventStreamBus::default()))
            .store(test_store())
            .build()
            .expect("agent should build");

        let session_id = SessionId::new();
        let _first_turn_id = agent
            .run_turn(
                AgentRuntimeContext::direct(session_id),
                ChatMessage::user("hello"),
            )
            .await
            .expect("first run should start");

        timeout(Duration::from_secs(1), async {
            let mut started_rx = started_rx.clone();
            while !*started_rx.borrow() {
                started_rx
                    .changed()
                    .await
                    .expect("started signal should remain open");
            }
        })
        .await
        .expect("background loop should start");

        assert_eq!(agent.current_session(), Some(session_id));
        assert!(agent.active.load(Ordering::SeqCst));
        let error = match agent
            .run_turn(
                AgentRuntimeContext::direct(session_id),
                ChatMessage::user("again"),
            )
            .await
        {
            Ok(_) => panic!("second run should be rejected while loop is active"),
            Err(error) => error,
        };
        assert!(matches!(error, AgentError::OperationAlreadyActive));

        release.send(true).expect("release signal should send");
        timeout(Duration::from_secs(1), async {
            let mut events = agent
                .event_bus()
                .subscribe_session(session_id, ReplayStart::All)
                .await
                .expect("event subscription should succeed");
            while let Some(envelope) = events.next().await {
                let StreamEnvelope { event, .. } =
                    envelope.expect("event should be delivered").envelope;
                if matches!(
                    event,
                    stratum_core::RuntimeEvent::Agent {
                        event: stratum_core::AgentEvent::Finished { .. },
                        ..
                    }
                ) {
                    break;
                }
            }
        })
        .await
        .expect("background loop should finish");
        timeout(Duration::from_secs(1), async {
            while agent.active.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("active flag should clear after loop completion");
    }

    #[test]
    fn continuation_error_logs_do_not_include_conversation_content() {
        let source = include_str!("definition.rs");
        let cancelled_debug = ["Err(AgentError::Cancelled) => tracing::", "debug!"].concat();
        assert!(
            source.contains(&cancelled_debug),
            "cancellation must be logged below error level"
        );
        let needle = ["tracing::", "error!("].concat();
        let logs = source.split(&needle).skip(1);
        for log in logs {
            let log = log.split(");").next().expect("error log closes");
            assert!(log.contains("agent_id"));
            assert!(log.contains("session_id"));
            assert!(log.contains("error_kind"));
            for sensitive in [
                "message",
                "prompt",
                "arguments",
                "api_key",
                "path",
                "source",
            ] {
                assert!(
                    !log.contains(sensitive),
                    "continuation error log must not contain {sensitive}"
                );
            }
        }
    }
}
