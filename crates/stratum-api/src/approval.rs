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
//! No typed secret value can occur in a final tool call today: tool arguments
//! are plain JSON validated against the tool schema and the effective
//! authorization metadata is `(ToolKind, DangerLevel)` only, so the persisted
//! `ToolApprovalRequested` payload is durable-safe by construction. A future
//! typed-secret channel must fail this append closed.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::FutureExt;
use stratum_agent::{DecideToolCallDecision, DecideToolCallInput, HookControl, HookHandler};
use stratum_core::{
    AgentId, ApprovalDecision, ApprovalId, DurableAgentEvent, HookFailure, HookHandlerVersionId,
    HookInvocationId, HookPoint, SessionId, ToolName, TurnId,
};
use stratum_postgres::{AppendEvent, ApprovalLookup, HookInvocationLookup, PostgresBackend};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::dispatcher::DispatcherHandle;

/// Fallback ledger re-read interval while waiting for a decision. Process
/// notifications are the fast path; this poll bounds the damage of a lost
/// notification without making the channel a truth source.
const APPROVAL_POLL_INTERVAL: Duration = Duration::from_secs(15);

/// Process-local approval waiters keyed by `ApprovalId`.
#[derive(Debug, Default)]
pub(crate) struct ApprovalWaiters {
    inner: Mutex<HashMap<ApprovalId, Vec<oneshot::Sender<()>>>>,
}

impl ApprovalWaiters {
    /// Registers a waiter for one approval (register-then-read protocol).
    pub(crate) fn register(&self, approval_id: ApprovalId) -> oneshot::Receiver<()> {
        let (sender, receiver) = oneshot::channel();
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(approval_id)
            .or_default()
            .push(sender);
        receiver
    }

    /// Best-effort notification after a decision commits; loss is harmless
    /// because waiters re-read the ledger on every wake and on the poll
    /// interval.
    pub(crate) fn notify(&self, approval_id: ApprovalId) {
        let senders = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&approval_id);
        if let Some(senders) = senders {
            for sender in senders {
                let _ = sender.send(());
            }
        }
    }
}

/// Stable version identity of the approval handler; decision behavior changes
/// must mint a new id (constitution §5).
pub(crate) const APPROVAL_HANDLER_VERSION: Uuid =
    Uuid::from_u128(0x5f1a7c2e_8b3d_4e6f_9a01_c4d5e6f70819);

/// Approval decide hook bound to one exact Turn.
pub(crate) struct ApprovalHandler {
    pg: PostgresBackend,
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
        agent_id: AgentId,
        session_id: SessionId,
        turn_id: TurnId,
        waiters: Arc<ApprovalWaiters>,
        dispatcher: DispatcherHandle,
    ) -> Self {
        Self {
            pg,
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
            .read_approval(self.agent_id, self.turn_id, lookup)
            .await
            .map_err(|error| {
                tracing::warn!(
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
    fn map_decision(decision: ApprovalDecision) -> DecideToolCallDecision {
        match decision {
            ApprovalDecision::Approve => DecideToolCallDecision::Execute,
            ApprovalDecision::Reject => DecideToolCallDecision::Block {
                reason: "the user rejected this tool call".to_owned(),
            },
            _ => DecideToolCallDecision::Block {
                reason: "the tool call was not approved".to_owned(),
            },
        }
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
                agent_id: self.agent_id,
                turn_id: self.turn_id,
                point: HookPoint::DecideToolCall,
                iteration: input.snapshot.iteration,
                call_id: Some(call.call_id.clone()),
            })
            .await
            .map_err(|error| {
                tracing::warn!(
                    agent_id = %self.agent_id,
                    turn_id = %self.turn_id,
                    error = %error,
                    "approval invocation lookup failed"
                );
                HookFailure::HandlerFailed
            })?
            .ok_or_else(|| {
                tracing::error!(
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
                if let Some(resolution) = facts.resolution {
                    return Ok(Self::map_decision(resolution.decision));
                }
                facts.approval_id
            }
            None => {
                self.request_approval(invocation_id, call, tool_kind, danger_level)
                    .await?
            }
        };

        // Register-then-read: a decision committed before registration is
        // observed by the immediate re-read.
        let waiter = self.waiters.register(approval_id).fuse();
        if let Some(facts) = self
            .read_facts(ApprovalLookup::ByApprovalId(approval_id))
            .await?
            && let Some(resolution) = facts.resolution
        {
            return Ok(Self::map_decision(resolution.decision));
        }

        let mut waiter = waiter;
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
                () = tokio::time::sleep(APPROVAL_POLL_INTERVAL) => {}
            }
            match self
                .read_facts(ApprovalLookup::ByApprovalId(approval_id))
                .await
            {
                Ok(Some(facts)) => {
                    if let Some(resolution) = facts.resolution {
                        return Ok(Self::map_decision(resolution.decision));
                    }
                }
                Ok(None) => {
                    // The request row cannot vanish; treat as a transient
                    // read anomaly and keep waiting.
                    tracing::warn!(
                        agent_id = %self.agent_id,
                        turn_id = %self.turn_id,
                        "approval request vanished from the ledger; still waiting"
                    );
                }
                Err(failure) => return Err(failure),
            }
        }
    }
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
                agent_id: self.agent_id,
                session_id: self.session_id,
                turn_id: self.turn_id,
                event,
                approval_hook_invocation_id: Some(invocation_id),
                default_model_update: None,
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
                self.read_facts(ApprovalLookup::ByHookInvocationId(invocation_id))
                    .await?
                    .map(|facts| facts.approval_id)
                    .ok_or(HookFailure::HandlerFailed)
            }
            Err(error) => {
                tracing::warn!(
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
        let approval_id = ApprovalId::new();
        let first = waiters.register(approval_id);
        let second = waiters.register(approval_id);

        waiters.notify(approval_id);

        assert!(first.await.is_ok());
        assert!(second.await.is_ok());
        // A decision for an unknown approval notifies nobody and never fails.
        waiters.notify(ApprovalId::new());
    }

    #[test]
    fn handler_version_identity_is_stable() {
        assert_eq!(
            HookHandlerVersionId::from(APPROVAL_HANDLER_VERSION),
            HookHandlerVersionId::from(APPROVAL_HANDLER_VERSION)
        );
    }
}
