//! Process-local hosting registry.
//!
//! The registry tracks exact `(AgentId, TurnId)` claims: a unique claim
//! identity, the `starting`/`running` state, the Turn cancellation token, and
//! the managed task handle. Nothing here is persisted; Postgres remains the
//! only durable truth and every command revalidates durable state. The lock
//! only guards the in-memory map and is never held across an `.await`.

use std::collections::HashMap;
use std::sync::Mutex;

use stratum_core::{AgentId, TurnId};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Lifecycle state of one process claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaimState {
    /// Preflight or managed task installation is in progress.
    Starting,
    /// The exact Turn is being driven by this process.
    Running,
}

/// One installed claim.
#[allow(dead_code)] // fields are read through the registry API
struct Claim {
    /// Unique identity of this claim installation.
    claim_id: Uuid,
    /// Current lifecycle state.
    state: ClaimState,
    /// Turn cancellation token signalled by the cancel endpoint.
    token: CancellationToken,
    /// Managed task handle, attached right after spawn.
    task: Option<JoinHandle<()>>,
}

/// A freshly installed claim, owned by the installing request.
#[derive(Debug, Clone)]
pub(crate) struct ClaimHandle {
    /// Unique identity of this claim installation.
    pub(crate) claim_id: Uuid,
    /// Turn cancellation token shared with the managed task.
    pub(crate) token: CancellationToken,
}

/// Outcome of an atomic claim attempt.
#[derive(Debug)]
pub(crate) enum ClaimOutcome {
    /// This request installed the claim.
    Claimed(ClaimHandle),
    /// A claim for the exact Turn already exists.
    Exists,
}

/// Process-local registry of hosted Turns.
#[derive(Default)]
pub(crate) struct TurnRegistry {
    claims: Mutex<HashMap<(AgentId, TurnId), Claim>>,
}

impl TurnRegistry {
    /// Atomically installs a `starting` claim for the exact Turn; the first
    /// caller wins and later callers observe the existing claim.
    pub(crate) fn try_claim(&self, agent_id: AgentId, turn_id: TurnId) -> ClaimOutcome {
        let mut claims = self.lock();
        match claims.entry((agent_id, turn_id)) {
            std::collections::hash_map::Entry::Occupied(_) => ClaimOutcome::Exists,
            std::collections::hash_map::Entry::Vacant(entry) => {
                let claim = Claim {
                    claim_id: Uuid::now_v7(),
                    state: ClaimState::Starting,
                    token: CancellationToken::new(),
                    task: None,
                };
                let handle = ClaimHandle {
                    claim_id: claim.claim_id,
                    token: claim.token.clone(),
                };
                entry.insert(claim);
                ClaimOutcome::Claimed(handle)
            }
        }
    }

    /// Attaches the managed task handle to a claim installed by this request.
    pub(crate) fn attach_task(
        &self,
        agent_id: AgentId,
        turn_id: TurnId,
        claim_id: Uuid,
        task: JoinHandle<()>,
    ) {
        let mut claims = self.lock();
        if let Some(claim) = claims.get_mut(&(agent_id, turn_id))
            && claim.claim_id == claim_id
        {
            claim.task = Some(task);
        }
    }

    /// Marks a claim installed by this request as `running`.
    pub(crate) fn mark_running(&self, agent_id: AgentId, turn_id: TurnId, claim_id: Uuid) {
        let mut claims = self.lock();
        if let Some(claim) = claims.get_mut(&(agent_id, turn_id))
            && claim.claim_id == claim_id
        {
            claim.state = ClaimState::Running;
        }
    }

    /// Returns the state of the exact-Turn claim, when one exists.
    pub(crate) fn claim_state(&self, agent_id: AgentId, turn_id: TurnId) -> Option<ClaimState> {
        self.lock()
            .get(&(agent_id, turn_id))
            .map(|claim| claim.state)
    }

    /// Returns the cancellation token of a `running` exact-Turn claim.
    pub(crate) fn running_token(
        &self,
        agent_id: AgentId,
        turn_id: TurnId,
    ) -> Option<CancellationToken> {
        let claims = self.lock();
        let claim = claims.get(&(agent_id, turn_id))?;
        (claim.state == ClaimState::Running).then(|| claim.token.clone())
    }

    /// Compare-and-removes a claim: only the exact claim identity that
    /// installed it can remove it, so stale cleanup never deletes a newer
    /// claim of a later Turn.
    pub(crate) fn compare_remove(&self, agent_id: AgentId, turn_id: TurnId, claim_id: Uuid) {
        let mut claims = self.lock();
        if claims
            .get(&(agent_id, turn_id))
            .is_some_and(|claim| claim.claim_id == claim_id)
        {
            claims.remove(&(agent_id, turn_id));
        }
    }

    /// Takes every managed task handle for the bounded shutdown drain. The
    /// claims themselves stay installed; their tokens are never signalled.
    pub(crate) fn take_tasks(&self) -> Vec<JoinHandle<()>> {
        let mut claims = self.lock();
        claims
            .values_mut()
            .filter_map(|claim| claim.task.take())
            .collect()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(AgentId, TurnId), Claim>> {
        self.claims.lock().unwrap_or_else(|poisoned| {
            // A panicked mutation never leaves a half-updated claim behind:
            // every critical section above is a single map operation.
            poisoned.into_inner()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_claim_wins_and_second_observes_it() {
        let registry = TurnRegistry::default();
        let agent_id = AgentId::new();
        let turn_id = TurnId::new();

        let first = registry.try_claim(agent_id, turn_id);
        assert!(matches!(first, ClaimOutcome::Claimed(_)));
        assert!(matches!(
            registry.try_claim(agent_id, turn_id),
            ClaimOutcome::Exists
        ));
        // A different turn of the same agent is independent.
        assert!(matches!(
            registry.try_claim(agent_id, TurnId::new()),
            ClaimOutcome::Claimed(_)
        ));
    }

    #[test]
    fn compare_remove_only_removes_the_exact_claim_identity() {
        let registry = TurnRegistry::default();
        let agent_id = AgentId::new();
        let turn_id = TurnId::new();

        let ClaimOutcome::Claimed(handle) = registry.try_claim(agent_id, turn_id) else {
            panic!("first claim installs");
        };
        // A stale cleanup with another identity keeps the claim.
        registry.compare_remove(agent_id, turn_id, Uuid::now_v7());
        assert_eq!(
            registry.claim_state(agent_id, turn_id),
            Some(ClaimState::Starting)
        );
        registry.compare_remove(agent_id, turn_id, handle.claim_id);
        assert_eq!(registry.claim_state(agent_id, turn_id), None);
    }

    #[test]
    fn running_token_is_only_available_for_running_claims() {
        let registry = TurnRegistry::default();
        let agent_id = AgentId::new();
        let turn_id = TurnId::new();

        let ClaimOutcome::Claimed(handle) = registry.try_claim(agent_id, turn_id) else {
            panic!("first claim installs");
        };
        assert!(registry.running_token(agent_id, turn_id).is_none());
        registry.mark_running(agent_id, turn_id, handle.claim_id);
        assert!(registry.running_token(agent_id, turn_id).is_some());
        assert!(matches!(
            registry.try_claim(agent_id, turn_id),
            ClaimOutcome::Exists
        ));
    }
}
