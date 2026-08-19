//! Turn lifecycle command handlers: message admission, resume, cancel, and
//! approval resolve.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use stratum_agent::{HookRuntime, LoopContext};
use stratum_core::{
    AgentId, AgentRuntimeId, ApprovalId, ChatMessage, DurableAgentEvent, SessionId, TokenUsage,
    TurnId,
};
use stratum_postgres::{
    AgentRuntimeView, AppendEvent, ResolveApproval, ResolveApprovalOutcome, ResumeSliceQuery,
};
use tracing::{Span, field};

use super::{json_request, parse_agent_runtime_id};
use crate::baseline::materialize_baseline;
use crate::dto::{
    ApprovalResolveRequest, CancelRequest, MessageRequest, ResumeRequest, TurnAccepted,
};
use crate::error::{ApiError, ErrorKind, ErrorResponse};
use crate::registry::{ClaimHandle, ClaimOutcome};
use crate::sink::{AdmissionSignal, TurnDurableSink, TurnIds, TurnTelemetrySink};
use crate::state::AppState;
use crate::turn::{
    TurnRun, build_agent_loop, build_hook_runtime, build_tool_registry, decode_definition,
    pinned_hook_handler_versions, pinned_skill_set_version, runtime_snapshot, spawn_managed_turn,
};

/// RAII cleanup for a claim installed by one request: disarmed once the
/// managed task owns the claim, otherwise only this exact claim identity is
/// removed.
struct ClaimCleanup<'a> {
    state: &'a AppState,
    agent_runtime_id: AgentRuntimeId,
    turn_id: TurnId,
    claim_id: uuid::Uuid,
    armed: bool,
}

impl ClaimCleanup<'_> {
    fn defuse(mut self) {
        self.armed = false;
    }
}

impl Drop for ClaimCleanup<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.state.registry().compare_remove(
                self.agent_runtime_id,
                self.turn_id,
                self.claim_id,
            );
        }
    }
}

/// Admits a new Turn with an exact current-Turn CAS.
#[utoipa::path(
    post,
    path = "/v1/agent-runtimes/{agent_runtime_id}/messages",
    params(("agent_runtime_id" = String, Path, description = "agent runtime identity")),
    request_body = MessageRequest,
    responses(
        (status = 202, description = "turn admitted; loop started and first user message committed", body = TurnAccepted),
        (status = 400, description = "invalid request body or runtime identity", body = ErrorResponse),
        (status = 404, description = "agent runtime not found", body = ErrorResponse),
        (status = 409, description = "stale turn, busy runtime, resume required, or session conflict", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "model is not configured or parameters are invalid", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt or an internal error occurred", body = ErrorResponse),
        (status = 503, description = "store or runtime unavailable, or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn post_message(
    State(state): State<Arc<AppState>>,
    Path(agent_runtime_id): Path<String>,
    request: Result<Json<MessageRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let agent_runtime_id = parse_agent_runtime_id(&agent_runtime_id)?;
    Span::current().record("agent_runtime_id", field::display(agent_runtime_id));
    let body = json_request(request)?;
    let accepted = admit_message(&state, agent_runtime_id, body).await?;
    Ok((StatusCode::ACCEPTED, Json(accepted)).into_response())
}

/// Admits one first or follow-up user message through the same path used by
/// the HTTP command and local scheduler.
///
/// The caller owns process admission; this function owns the exact Turn claim
/// and returns only after the first user message is durably committed.
///
/// # Errors
///
/// Returns a typed API error for stale/busy runtime state, invalid runtime
/// components, or durable admission failure.
pub(crate) async fn admit_message(
    state: &Arc<AppState>,
    agent_runtime_id: AgentRuntimeId,
    body: MessageRequest,
) -> Result<TurnAccepted, ApiError> {
    if body.text.trim().is_empty() {
        return Err(ApiError::new(ErrorKind::InvalidRequest));
    }

    let view = state
        .pg()
        .read_agent_runtime_view(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    Span::current().record("agent_id", field::display(view.agent_id));
    let definition = decode_definition(&view)?;

    // Durable state pre-checks; the admission CAS stays authoritative. An
    // expected-turn mismatch is stale regardless of status: a lost-response
    // retry with an old expectation must never create a second Turn, while
    // busy/resume_required only apply to requests carrying the correct
    // current expectation.
    if body.expected_current_turn_id != view.current_turn_id {
        return Err(ApiError::new(ErrorKind::StaleTurn));
    }
    match view.status {
        stratum_postgres::AgentStatus::Running => {
            let current = view
                .current_turn_id
                .ok_or_else(|| ApiError::new(ErrorKind::DurableStateCorrupt))?;
            return Err(
                match state.registry().claim_state(agent_runtime_id, current) {
                    Some(_) => ApiError::new(ErrorKind::AgentRuntimeBusy),
                    None => ApiError::new(ErrorKind::ResumeRequired),
                },
            );
        }
        stratum_postgres::AgentStatus::Idle
        | stratum_postgres::AgentStatus::Finished
        | stratum_postgres::AgentStatus::Failed
        | stratum_postgres::AgentStatus::Cancelled => {}
        _ => return Err(ApiError::new(ErrorKind::Internal)),
    }

    let session_id = match (view.session_id, body.session_id) {
        (Some(bound), Some(requested)) if bound != requested => {
            return Err(ApiError::new(ErrorKind::SessionMismatch));
        }
        (Some(bound), _) => bound,
        (None, requested) => requested.unwrap_or_else(SessionId::new),
    };

    // Model, provider, and tool preflight before any durable mutation.
    let effective_model = match &body.model_config {
        Some(overridden) => {
            super::agents::validate_model_override(&state, overridden).await?;
            overridden.clone()
        }
        None => view.model_config.clone(),
    };
    let providers = state
        .providers()
        .await
        .map_err(|source| ApiError::with_source(ErrorKind::RuntimeUnavailable, source))?;
    let provider = providers
        .configure(&effective_model)
        .map_err(|source| ApiError::with_source(ErrorKind::RuntimeUnavailable, source))?;
    let registry = build_tool_registry(&definition.tools)?;

    let turn_id = TurnId::new();
    let ids = TurnIds {
        agent_runtime_id,
        agent_id: view.agent_id,
        session_id,
        turn_id,
    };
    let claim = match state.registry().try_claim(agent_runtime_id, turn_id) {
        ClaimOutcome::Claimed(claim) => claim,
        // A fresh TurnId can never collide; this is an internal invariant.
        ClaimOutcome::Exists => return Err(ApiError::new(ErrorKind::Internal)),
    };
    let cleanup = ClaimCleanup {
        state: &state,
        agent_runtime_id,
        turn_id,
        claim_id: claim.claim_id,
        armed: true,
    };

    let baseline =
        materialize_baseline(state.pg(), agent_runtime_id, view.snapshot_event_seq).await?;
    let dispatcher = state
        .dispatchers()
        .ensure(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    let hook_runtime = build_hook_runtime(&state, ids, dispatcher.clone());
    let snapshot = runtime_snapshot(&view, effective_model.clone(), &registry, &hook_runtime)?;

    let (signal, admission_result) = AdmissionSignal::new();
    let sink = TurnDurableSink::fresh(
        state.pg().clone(),
        ids,
        crate::sink::FreshTurnAdmission {
            expected_current_turn_id: body.expected_current_turn_id,
            snapshot,
            model_config_update: (effective_model != view.model_config)
                .then_some(effective_model.clone()),
            signal: signal.clone(),
        },
        baseline.lineage.clone(),
        dispatcher.clone(),
    );
    let telemetry = TurnTelemetrySink::new(ids, dispatcher);
    let agent_loop = build_agent_loop(
        provider,
        registry,
        Arc::new(sink),
        hook_runtime,
        Arc::new(telemetry),
    )?;

    let handle = spawn_managed_turn(
        &state,
        ids,
        &claim,
        TurnRun::Fresh {
            agent_loop,
            context: LoopContext::new(definition.prompt.clone()).with_messages(baseline.messages),
            prompts: vec![ChatMessage::user(body.text)],
        },
        Some(signal),
    );
    state
        .registry()
        .attach_task(agent_runtime_id, turn_id, claim.claim_id, handle);
    // From here the process-owned JoinSet owns the managed task, while the
    // task removes only its exact claim identity when it ends. Returning from
    // this request cannot detach the task from shutdown drain ownership.
    cleanup.defuse();

    // Accepted only after the managed future is installed and the first user
    // message is committed.
    let shutdown = state.shutdown_token();
    let outcome = tokio::select! {
        () = shutdown.cancelled() => Err(ApiError::new(ErrorKind::ServiceShuttingDown)),
        outcome = admission_result => match outcome {
            Ok(outcome) => outcome,
            Err(_) => Err(ApiError::new(ErrorKind::Internal)),
        },
    };
    outcome?;
    state
        .registry()
        .mark_running(agent_runtime_id, turn_id, claim.claim_id);
    Span::current().record("session_id", field::display(session_id));
    Span::current().record("turn_id", field::display(turn_id));
    Ok(TurnAccepted {
        agent_runtime_id,
        agent_id: view.agent_id,
        session_id,
        turn_id,
    })
}

/// Takes over an exact unhosted running Turn.
#[utoipa::path(
    post,
    path = "/v1/agent-runtimes/{agent_runtime_id}/resume",
    params(("agent_runtime_id" = String, Path, description = "agent runtime identity")),
    request_body = ResumeRequest,
    responses(
        (status = 202, description = "turn resumed under this process", body = TurnAccepted),
        (status = 204, description = "the exact turn is already starting or running here", body = ()),
        (status = 400, description = "invalid request body or runtime identity", body = ErrorResponse),
        (status = 404, description = "agent runtime not found", body = ErrorResponse),
        (status = 409, description = "stale turn, not running, preamble incomplete, or incompatible runtime", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt", body = ErrorResponse),
        (status = 503, description = "store or runtime unavailable, or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn post_resume(
    State(state): State<Arc<AppState>>,
    Path(agent_runtime_id): Path<String>,
    request: Result<Json<ResumeRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let agent_runtime_id = parse_agent_runtime_id(&agent_runtime_id)?;
    Span::current().record("agent_runtime_id", field::display(agent_runtime_id));
    let body = json_request(request)?;

    let view = state
        .pg()
        .read_agent_runtime_view(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    Span::current().record("agent_id", field::display(view.agent_id));
    if view.current_turn_id != Some(body.turn_id) {
        return Err(ApiError::new(ErrorKind::StaleTurn));
    }
    if view.status != stratum_postgres::AgentStatus::Running {
        return Err(ApiError::new(ErrorKind::TurnNotRunning));
    }
    if state
        .registry()
        .claim_state(agent_runtime_id, body.turn_id)
        .is_some()
    {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let claim = match state.registry().try_claim(agent_runtime_id, body.turn_id) {
        ClaimOutcome::Claimed(claim) => claim,
        ClaimOutcome::Exists => return Ok(StatusCode::NO_CONTENT.into_response()),
    };
    let cleanup = ClaimCleanup {
        state: &state,
        agent_runtime_id,
        turn_id: body.turn_id,
        claim_id: claim.claim_id,
        armed: true,
    };
    let response = resume_preflight_and_spawn(&state, &view, body.turn_id, &claim).await?;
    // Success keeps the claim; any preflight failure drops the guard and
    // releases only this exact claim, leaving the Turn running/unhosted.
    cleanup.defuse();
    Ok(response)
}

/// Fixed-barrier resume preflight and spawn; every failure leaves the Turn
/// durable `running` and unhosted.
async fn resume_preflight_and_spawn(
    state: &Arc<AppState>,
    view: &AgentRuntimeView,
    turn_id: TurnId,
    claim: &ClaimHandle,
) -> Result<Response, ApiError> {
    let agent_runtime_id = view.agent_runtime_id;
    let through = view.snapshot_event_seq;
    let started = state
        .pg()
        .read_loop_started(agent_runtime_id, turn_id)
        .await
        .map_err(ApiError::from_postgres)?;
    let base = started
        .event_seq
        .checked_sub(1)
        .ok_or_else(|| ApiError::new(ErrorKind::DurableStateCorrupt))?;
    let slice = state
        .pg()
        .read_resume_slice(ResumeSliceQuery {
            agent_runtime_id,
            session_id: started.session_id,
            turn_id,
            base_event_seq: base,
            through_event_seq: through,
        })
        .await
        .map_err(ApiError::from_postgres)?;
    if slice.iter().any(|row| {
        matches!(
            row.event,
            DurableAgentEvent::LoopFinished { .. }
                | DurableAgentEvent::LoopFailed { .. }
                | DurableAgentEvent::LoopCancelled { .. }
        )
    }) {
        return Err(ApiError::new(ErrorKind::TurnNotRunning));
    }

    // Started-only reconciliation: atomically fail the Turn, then report the
    // incomplete preamble.
    if slice.len() == 1 {
        // This branch is itself a durable writer, so establish its generation
        // immediately before the reconciliation transaction.
        let dispatcher = state
            .dispatchers()
            .ensure(agent_runtime_id)
            .await
            .map_err(ApiError::from_postgres)?;
        match reconcile_started_only(
            state,
            &dispatcher,
            agent_runtime_id,
            view.agent_id,
            started.session_id,
            turn_id,
        )
        .await
        {
            Err(error) => return Err(error),
            Ok(never) => match never {},
        }
    }

    let baseline = materialize_baseline(state.pg(), agent_runtime_id, base).await?;
    let definition = decode_definition(view)?;
    let snapshot = started.snapshot;
    if snapshot.agent_id != view.agent_id {
        return Err(ApiError::new(ErrorKind::DurableStateCorrupt));
    }

    // Reconstruct the exact runtime pinned by the snapshot.
    let providers = state
        .providers()
        .await
        .map_err(|source| ApiError::with_source(ErrorKind::RuntimeUnavailable, source))?;
    let provider = providers
        .configure(&snapshot.model)
        .map_err(|source| ApiError::with_source(ErrorKind::RuntimeUnavailable, source))?;
    let registry = build_tool_registry(&definition.tools)?;
    let fingerprint = registry
        .fingerprint()
        .map_err(|source| ApiError::with_source(ErrorKind::Internal, source))?;
    if fingerprint != snapshot.tool_set_fingerprint {
        return Err(ApiError::new(ErrorKind::RuntimeUnavailable));
    }

    // The six-field snapshot also pins the skill set identity and the ordered
    // hook handler versions; both must match the rebuilt runtime exactly or
    // the pinned runtime component is unavailable to this binary.
    if snapshot.skill_set_version_id != pinned_skill_set_version()
        || snapshot.hook_handler_versions != pinned_hook_handler_versions()
    {
        return Err(ApiError::new(ErrorKind::RuntimeUnavailable));
    }

    // The resumed sink's lineage mirrors the rebuilt kernel context: the
    // historical baseline plus every current-Turn commit up to the barrier.
    let mut lineage = baseline.lineage.clone();
    for row in &slice[1..] {
        match &row.event {
            DurableAgentEvent::MessageAppended { .. } => lineage.record_message(row.event_seq),
            DurableAgentEvent::TranscriptCompacted { upto, .. } => lineage.apply_compaction(*upto),
            DurableAgentEvent::ToolApprovalRequested { .. }
            | DurableAgentEvent::ToolApprovalResolved { .. }
            | DurableAgentEvent::ToolExecutionStarted { .. }
            | DurableAgentEvent::HookInvocationPending { .. }
            | DurableAgentEvent::HookInvocationCompleted { .. }
            | DurableAgentEvent::HookInvocationFailed { .. }
            | DurableAgentEvent::IterationCompleted { .. } => {}
            DurableAgentEvent::LoopStarted { .. }
            | DurableAgentEvent::LoopFinished { .. }
            | DurableAgentEvent::LoopFailed { .. }
            | DurableAgentEvent::LoopCancelled { .. } => {
                return Err(ApiError::new(ErrorKind::DurableStateCorrupt));
            }
            _ => return Err(ApiError::new(ErrorKind::DurableStateCorrupt)),
        }
    }

    // Replay window: the current LoopStarted, the historical baseline as
    // message events, then the exact current-Turn suffix in event order.
    let window_capacity = slice
        .len()
        .checked_add(baseline.messages.len())
        .ok_or_else(|| ApiError::new(ErrorKind::Internal))?;
    let mut window = Vec::with_capacity(window_capacity);
    window.push(slice[0].event.clone());
    window.extend(
        baseline
            .messages
            .iter()
            .cloned()
            .map(|message| DurableAgentEvent::MessageAppended { message }),
    );
    window.extend(slice[1..].iter().map(|row| row.event.clone()));

    // Durable reads, definition/provider/tool validation, lineage reduction,
    // and replay-window construction have now succeeded. Establish the live
    // dispatcher generation before the API-owned sinks are bound; pure kernel
    // replay validation follows and any failure drops this handle without a
    // durable write or external action.
    let dispatcher = state
        .dispatchers()
        .ensure(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    let ids = TurnIds {
        agent_runtime_id,
        agent_id: view.agent_id,
        session_id: started.session_id,
        turn_id,
    };
    let hook_runtime = build_hook_runtime(state, ids, dispatcher.clone());
    if hook_runtime.extension_set_version() != Some(snapshot.extension_set_version_id) {
        return Err(ApiError::new(ErrorKind::RuntimeUnavailable));
    }
    let sink = TurnDurableSink::resumed(state.pg().clone(), ids, lineage, dispatcher.clone());
    let telemetry = TurnTelemetrySink::new(ids, dispatcher);
    let agent_loop = Arc::new(build_agent_loop(
        provider,
        registry,
        Arc::new(sink),
        hook_runtime,
        Arc::new(telemetry),
    )?);

    let prepared = agent_loop
        .prepare_resume(definition.prompt.clone(), window)
        .map_err(|error| match error {
            stratum_agent::ResumeError::ExtensionSetVersionMismatch { .. } => {
                ApiError::new(ErrorKind::RuntimeUnavailable)
            }
            other => ApiError::with_source(ErrorKind::DurableStateCorrupt, other),
        })?;

    // The fixed replay work above is intentionally lock-free. Immediately
    // before installing the managed task, take the short state-row lock and
    // revalidate the exact runtime/Session/Turn/running fence.
    state
        .pg()
        .revalidate_resume(agent_runtime_id, view.agent_id, started.session_id, turn_id)
        .await
        .map_err(ApiError::from_postgres)?;

    let handle = spawn_managed_turn(state, ids, claim, TurnRun::Resume(prepared), None);
    state
        .registry()
        .attach_task(agent_runtime_id, turn_id, claim.claim_id, handle);
    state
        .registry()
        .mark_running(agent_runtime_id, turn_id, claim.claim_id);
    Ok((
        StatusCode::ACCEPTED,
        Json(TurnAccepted {
            agent_runtime_id,
            agent_id: view.agent_id,
            session_id: started.session_id,
            turn_id,
        }),
    )
        .into_response())
}

/// Started-only reconciliation: append the unique safe `LoopFailed`; on an
/// uncertain commit, re-read the exact Turn before answering.
async fn reconcile_started_only(
    state: &AppState,
    dispatcher: &crate::dispatcher::DispatcherHandle,
    agent_runtime_id: AgentRuntimeId,
    agent_id: AgentId,
    session_id: SessionId,
    turn_id: TurnId,
) -> Result<std::convert::Infallible, ApiError> {
    let result = state
        .pg()
        .append_event(AppendEvent {
            agent_runtime_id,
            agent_id,
            session_id,
            turn_id,
            event: DurableAgentEvent::LoopFailed {
                error_text: "turn preamble incomplete".to_owned(),
                usage: TokenUsage::default(),
            },
            approval_hook_invocation_id: None,
            model_config_update: None,
            compaction: None,
        })
        .await;
    match result {
        Ok(receipt) => {
            dispatcher.receipt(receipt.event_seq);
            Err(ApiError::new(ErrorKind::TurnPreambleIncomplete))
        }
        Err(append_error) => {
            let reread = state
                .pg()
                .read_agent_runtime_state(agent_runtime_id)
                .await
                .map_err(ApiError::from_postgres)?;
            if reread.current_turn_id != Some(turn_id) {
                return Err(ApiError::new(ErrorKind::StaleTurn));
            }
            match reread.status {
                // Our reconcile append (or a concurrent identical one) landed.
                stratum_postgres::AgentStatus::Failed => {
                    Err(ApiError::new(ErrorKind::TurnPreambleIncomplete))
                }
                stratum_postgres::AgentStatus::Running => {
                    Err(ApiError::from_postgres(append_error))
                }
                stratum_postgres::AgentStatus::Finished
                | stratum_postgres::AgentStatus::Cancelled => {
                    Err(ApiError::new(ErrorKind::TurnNotRunning))
                }
                _ => Err(ApiError::new(ErrorKind::Internal)),
            }
        }
    }
}

/// Signals the in-memory cancellation token of an exact hosted Turn.
#[utoipa::path(
    post,
    path = "/v1/agent-runtimes/{agent_runtime_id}/cancel",
    params(("agent_runtime_id" = String, Path, description = "agent runtime identity")),
    request_body = CancelRequest,
    responses(
        (status = 202, description = "cancellation signal accepted by the hosted turn", body = ()),
        (status = 204, description = "the exact turn is already cancelled", body = ()),
        (status = 400, description = "invalid request body or runtime identity", body = ErrorResponse),
        (status = 404, description = "agent runtime not found", body = ErrorResponse),
        (status = 409, description = "stale turn, turn not running/hosted, or turn still starting", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 503, description = "store unavailable or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn post_cancel(
    State(state): State<Arc<AppState>>,
    Path(agent_runtime_id): Path<String>,
    request: Result<Json<CancelRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let agent_runtime_id = parse_agent_runtime_id(&agent_runtime_id)?;
    Span::current().record("agent_runtime_id", field::display(agent_runtime_id));
    let body = json_request(request)?;

    let runtime_state = state
        .pg()
        .read_agent_runtime_state(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    if runtime_state.current_turn_id != Some(body.turn_id) {
        return Err(ApiError::new(ErrorKind::StaleTurn));
    }
    match runtime_state.status {
        stratum_postgres::AgentStatus::Cancelled => Ok(StatusCode::NO_CONTENT.into_response()),
        stratum_postgres::AgentStatus::Finished | stratum_postgres::AgentStatus::Failed => {
            Err(ApiError::new(ErrorKind::TurnNotRunning))
        }
        stratum_postgres::AgentStatus::Running => {
            match state.registry().claim_state(agent_runtime_id, body.turn_id) {
                Some(crate::registry::ClaimState::Starting) => {
                    Err(ApiError::new(ErrorKind::TurnStarting))
                }
                Some(crate::registry::ClaimState::Running) => {
                    let token = state
                        .registry()
                        .running_token(agent_runtime_id, body.turn_id)
                        .ok_or_else(|| ApiError::new(ErrorKind::Internal))?;
                    // In-memory signal only: no durable intent, no abort.
                    token.cancel();
                    Ok(StatusCode::ACCEPTED.into_response())
                }
                None => Err(ApiError::new(ErrorKind::TurnNotHosted)),
            }
        }
        _ => Err(ApiError::new(ErrorKind::Internal)),
    }
}

/// Resolves one durable approval request; resolve never resumes implicitly.
#[utoipa::path(
    post,
    path = "/v1/agent-runtimes/{agent_runtime_id}/approvals/{approval_id}",
    params(
        ("agent_runtime_id" = String, Path, description = "agent runtime identity"),
        ("approval_id" = String, Path, description = "approval request identity"),
    ),
    request_body = ApprovalResolveRequest,
    responses(
        (status = 204, description = "decision committed (or an identical decision already exists)", body = ()),
        (status = 400, description = "invalid request body or path identity", body = ErrorResponse),
        (status = 404, description = "agent runtime or approval not found", body = ErrorResponse),
        (status = 409, description = "stale turn, opposite decision exists, or approval invalidated", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn post_approval(
    State(state): State<Arc<AppState>>,
    Path((agent_runtime_id, approval_id)): Path<(String, String)>,
    request: Result<Json<ApprovalResolveRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let agent_runtime_id = parse_agent_runtime_id(&agent_runtime_id)?;
    let approval_uuid = uuid::Uuid::parse_str(&approval_id)
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let approval_id = ApprovalId::from(approval_uuid);
    Span::current().record("agent_runtime_id", field::display(agent_runtime_id));
    let body = json_request(request)?;

    let runtime = state
        .pg()
        .read_agent_runtime_state(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;
    // For the exact current Turn, derive the expected immutable definition
    // from its durable LoopStarted snapshot rather than trusting the mutable
    // state row alone. A stale Turn still reaches the resolver so it preserves
    // the stable stale_turn classification.
    let expected_agent_id = if runtime.current_turn_id == Some(body.turn_id) {
        state
            .pg()
            .read_loop_started(agent_runtime_id, body.turn_id)
            .await
            .map_err(ApiError::from_postgres)?
            .snapshot
            .agent_id
    } else {
        runtime.agent_id
    };

    // The dispatcher must exist at a pre-commit PG barrier. Creating it from a
    // later receipt would let two concurrently committed writers whose wakeups
    // arrive in reverse order skip the earlier row as historical.
    let dispatcher = state
        .dispatchers()
        .ensure(agent_runtime_id)
        .await
        .map_err(ApiError::from_postgres)?;

    let outcome = state
        .pg()
        .resolve_approval(ResolveApproval {
            agent_runtime_id,
            agent_id: expected_agent_id,
            approval_id,
            turn_id: body.turn_id,
            decision: body.decision,
        })
        .await
        .map_err(ApiError::from_postgres)?;
    if let ResolveApprovalOutcome::Resolved { receipt } = outcome {
        // Commit first, then best-effort notification and realtime receipt.
        state.waiters().notify(agent_runtime_id, approval_id);
        dispatcher.receipt(receipt.event_seq);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    fn assert_response_has_explicit_unit_body(
        operation: utoipa::openapi::path::Operation,
        status: &str,
    ) {
        let document = serde_json::to_value(operation).expect("OpenAPI operation serializes");
        assert_eq!(
            document.pointer(&format!(
                "/responses/{status}/content/application~1json/schema/default"
            )),
            Some(&serde_json::Value::Null),
            "response {status} must declare the explicit unit body: {document}"
        );
    }

    #[test]
    fn empty_success_responses_declare_explicit_unit_bodies() {
        assert_response_has_explicit_unit_body(
            <super::__path_post_resume as utoipa::Path>::operation(),
            "204",
        );
        let cancel = <super::__path_post_cancel as utoipa::Path>::operation();
        assert_response_has_explicit_unit_body(cancel.clone(), "202");
        assert_response_has_explicit_unit_body(cancel, "204");
        assert_response_has_explicit_unit_body(
            <super::__path_post_approval as utoipa::Path>::operation(),
            "204",
        );
    }
}
