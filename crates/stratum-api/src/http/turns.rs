//! Turn lifecycle command handlers: message admission, resume, cancel, and
//! approval resolve.

use std::sync::Arc;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use stratum_agent::{HookRuntime, LoopContext};
use stratum_core::{ApprovalId, ChatMessage, DurableAgentEvent, SessionId, TokenUsage, TurnId};
use stratum_postgres::{
    AgentView, AppendEvent, ResolveApproval, ResolveApprovalOutcome, ResumeSliceQuery,
};
use tracing::{Span, field};

use super::{json_request, parse_agent_id};
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
    runtime_snapshot, spawn_managed_turn,
};

/// RAII cleanup for a claim installed by one request: disarmed on success,
/// otherwise only this exact claim identity is removed.
struct ClaimCleanup<'a> {
    state: &'a AppState,
    agent_id: stratum_core::AgentId,
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
            self.state
                .registry()
                .compare_remove(self.agent_id, self.turn_id, self.claim_id);
        }
    }
}

/// Admits a new Turn with an exact current-Turn CAS.
#[utoipa::path(
    post,
    path = "/v1/agents/{agent_id}/messages",
    params(("agent_id" = String, Path, description = "agent identity")),
    request_body = MessageRequest,
    responses(
        (status = 202, description = "turn admitted; loop started and first user message committed", body = TurnAccepted),
        (status = 400, description = "invalid request body or agent identity", body = ErrorResponse),
        (status = 404, description = "agent not found", body = ErrorResponse),
        (status = 409, description = "stale turn, busy agent, resume required, or session conflict", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 422, description = "model is not configured or parameters are invalid", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt or an internal error occurred", body = ErrorResponse),
        (status = 503, description = "store or runtime unavailable, or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn post_message(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
    request: Result<Json<MessageRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let agent_id = parse_agent_id(&agent_id)?;
    Span::current().record("agent_id", field::display(agent_id));
    let body = json_request(request)?;
    if body.text.trim().is_empty() {
        return Err(ApiError::new(ErrorKind::InvalidRequest));
    }

    let view = state
        .pg()
        .read_agent_view(agent_id)
        .await
        .map_err(ApiError::from_postgres)?;
    let definition = decode_definition(&view)?;

    // Durable state pre-checks; the admission CAS stays authoritative.
    match view.status {
        stratum_postgres::AgentStatus::Running => {
            let current = view
                .current_turn_id
                .ok_or_else(|| ApiError::new(ErrorKind::DurableStateCorrupt))?;
            return Err(match state.registry().claim_state(agent_id, current) {
                Some(_) => ApiError::new(ErrorKind::AgentBusy),
                None => ApiError::new(ErrorKind::ResumeRequired),
            });
        }
        stratum_postgres::AgentStatus::Idle
        | stratum_postgres::AgentStatus::Finished
        | stratum_postgres::AgentStatus::Failed
        | stratum_postgres::AgentStatus::Cancelled => {
            if body.expected_current_turn_id != view.current_turn_id {
                return Err(ApiError::new(ErrorKind::StaleTurn));
            }
        }
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
            super::agents::validate_model_override(&state, overridden)?;
            overridden.clone()
        }
        None => view.default_model_config.clone(),
    };
    let provider = state
        .providers()
        .configure(&effective_model)
        .map_err(|_| ApiError::new(ErrorKind::RuntimeUnavailable))?;
    let registry = build_tool_registry(&definition.tools)?;

    let turn_id = TurnId::new();
    let ids = TurnIds {
        agent_id,
        session_id,
        turn_id,
    };
    let claim = match state.registry().try_claim(agent_id, turn_id) {
        ClaimOutcome::Claimed(claim) => claim,
        // A fresh TurnId can never collide; this is an internal invariant.
        ClaimOutcome::Exists => return Err(ApiError::new(ErrorKind::Internal)),
    };
    let cleanup = ClaimCleanup {
        state: &state,
        agent_id,
        turn_id,
        claim_id: claim.claim_id,
        armed: true,
    };

    let baseline = materialize_baseline(state.pg(), agent_id, view.snapshot_event_seq).await?;
    let dispatcher = state
        .dispatchers()
        .ensure(agent_id, view.snapshot_event_seq);
    let hook_runtime = build_hook_runtime(&state, ids, dispatcher.clone());
    let snapshot = runtime_snapshot(&view, effective_model.clone(), &registry, &hook_runtime)?;

    let (signal, admission_result) = AdmissionSignal::new();
    let sink = TurnDurableSink::fresh(
        state.pg().clone(),
        ids,
        crate::sink::FreshTurnAdmission {
            expected_current_turn_id: body.expected_current_turn_id,
            snapshot,
            effective_model,
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
        .attach_task(agent_id, turn_id, claim.claim_id, handle);

    // Accepted only after the managed future is installed and the first user
    // message is committed.
    let shutdown = state.shutdown_token();
    let outcome = tokio::select! {
        () = shutdown.cancelled() => Err(ApiError::new(ErrorKind::ServiceUnavailable)),
        outcome = admission_result => match outcome {
            Ok(outcome) => outcome,
            Err(_) => Err(ApiError::new(ErrorKind::Internal)),
        },
    };
    outcome?;
    state
        .registry()
        .mark_running(agent_id, turn_id, claim.claim_id);
    cleanup.defuse();
    Span::current().record("session_id", field::display(session_id));
    Span::current().record("turn_id", field::display(turn_id));
    Ok((
        StatusCode::ACCEPTED,
        Json(TurnAccepted {
            agent_id,
            session_id,
            turn_id,
        }),
    )
        .into_response())
}

/// Takes over an exact unhosted running Turn.
#[utoipa::path(
    post,
    path = "/v1/agents/{agent_id}/resume",
    params(("agent_id" = String, Path, description = "agent identity")),
    request_body = ResumeRequest,
    responses(
        (status = 202, description = "turn resumed under this process", body = TurnAccepted),
        (status = 204, description = "the exact turn is already starting or running here"),
        (status = 400, description = "invalid request body or agent identity", body = ErrorResponse),
        (status = 404, description = "agent not found", body = ErrorResponse),
        (status = 409, description = "stale turn, not running, preamble incomplete, or incompatible runtime", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt", body = ErrorResponse),
        (status = 503, description = "store or runtime unavailable, or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn post_resume(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
    request: Result<Json<ResumeRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let agent_id = parse_agent_id(&agent_id)?;
    Span::current().record("agent_id", field::display(agent_id));
    let body = json_request(request)?;

    let view = state
        .pg()
        .read_agent_view(agent_id)
        .await
        .map_err(ApiError::from_postgres)?;
    if view.current_turn_id != Some(body.turn_id) {
        return Err(ApiError::new(ErrorKind::StaleTurn));
    }
    if view.status != stratum_postgres::AgentStatus::Running {
        return Err(ApiError::new(ErrorKind::TurnNotRunning));
    }
    if state
        .registry()
        .claim_state(agent_id, body.turn_id)
        .is_some()
    {
        return Ok(StatusCode::NO_CONTENT.into_response());
    }
    let claim = match state.registry().try_claim(agent_id, body.turn_id) {
        ClaimOutcome::Claimed(claim) => claim,
        ClaimOutcome::Exists => return Ok(StatusCode::NO_CONTENT.into_response()),
    };
    let cleanup = ClaimCleanup {
        state: &state,
        agent_id,
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
    view: &AgentView,
    turn_id: TurnId,
    claim: &ClaimHandle,
) -> Result<Response, ApiError> {
    let agent_id = view.agent_id;
    let through = view.snapshot_event_seq;
    let started = state
        .pg()
        .read_loop_started(agent_id, turn_id)
        .await
        .map_err(ApiError::from_postgres)?;
    let base = started.event_seq.saturating_sub(1);
    let slice = state
        .pg()
        .read_resume_slice(ResumeSliceQuery {
            agent_id,
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
        match reconcile_started_only(state, agent_id, started.session_id, turn_id).await {
            Err(error) => return Err(error),
            Ok(never) => match never {},
        }
    }

    let baseline = materialize_baseline(state.pg(), agent_id, base).await?;
    let definition = decode_definition(view)?;
    let snapshot = started.snapshot;
    if snapshot.agent_version_id != view.agent_version_id {
        return Err(ApiError::new(ErrorKind::DurableStateCorrupt));
    }

    // Reconstruct the exact runtime pinned by the snapshot.
    let provider = state
        .providers()
        .configure(&snapshot.model)
        .map_err(|_| ApiError::new(ErrorKind::RuntimeUnavailable))?;
    let registry = build_tool_registry(&definition.tools)?;
    let fingerprint = registry
        .fingerprint()
        .map_err(|source| ApiError::with_source(ErrorKind::Internal, source))?;
    if fingerprint != snapshot.tool_set_fingerprint {
        return Err(ApiError::new(ErrorKind::RuntimeUnavailable));
    }

    let ids = TurnIds {
        agent_id,
        session_id: started.session_id,
        turn_id,
    };
    let dispatcher = state.dispatchers().ensure(agent_id, through);
    let hook_runtime = build_hook_runtime(state, ids, dispatcher.clone());
    if hook_runtime.extension_set_version() != Some(snapshot.extension_set_version_id) {
        return Err(ApiError::new(ErrorKind::RuntimeUnavailable));
    }

    // The resumed sink's lineage mirrors the rebuilt kernel context: the
    // historical baseline plus every current-Turn commit up to the barrier.
    let mut lineage = baseline.lineage.clone();
    for row in &slice[1..] {
        match &row.event {
            DurableAgentEvent::MessageAppended { .. } => lineage.record_message(row.event_seq),
            DurableAgentEvent::TranscriptCompacted { upto, .. } => lineage.apply_compaction(*upto),
            _ => {}
        }
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

    // Replay window: the current LoopStarted, the historical baseline as
    // message events, then the exact current-Turn suffix in event order.
    let mut window = Vec::with_capacity(slice.len() + baseline.messages.len());
    window.push(slice[0].event.clone());
    window.extend(
        baseline
            .messages
            .iter()
            .cloned()
            .map(|message| DurableAgentEvent::MessageAppended { message }),
    );
    window.extend(slice[1..].iter().map(|row| row.event.clone()));

    let prepared = agent_loop
        .prepare_resume(definition.prompt.clone(), window)
        .map_err(|error| match error {
            stratum_agent::ResumeError::ExtensionSetVersionMismatch { .. } => {
                ApiError::new(ErrorKind::RuntimeUnavailable)
            }
            other => ApiError::with_source(ErrorKind::DurableStateCorrupt, other),
        })?;

    let handle = spawn_managed_turn(state, ids, claim, TurnRun::Resume(prepared), None);
    state
        .registry()
        .attach_task(agent_id, turn_id, claim.claim_id, handle);
    state
        .registry()
        .mark_running(agent_id, turn_id, claim.claim_id);
    Ok((
        StatusCode::ACCEPTED,
        Json(TurnAccepted {
            agent_id,
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
    agent_id: stratum_core::AgentId,
    session_id: SessionId,
    turn_id: TurnId,
) -> Result<std::convert::Infallible, ApiError> {
    let result = state
        .pg()
        .append_event(AppendEvent {
            agent_id,
            session_id,
            turn_id,
            event: DurableAgentEvent::LoopFailed {
                error_text: "turn preamble incomplete".to_owned(),
                usage: TokenUsage::default(),
            },
            approval_hook_invocation_id: None,
            default_model_update: None,
            compaction: None,
        })
        .await;
    match result {
        Ok(receipt) => {
            state.dispatchers().receipt(agent_id, receipt.event_seq);
            Err(ApiError::new(ErrorKind::TurnPreambleIncomplete))
        }
        Err(append_error) => {
            let reread = state
                .pg()
                .read_agent_state(agent_id)
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
    path = "/v1/agents/{agent_id}/cancel",
    params(("agent_id" = String, Path, description = "agent identity")),
    request_body = CancelRequest,
    responses(
        (status = 202, description = "cancellation signal accepted by the hosted turn"),
        (status = 204, description = "the exact turn is already cancelled"),
        (status = 400, description = "invalid request body or agent identity", body = ErrorResponse),
        (status = 404, description = "agent not found", body = ErrorResponse),
        (status = 409, description = "stale turn, turn not running/hosted, or turn still starting", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 503, description = "store unavailable or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn post_cancel(
    State(state): State<Arc<AppState>>,
    Path(agent_id): Path<String>,
    request: Result<Json<CancelRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let agent_id = parse_agent_id(&agent_id)?;
    Span::current().record("agent_id", field::display(agent_id));
    let body = json_request(request)?;

    let agent_state = state
        .pg()
        .read_agent_state(agent_id)
        .await
        .map_err(ApiError::from_postgres)?;
    if agent_state.current_turn_id != Some(body.turn_id) {
        return Err(ApiError::new(ErrorKind::StaleTurn));
    }
    match agent_state.status {
        stratum_postgres::AgentStatus::Cancelled => Ok(StatusCode::NO_CONTENT.into_response()),
        stratum_postgres::AgentStatus::Finished | stratum_postgres::AgentStatus::Failed => {
            Err(ApiError::new(ErrorKind::TurnNotRunning))
        }
        stratum_postgres::AgentStatus::Running => {
            match state.registry().claim_state(agent_id, body.turn_id) {
                Some(crate::registry::ClaimState::Starting) => {
                    Err(ApiError::new(ErrorKind::TurnStarting))
                }
                Some(crate::registry::ClaimState::Running) => {
                    let token = state
                        .registry()
                        .running_token(agent_id, body.turn_id)
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
    path = "/v1/agents/{agent_id}/approvals/{approval_id}",
    params(
        ("agent_id" = String, Path, description = "agent identity"),
        ("approval_id" = String, Path, description = "approval request identity"),
    ),
    request_body = ApprovalResolveRequest,
    responses(
        (status = 204, description = "decision committed (or an identical decision already exists)"),
        (status = 400, description = "invalid request body or path identity", body = ErrorResponse),
        (status = 404, description = "agent or approval not found", body = ErrorResponse),
        (status = 409, description = "stale turn, opposite decision exists, or approval invalidated", body = ErrorResponse),
        (status = 413, description = "request body is too large", body = ErrorResponse),
        (status = 500, description = "durable state is corrupt", body = ErrorResponse),
        (status = 503, description = "store unavailable or service shutting down", body = ErrorResponse),
    )
)]
pub(crate) async fn post_approval(
    State(state): State<Arc<AppState>>,
    Path((agent_id, approval_id)): Path<(String, String)>,
    request: Result<Json<ApprovalResolveRequest>, JsonRejection>,
) -> Result<Response, ApiError> {
    let _admission = state.admission().enter()?;
    let agent_id = parse_agent_id(&agent_id)?;
    let approval_uuid = uuid::Uuid::parse_str(&approval_id)
        .map_err(|_| ApiError::new(ErrorKind::InvalidRequest))?;
    let approval_id = ApprovalId::from(approval_uuid);
    Span::current().record("agent_id", field::display(agent_id));
    let body = json_request(request)?;

    let outcome = state
        .pg()
        .resolve_approval(ResolveApproval {
            agent_id,
            approval_id,
            turn_id: body.turn_id,
            decision: body.decision,
        })
        .await
        .map_err(ApiError::from_postgres)?;
    if let ResolveApprovalOutcome::Resolved { receipt } = outcome {
        // Commit first, then best-effort notification and realtime receipt.
        state.waiters().notify(approval_id);
        state.dispatchers().receipt(agent_id, receipt.event_seq);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
