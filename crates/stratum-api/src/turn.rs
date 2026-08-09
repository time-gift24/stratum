//! Shared turn orchestration: definition decoding, runtime reconstruction,
//! and the managed task wrapper.
//!
//! Every managed Turn runs as one spawned future driving the kernel
//! (`AgentLoop::run` for fresh Turns, `PreparedResume::run` for resume). The
//! wrapper never aborts or drops the kernel future on cancel, signals the
//! admission oneshot when a fresh Turn fails before its first user message
//! commits, logs the terminal failure once with safe fields, and finally
//! removes only its own claim identity.

use std::sync::Arc;

use stratum_agent::{
    AgentLoop, ChainHookRuntime, HookRuntime, LoopContext, LoopLimits, PreparedResume, ToolExecutor,
};
use stratum_core::{
    ChatMessage, ModelConfig, SkillSetVersionId, ToolKind, ToolName, TurnRuntimeSnapshot,
};
use stratum_infra::{DurableEventSink, TelemetryEventSink};
use stratum_llm::LlmProvider;
use stratum_postgres::AgentView;
use stratum_tools::{BuiltinToolRegistry, EchoTool, ToolPermissionMode, ToolRegistry};
use tracing::Instrument;
use uuid::Uuid;

use crate::approval::ApprovalHandler;
use crate::dispatcher::DispatcherHandle;
use crate::error::{ApiError, ErrorKind};
use crate::registry::ClaimHandle;
use crate::sink::{AdmissionSignal, TurnIds};
use crate::state::AppState;

/// Definition schema version this binary writes and reads.
pub(crate) const DEFINITION_SCHEMA_VERSION_V1: i32 = 1;

/// No skill system exists yet; the snapshot pins the nil skill set identity.
const NO_SKILL_SET: Uuid = Uuid::nil();

/// Skill set identity pinned into every runtime snapshot this binary writes;
/// the resume barrier compares the persisted value against it.
pub(crate) fn pinned_skill_set_version() -> SkillSetVersionId {
    SkillSetVersionId::from(NO_SKILL_SET)
}

/// Ordered hook handler version ids of the chain built by
/// [`build_hook_runtime`]; the single source for snapshot writes and the
/// resume barrier's ordered-chain comparison.
pub(crate) fn pinned_hook_handler_versions() -> Vec<stratum_core::HookHandlerVersionId> {
    vec![stratum_core::HookHandlerVersionId::from(
        crate::approval::APPROVAL_HANDLER_VERSION,
    )]
}

/// Immutable resolved definition persisted in `agents.resolved_definition`
/// (definition schema v1).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ResolvedDefinitionV1 {
    /// Agent name.
    pub(crate) agent_name: String,
    /// Creation-time effective model configuration.
    pub(crate) model: ModelConfig,
    /// Ordered tools exposed to the Agent.
    pub(crate) tools: Vec<ToolName>,
    /// System prompt.
    pub(crate) prompt: String,
}

/// Decodes the immutable resolved definition of one Agent view.
///
/// # Errors
///
/// Returns [`ErrorKind::RuntimeIncompatible`] for an unsupported definition
/// schema version and [`ErrorKind::DurableStateCorrupt`] for a malformed v1
/// definition.
pub(crate) fn decode_definition(view: &AgentView) -> Result<ResolvedDefinitionV1, ApiError> {
    if view.definition_schema_version != DEFINITION_SCHEMA_VERSION_V1 {
        return Err(ApiError::new(ErrorKind::RuntimeIncompatible));
    }
    serde_json::from_value(view.resolved_definition.clone())
        .map_err(|source| ApiError::with_source(ErrorKind::DurableStateCorrupt, source))
}

/// Builds the builtin tool registry for one definition.
///
/// The registry policy is deliberately minimal: only the `echo` tool exists
/// and every call requires approval.
///
/// # Errors
///
/// Returns [`ErrorKind::RuntimeUnavailable`] when the definition names a tool
/// this binary does not provide.
pub(crate) fn build_tool_registry(tools: &[ToolName]) -> Result<Arc<dyn ToolRegistry>, ApiError> {
    let mut registry = BuiltinToolRegistry::new(ToolPermissionMode::RequireApproval);
    for name in tools {
        if name.as_str() != "echo" {
            return Err(ApiError::new(ErrorKind::RuntimeUnavailable));
        }
        registry
            .register(
                Arc::new(EchoTool::new()),
                ToolKind::Read,
                stratum_core::DangerLevel::Low,
            )
            .map_err(|source| ApiError::with_source(ErrorKind::Internal, source))?;
    }
    Ok(Arc::new(registry))
}

/// Builds the ordered hook chain of one Turn (currently the approval handler
/// alone).
pub(crate) fn build_hook_runtime(
    state: &AppState,
    ids: TurnIds,
    dispatcher: DispatcherHandle,
) -> Arc<ChainHookRuntime> {
    Arc::new(ChainHookRuntime::new(vec![Arc::new(ApprovalHandler::new(
        state.pg().clone(),
        ids.agent_id,
        ids.session_id,
        ids.turn_id,
        Arc::clone(state.waiters()),
        dispatcher,
        state.approval_poll_interval(),
    ))]))
}

/// Builds the v1 runtime snapshot pinned to a new Turn's `LoopStarted`.
///
/// # Errors
///
/// Returns [`ErrorKind::Internal`] when the tool-set fingerprint cannot be
/// computed or the chain reports no extension set version.
pub(crate) fn runtime_snapshot(
    view: &AgentView,
    model: ModelConfig,
    registry: &Arc<dyn ToolRegistry>,
    hook_runtime: &ChainHookRuntime,
) -> Result<TurnRuntimeSnapshot, ApiError> {
    let fingerprint = registry
        .fingerprint()
        .map_err(|source| ApiError::with_source(ErrorKind::Internal, source))?;
    let extension_set_version_id = hook_runtime
        .extension_set_version()
        .ok_or_else(|| ApiError::new(ErrorKind::Internal))?;
    Ok(TurnRuntimeSnapshot::new(
        view.agent_version_id,
        model,
        fingerprint,
        pinned_skill_set_version(),
        extension_set_version_id,
        pinned_hook_handler_versions(),
    ))
}

/// One spawned Turn execution.
pub(crate) enum TurnRun {
    /// A fresh Turn admitted through `LoopStarted`.
    Fresh {
        /// Kernel loop bound to the per-turn sink.
        agent_loop: AgentLoop,
        /// Committed baseline plus system prompt.
        context: LoopContext,
        /// The new user prompts.
        prompts: Vec<ChatMessage>,
    },
    /// A validated resume continuation.
    Resume(PreparedResume),
}

/// Proof returned after the Turn future has been inserted into the
/// process-owned runtime `JoinSet`. The registry keeps claim/token state only;
/// this marker preserves the existing install-before-defuse call sequence.
pub(crate) struct ManagedTurnTask;

/// Spawns the managed future of one Turn and returns its handle. The future
/// runs inside a `turn.run` span carrying the typed Agent/Session/Turn
/// identity, so kernel and LLM spans nest under the admitting HTTP request
/// in the OTLP trace.
pub(crate) fn spawn_managed_turn(
    state: &Arc<AppState>,
    ids: TurnIds,
    claim: &ClaimHandle,
    run: TurnRun,
    admission: Option<AdmissionSignal>,
) -> ManagedTurnTask {
    let task_state = Arc::clone(state);
    let claim_id = claim.claim_id;
    let token = claim.token.clone();
    let span = tracing::info_span!(
        "turn.run",
        agent_id = %ids.agent_id,
        session_id = %ids.session_id,
        turn_id = %ids.turn_id,
    );
    state.spawn_runtime_task(
        async move {
            let result = match run {
                TurnRun::Fresh {
                    agent_loop,
                    context,
                    prompts,
                } => agent_loop.run(context, prompts, token).await,
                TurnRun::Resume(prepared) => prepared.run(token).await,
            };
            if let Err(error) = result {
                if let Some(signal) = &admission {
                    // A no-op when the sink already signalled the precise error.
                    signal.fail(ApiError::new(loop_error_kind(&error)));
                }
                let kind = loop_error_kind(&error);
                match kind {
                    ErrorKind::StoreUnavailable => {
                        tracing::error!(
                            agent_id = %ids.agent_id,
                            turn_id = %ids.turn_id,
                            error.kind = kind.code(),
                            "managed turn ended with a durability error"
                        );
                    }
                    _ => {
                        tracing::warn!(
                            agent_id = %ids.agent_id,
                            turn_id = %ids.turn_id,
                            error.kind = kind.code(),
                            "managed turn ended"
                        );
                    }
                }
            }
            task_state
                .registry()
                .compare_remove(ids.agent_id, ids.turn_id, claim_id);
        }
        .instrument(span),
    );
    ManagedTurnTask
}

/// Stable classification of a kernel failure, used for logs and as the
/// admission-signal fallback. The kernel error text stays out of the HTTP
/// surface.
fn loop_error_kind(error: &stratum_agent::AgentLoopError) -> ErrorKind {
    use stratum_agent::AgentLoopError as LoopError;
    match error {
        LoopError::Durability { .. } | LoopError::TerminalDurability { .. } => {
            ErrorKind::StoreUnavailable
        }
        LoopError::Resume { .. } => ErrorKind::DurableStateCorrupt,
        _ => ErrorKind::Internal,
    }
}

/// Builds the kernel loop of one Turn.
///
/// # Errors
///
/// Returns [`ErrorKind::Internal`] when required loop fields are missing
/// (they are always supplied here).
pub(crate) fn build_agent_loop(
    provider: Arc<dyn LlmProvider>,
    registry: Arc<dyn ToolRegistry>,
    durable_sink: Arc<dyn DurableEventSink>,
    hook_runtime: Arc<ChainHookRuntime>,
    telemetry: Arc<dyn TelemetryEventSink>,
) -> Result<AgentLoop, ApiError> {
    AgentLoop::builder()
        .llm_provider(provider)
        .tool_executor(ToolExecutor::new(registry, durable_sink))
        .hook_runtime(hook_runtime)
        .telemetry(telemetry)
        .limits(LoopLimits::default())
        .build()
        .map_err(|_| ApiError::new(ErrorKind::Internal))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolved_definition_round_trips_and_rejects_unknown_fields() {
        let definition = ResolvedDefinitionV1 {
            agent_name: "agent".to_owned(),
            model: ModelConfig::new(
                stratum_core::ModelId::new("openai", "test-model").expect("model id is valid"),
                serde_json::Map::new(),
            ),
            tools: vec![ToolName::from("echo")],
            prompt: "be helpful".to_owned(),
        };
        let value = serde_json::to_value(&definition).expect("definition serializes");
        let decoded: ResolvedDefinitionV1 =
            serde_json::from_value(value).expect("definition decodes");
        assert_eq!(decoded, definition);

        let mut with_extra = serde_json::to_value(&definition).expect("definition serializes");
        with_extra["template_path"] = serde_json::json!("/etc/passwd");
        assert!(serde_json::from_value::<ResolvedDefinitionV1>(with_extra).is_err());
    }

    #[test]
    fn tool_registry_only_provides_echo_with_required_approval() {
        let registry = build_tool_registry(&[ToolName::from("echo")]).expect("registry builds");
        assert!(
            registry
                .authorization(&ToolName::from("echo"))
                .expect("echo is registered")
                .is_some(),
            "echo requires approval"
        );
        assert!(build_tool_registry(&[ToolName::from("write_file")]).is_err());
    }
}
