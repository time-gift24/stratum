//! Durable tool approval: the decide-phase hook handler and the process-local
//! waiter registry.
//!
//! The approval handler is an ordinary `decide_tool_call` [`HookHandler`].
//! The kernel commits `HookInvocationPending` before invoking it; the handler
//! finds that open invocation by its exact address, reuses or creates the
//! durable `ToolApprovalRequested` bound to the invocation, then waits with
//! register-then-read: register the waiter, re-read the ledger, and re-read
//! again on every wake. Notifications are best-effort latency only; the
//! ledger is the only truth. Cancellation stops the wait without a decision
//! (the kernel's own cancellation race treats the invocation as aborted).
//!
//! No typed secret value can occur in the current closed composition: the
//! only registered tool is `echo`, its arguments are schema-validated opaque
//! user-authored JSON, authorization metadata is `(ToolKind, DangerLevel)`,
//! and there is no runtime credential provider or injection channel. Echo's
//! result is the same non-runtime-managed JSON and still crosses the kernel's
//! `AfterToolCall` boundary; the resulting `Keep` is durable-safe for this
//! composition, not a generic secret-scanning claim. A future credential-aware
//! tool must add a typed secret/reference boundary and fail a secret-bearing
//! Requested/result append closed before it can be registered here.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::FutureExt;
use stratum_agent::{DecideToolCallDecision, DecideToolCallInput, HookControl, HookHandler};
use stratum_core::{
    AgentId, AgentRuntimeId, ApprovalDecision, ApprovalId, DurableAgentEvent, HookFailure,
    HookHandlerVersionId, HookInvocationId, HookPoint, SessionId, ToolName, TurnId,
};
use stratum_postgres::{AppendEvent, ApprovalLookup, HookInvocationLookup, PostgresBackend};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::dispatcher::DispatcherHandle;
use crate::error::PersistedVariantError;

/// One registered waiter: its unique registration identity and notify half.
type WaiterEntry = (Uuid, oneshot::Sender<()>);

/// Process-local approval waiters keyed by exact AgentRuntime + approval.
#[derive(Debug, Default)]
pub(crate) struct ApprovalWaiters {
    inner: Mutex<HashMap<(AgentRuntimeId, ApprovalId), Vec<WaiterEntry>>>,
}

impl ApprovalWaiters {
    /// Registers a waiter for one approval (register-then-read protocol). The
    /// returned guard unregisters on drop, so an early resolution hit, a read
    /// error, or a cancelled wait never leaks the sender.
    pub(crate) fn register(
        &self,
        agent_runtime_id: AgentRuntimeId,
        approval_id: ApprovalId,
    ) -> (WaiterRegistration<'_>, oneshot::Receiver<()>) {
        let (sender, receiver) = oneshot::channel();
        let registration_id = Uuid::now_v7();
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry((agent_runtime_id, approval_id))
            .or_default()
            .push((registration_id, sender));
        (
            WaiterRegistration {
                waiters: self,
                agent_runtime_id,
                approval_id,
                registration_id,
            },
            receiver,
        )
    }

    /// Best-effort notification after a decision commits; loss is harmless
    /// because waiters re-read the ledger on every wake and on the poll
    /// interval.
    pub(crate) fn notify(&self, agent_runtime_id: AgentRuntimeId, approval_id: ApprovalId) {
        let senders = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&(agent_runtime_id, approval_id));
        if let Some(senders) = senders {
            for (_, sender) in senders {
                let _ = sender.send(());
            }
        }
    }

    /// Removes one exact registration; a notification that already removed
    /// the entry makes this a no-op.
    fn unregister(
        &self,
        agent_runtime_id: AgentRuntimeId,
        approval_id: ApprovalId,
        registration_id: Uuid,
    ) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(senders) = inner.get_mut(&(agent_runtime_id, approval_id)) {
            senders.retain(|(id, _)| *id != registration_id);
            if senders.is_empty() {
                inner.remove(&(agent_runtime_id, approval_id));
            }
        }
    }
}

/// RAII half of [`ApprovalWaiters::register`]: dropping it removes only this
/// exact registration identity.
#[derive(Debug)]
pub(crate) struct WaiterRegistration<'a> {
    waiters: &'a ApprovalWaiters,
    agent_runtime_id: AgentRuntimeId,
    approval_id: ApprovalId,
    registration_id: Uuid,
}

impl Drop for WaiterRegistration<'_> {
    fn drop(&mut self) {
        self.waiters.unregister(
            self.agent_runtime_id,
            self.approval_id,
            self.registration_id,
        );
    }
}

/// Stable version identity of the approval handler; decision behavior changes
/// must mint a new id (constitution §5).
pub(crate) const APPROVAL_HANDLER_VERSION: Uuid =
    Uuid::from_u128(0x5f1a7c2e_8b3d_4e6f_9a01_c4d5e6f70819);

/// Fixed upper bound between durable approval re-reads. Notifications are a
/// latency optimization only, so a lost notification converges within this
/// internal interval without exposing a protocol knob.
const APPROVAL_RECHECK_INTERVAL: Duration = Duration::from_secs(15);

/// Approval decide hook bound to one exact Turn.
pub(crate) struct ApprovalHandler {
    pg: PostgresBackend,
    agent_runtime_id: AgentRuntimeId,
    agent_id: AgentId,
    session_id: SessionId,
    turn_id: TurnId,
    waiters: Arc<ApprovalWaiters>,
    dispatcher: DispatcherHandle,
}

impl ApprovalHandler {
    /// Creates the handler for one Turn.
    #[must_use]
    pub(crate) fn new(
        pg: PostgresBackend,
        agent_runtime_id: AgentRuntimeId,
        agent_id: AgentId,
        session_id: SessionId,
        turn_id: TurnId,
        waiters: Arc<ApprovalWaiters>,
        dispatcher: DispatcherHandle,
    ) -> Self {
        Self {
            pg,
            agent_runtime_id,
            agent_id,
            session_id,
            turn_id,
            waiters,
            dispatcher,
        }
    }

    /// Reads the current ledger facts of one approval request.
    async fn read_facts(
        &self,
        lookup: ApprovalLookup,
    ) -> Result<Option<stratum_postgres::ApprovalFacts>, HookFailure> {
        self.pg
            .read_approval(self.agent_runtime_id, self.turn_id, lookup)
            .await
            .map_err(|error| {
                tracing::warn!(
                    agent_runtime_id = %self.agent_runtime_id,
                    agent_id = %self.agent_id,
                    turn_id = %self.turn_id,
                    error = %error,
                    "approval ledger read failed"
                );
                HookFailure::HandlerFailed
            })
    }

    /// Maps a durable decision to the hook decision: approve executes, reject
    /// blocks with a safe fixed reason.
    fn map_decision(
        decision: ApprovalDecision,
    ) -> Result<DecideToolCallDecision, PersistedVariantError> {
        match decision {
            ApprovalDecision::Approve => Ok(DecideToolCallDecision::Execute),
            ApprovalDecision::Reject => Ok(DecideToolCallDecision::Block {
                reason: "the user rejected this tool call".to_owned(),
            }),
            _ => Err(PersistedVariantError::UnsupportedApprovalDecision),
        }
    }

    /// Converts the typed persisted-variant error at the hook boundary without
    /// fabricating a permissive or rejecting decision.
    fn resolved_decision(
        &self,
        decision: ApprovalDecision,
    ) -> Result<DecideToolCallDecision, HookFailure> {
        Self::map_decision(decision).map_err(|error| {
            tracing::error!(
                agent_runtime_id = %self.agent_runtime_id,
                agent_id = %self.agent_id,
                turn_id = %self.turn_id,
                error = %error,
                "persisted approval decision is unsupported"
            );
            HookFailure::HandlerFailed
        })
    }
}

#[async_trait::async_trait]
impl HookHandler for ApprovalHandler {
    fn descriptor(&self) -> stratum_agent::HookHandlerDescriptor {
        stratum_agent::HookHandlerDescriptor::new(HookHandlerVersionId::from(
            APPROVAL_HANDLER_VERSION,
        ))
    }

    async fn decide_tool_call<'a>(
        &self,
        input: DecideToolCallInput<'a>,
        control: HookControl,
    ) -> Result<DecideToolCallDecision, HookFailure> {
        // Pre-authorized calls (registry allows them, or a transform hook
        // pre-authorized) never wait for a human.
        let Some((tool_kind, danger_level)) = input.tool.authorization else {
            return Ok(DecideToolCallDecision::Execute);
        };
        let call = input.tool_call;

        // The kernel committed `HookInvocationPending` before this call; the
        // open invocation at this exact address is this invocation.
        let invocation_id = self
            .pg
            .read_open_hook_invocation(HookInvocationLookup {
                agent_runtime_id: self.agent_runtime_id,
                turn_id: self.turn_id,
                point: HookPoint::DecideToolCall,
                iteration: input.snapshot.iteration,
                call_id: Some(call.call_id.clone()),
            })
            .await
            .map_err(|error| {
                tracing::warn!(
                    agent_runtime_id = %self.agent_runtime_id,
                    agent_id = %self.agent_id,
                    turn_id = %self.turn_id,
                    error = %error,
                    "approval invocation lookup failed"
                );
                HookFailure::HandlerFailed
            })?
            .ok_or_else(|| {
                tracing::error!(
                    agent_runtime_id = %self.agent_runtime_id,
                    agent_id = %self.agent_id,
                    turn_id = %self.turn_id,
                    "decide hook ran without a journaled pending invocation"
                );
                HookFailure::HandlerFailed
            })?;

        // Resume path: a request already bound to this invocation is reused
        // and a committed decision maps directly, without re-asking.
        let approval_id = match self
            .read_facts(ApprovalLookup::ByHookInvocationId(invocation_id))
            .await?
        {
            Some(facts) => {
                let facts = require_active_approval_facts(facts)?;
                if let Some(resolution) = facts.resolution {
                    return self.resolved_decision(resolution.decision);
                }
                facts.approval_id
            }
            None => {
                self.request_approval(invocation_id, call, tool_kind, danger_level)
                    .await?
            }
        };

        // Register-then-read: a decision committed before registration is
        // observed by the immediate re-read. The registration guard lives to
        // the end of this call, so every exit path unregisters the waiter.
        let (_registration, waiter) = self.waiters.register(self.agent_runtime_id, approval_id);
        let mut waiter = waiter.fuse();
        let facts = require_active_approval_facts(require_approval_facts(
            self.read_facts(ApprovalLookup::ByApprovalId(approval_id))
                .await?,
        )?)?;
        if let Some(resolution) = facts.resolution {
            return self.resolved_decision(resolution.decision);
        }

        loop {
            tokio::select! {
                biased;
                () = control.cancellation().cancelled() => {
                    // Never fabricate a decision: the kernel's own
                    // cancellation race has already won, so this future is
                    // dropped right after; pending keeps that contract
                    // explicit for the caller side of the race.
                    std::future::pending::<()>().await;
                    unreachable!("a cancelled approval wait never resumes");
                }
                _ = &mut waiter => {}
                () = tokio::time::sleep(APPROVAL_RECHECK_INTERVAL) => {}
            }
            match self
                .read_facts(ApprovalLookup::ByApprovalId(approval_id))
                .await
            {
                Ok(facts) => {
                    let facts = require_active_approval_facts(require_approval_facts(facts)?)?;
                    if let Some(resolution) = facts.resolution {
                        return self.resolved_decision(resolution.decision);
                    }
                }
                Err(failure) => return Err(failure),
            }
        }
    }
}

/// Once an approval id has been acquired, its Requested row is permanent.
/// Absence is durable corruption and must terminate the hook wait.
fn require_approval_facts(
    facts: Option<stratum_postgres::ApprovalFacts>,
) -> Result<stratum_postgres::ApprovalFacts, HookFailure> {
    facts.ok_or(HookFailure::HandlerFailed)
}

/// A consumed or invalidated request cannot authorize another execution.
/// Seeing either fact while the handler is active means replay/lifecycle
/// state is inconsistent, so the hook fails closed.
fn require_active_approval_facts(
    facts: stratum_postgres::ApprovalFacts,
) -> Result<stratum_postgres::ApprovalFacts, HookFailure> {
    if approval_facts_are_active(facts.consumed_event_seq, facts.invalidated_event_seq) {
        Ok(facts)
    } else {
        Err(HookFailure::HandlerFailed)
    }
}

const fn approval_facts_are_active(
    consumed_event_seq: Option<u64>,
    invalidated_event_seq: Option<u64>,
) -> bool {
    consumed_event_seq.is_none() && invalidated_event_seq.is_none()
}

impl ApprovalHandler {
    /// Appends the durable `ToolApprovalRequested` bound to the open
    /// invocation, reusing an existing request when a concurrent writer
    /// committed it first.
    async fn request_approval(
        &self,
        invocation_id: HookInvocationId,
        call: &stratum_core::ToolCall,
        tool_kind: stratum_core::ToolKind,
        danger_level: stratum_core::DangerLevel,
    ) -> Result<ApprovalId, HookFailure> {
        let approval_id = ApprovalId::new();
        let event = DurableAgentEvent::ToolApprovalRequested {
            approval_id,
            call_id: call.call_id.clone(),
            tool_name: ToolName::new(call.name.clone()),
            arguments: call.arguments.clone(),
            tool_kind,
            danger_level,
        };
        let result = self
            .pg
            .append_event(AppendEvent {
                agent_runtime_id: self.agent_runtime_id,
                agent_id: self.agent_id,
                session_id: self.session_id,
                turn_id: self.turn_id,
                event,
                approval_hook_invocation_id: Some(invocation_id),
                model_config_update: None,
                compaction: None,
            })
            .await;
        match result {
            Ok(receipt) => {
                self.dispatcher.receipt(receipt.event_seq);
                Ok(approval_id)
            }
            Err(stratum_postgres::PostgresError::ApprovalAlreadyRequested { .. }) => {
                // A concurrent or recovered writer committed the request for
                // this invocation first; reuse it.
                let facts = require_active_approval_facts(require_approval_facts(
                    self.read_facts(ApprovalLookup::ByHookInvocationId(invocation_id))
                        .await?,
                )?)?;
                Ok(facts.approval_id)
            }
            Err(error) => {
                tracing::warn!(
                    agent_runtime_id = %self.agent_runtime_id,
                    agent_id = %self.agent_id,
                    turn_id = %self.turn_id,
                    error = %error,
                    "approval request append failed"
                );
                Err(HookFailure::HandlerFailed)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn notify_wakes_every_registered_waiter() {
        let waiters = ApprovalWaiters::default();
        let agent_runtime_id = AgentRuntimeId::new();
        let approval_id = ApprovalId::new();
        let (_first_guard, first) = waiters.register(agent_runtime_id, approval_id);
        let (_second_guard, second) = waiters.register(agent_runtime_id, approval_id);

        waiters.notify(agent_runtime_id, approval_id);

        assert!(first.await.is_ok());
        assert!(second.await.is_ok());
        // A decision for an unknown approval notifies nobody and never fails.
        waiters.notify(agent_runtime_id, ApprovalId::new());
    }

    #[tokio::test]
    async fn dropped_registration_unregisters_the_waiter() {
        let waiters = ApprovalWaiters::default();
        let agent_runtime_id = AgentRuntimeId::new();
        let approval_id = ApprovalId::new();
        let (_kept_guard, kept) = waiters.register(agent_runtime_id, approval_id);
        let (dropped_guard, dropped) = waiters.register(agent_runtime_id, approval_id);
        drop(dropped_guard);

        // The dropped sender is gone: the waiter observes a closed channel
        // and a notify only reaches the live registration.
        assert!(dropped.await.is_err());
        waiters.notify(agent_runtime_id, approval_id);
        assert!(kept.await.is_ok());
        // The entry was consumed by the notify; nothing accumulates.
        assert!(
            waiters
                .inner
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_empty()
        );
    }

    #[tokio::test]
    async fn same_approval_uuid_is_isolated_by_agent_runtime() {
        let waiters = ApprovalWaiters::default();
        let runtime_a = AgentRuntimeId::new();
        let runtime_b = AgentRuntimeId::new();
        let approval_id = ApprovalId::new();
        let (_guard_a, waiter_a) = waiters.register(runtime_a, approval_id);
        let (_guard_b, mut waiter_b) = waiters.register(runtime_b, approval_id);

        waiters.notify(runtime_a, approval_id);

        assert!(waiter_a.await.is_ok());
        assert!(
            tokio::time::timeout(Duration::from_millis(10), &mut waiter_b)
                .await
                .is_err(),
            "a resolver wakes only the exact runtime waiter"
        );
        waiters.notify(runtime_b, approval_id);
        assert!(waiter_b.await.is_ok());
    }

    #[test]
    fn acquired_approval_id_without_requested_facts_fails_closed() {
        assert!(matches!(
            require_approval_facts(None),
            Err(HookFailure::HandlerFailed)
        ));
    }

    #[test]
    fn consumed_or_invalidated_approval_facts_fail_closed() {
        assert!(approval_facts_are_active(None, None));
        assert!(!approval_facts_are_active(Some(7), None));
        assert!(!approval_facts_are_active(None, Some(8)));
        assert!(!approval_facts_are_active(Some(7), Some(8)));
    }

    #[test]
    fn handler_version_identity_is_stable() {
        assert_eq!(
            HookHandlerVersionId::from(APPROVAL_HANDLER_VERSION),
            HookHandlerVersionId::from(APPROVAL_HANDLER_VERSION)
        );
    }

    #[test]
    fn persisted_approval_decisions_require_explicit_mapping() {
        assert!(matches!(
            ApprovalHandler::map_decision(ApprovalDecision::Approve),
            Ok(DecideToolCallDecision::Execute)
        ));
        assert!(matches!(
            ApprovalHandler::map_decision(ApprovalDecision::Reject),
            Ok(DecideToolCallDecision::Block { reason })
                if reason == "the user rejected this tool call"
        ));
    }
}
