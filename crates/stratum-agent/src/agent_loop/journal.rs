//! Hook invocation journal reconstructed from and appended to the durable
//! event stream.
//!
//! The journal lives inside the single [`DurableEventSink`] stream as the
//! `HookInvocationPending` / `HookInvocationCompleted` / `HookInvocationFailed`
//! variants; there is no second durability boundary. Its address shape is the
//! kernel-minimal `(iteration, HookPoint, Option<CallId>)`: tool hooks
//! distinguish same-iteration calls by `CallId`, while `transform_context` and
//! `prepare_next_turn` are uniquely identified by `(iteration, point)`.
//! Session/Turn ownership stays with the composing storage scope.
//!
//! Input digests are payload-level: tool hooks hash the canonical JSON of the
//! exact [`ToolCall`] the hook observes (the original call at
//! `transform_tool_call`, the final re-validated call at `decide_tool_call`
//! and `after_tool_call`); context hooks hash the stable bytes of their
//! `(iteration, point)` address. Usage and conversation history never
//! participate: replaying the same event stream rebuilds byte-identical
//! contexts for free.

use std::collections::HashMap;
use std::fmt::Write as _;

use sha2::{Digest, Sha256};
use stratum_core::{
    AfterToolCallDecisionRecord, AuthorizationOverrideRecord, CallId, DecideToolCallDecisionRecord,
    HookDecisionRecord, HookFailure, HookInputDigest, HookInvocationId, HookPoint,
    PrepareNextTurnDecisionRecord, ToolCall, TransformContextDecisionRecord,
    TransformToolCallDecisionRecord, TransformToolCallModificationRecord,
};

use super::ResumeError;
use crate::{
    AfterToolCallDecision, AuthorizationOverride, DecideToolCallDecision, PrepareNextTurnDecision,
    TransformContextDecision, TransformToolCallDecision, TransformToolCallModification,
};

/// Kernel-minimal semantic address of one hook invocation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct HookAddress {
    iteration: u64,
    point: HookPoint,
    call_id: Option<CallId>,
}

impl HookAddress {
    pub(crate) fn new(iteration: u64, point: HookPoint, call_id: Option<CallId>) -> Self {
        Self {
            iteration,
            point,
            call_id,
        }
    }
}

/// Everything the kernel journals about one hook invocation site.
pub(crate) struct HookInvocationSite<'a> {
    pub(crate) journal: &'a HookJournal,
    pub(crate) point: HookPoint,
    pub(crate) iteration: u64,
    pub(crate) call_id: Option<CallId>,
    pub(crate) input_digest: HookInputDigest,
}

/// Lifecycle of one journaled invocation.
#[derive(Debug)]
pub(crate) enum JournalState {
    /// The invocation was journaled before calling the runtime.
    Pending,
    /// A validated decision was journaled before applying it.
    Completed(HookDecisionRecord),
    /// The invocation reached a typed terminal failure.
    Failed(HookFailure),
}

/// One journaled invocation entry.
#[derive(Debug)]
pub(crate) struct JournalEntry {
    pub(crate) invocation_id: HookInvocationId,
    pub(crate) input_digest: HookInputDigest,
    pub(crate) state: JournalState,
}

/// Hook invocation journal rebuilt from a run's durable event stream.
///
/// Fresh runs start with an empty journal: every lookup misses and every hook
/// invocation appends a new pending record. Resumed runs rebuild the journal
/// during replay and consult it before invoking the runtime.
#[derive(Debug, Default)]
pub(crate) struct HookJournal {
    entries: HashMap<HookAddress, JournalEntry>,
    by_invocation: HashMap<HookInvocationId, HookAddress>,
}

impl HookJournal {
    /// Returns the journaled entry for one invocation address.
    pub(crate) fn lookup(&self, address: &HookAddress) -> Option<&JournalEntry> {
        self.entries.get(address)
    }

    /// Replays one pending record into the journal.
    pub(crate) fn record_pending(
        &mut self,
        address: HookAddress,
        invocation_id: HookInvocationId,
        input_digest: HookInputDigest,
    ) -> Result<(), ResumeError> {
        if self.entries.contains_key(&address) {
            return Err(ResumeError::DuplicateHookInvocation);
        }
        self.by_invocation.insert(invocation_id, address.clone());
        self.entries.insert(
            address,
            JournalEntry {
                invocation_id,
                input_digest,
                state: JournalState::Pending,
            },
        );
        Ok(())
    }

    /// Replays one completed record, resolving its address by invocation id.
    pub(crate) fn record_completed(
        &mut self,
        invocation_id: &HookInvocationId,
        decision: HookDecisionRecord,
    ) -> Result<(), ResumeError> {
        let entry = self.resolve_mut(invocation_id)?;
        if !matches!(entry.state, JournalState::Pending) {
            return Err(ResumeError::DuplicateHookInvocation);
        }
        entry.state = JournalState::Completed(decision);
        Ok(())
    }

    /// Replays one failed record, resolving its address by invocation id.
    pub(crate) fn record_failed(
        &mut self,
        invocation_id: &HookInvocationId,
        failure: HookFailure,
    ) -> Result<(), ResumeError> {
        let entry = self.resolve_mut(invocation_id)?;
        if !matches!(entry.state, JournalState::Pending) {
            return Err(ResumeError::DuplicateHookInvocation);
        }
        entry.state = JournalState::Failed(failure);
        Ok(())
    }

    fn resolve_mut(
        &mut self,
        invocation_id: &HookInvocationId,
    ) -> Result<&mut JournalEntry, ResumeError> {
        let address = self
            .by_invocation
            .get(invocation_id)
            .ok_or(ResumeError::UnknownHookInvocation)?;
        self.entries
            .get_mut(address)
            .ok_or(ResumeError::UnknownHookInvocation)
    }
}

/// Bidirectional mapping between one hook decision and its journaled record.
pub(crate) trait JournalDecision: Sized {
    /// Projects the decision into its durable record.
    fn to_record(&self) -> HookDecisionRecord;
    /// Rebuilds the decision from a durable record; `None` when the record
    /// belongs to a different hook point or a future variant, which the caller
    /// treats as journal corruption.
    fn from_record(record: &HookDecisionRecord) -> Option<Self>;
}

impl JournalDecision for TransformContextDecision {
    fn to_record(&self) -> HookDecisionRecord {
        let record = match self {
            Self::Unchanged => TransformContextDecisionRecord::Unchanged,
            Self::Patch(patch) => TransformContextDecisionRecord::Patch(patch.clone()),
        };
        HookDecisionRecord::TransformContext(record)
    }

    fn from_record(record: &HookDecisionRecord) -> Option<Self> {
        match record {
            HookDecisionRecord::TransformContext(TransformContextDecisionRecord::Unchanged) => {
                Some(Self::Unchanged)
            }
            HookDecisionRecord::TransformContext(TransformContextDecisionRecord::Patch(patch)) => {
                Some(Self::Patch(patch.clone()))
            }
            _ => None,
        }
    }
}

impl JournalDecision for TransformToolCallDecision {
    fn to_record(&self) -> HookDecisionRecord {
        let record = match self {
            Self::Continue => TransformToolCallDecisionRecord::Continue,
            Self::Modify(modification) => {
                TransformToolCallDecisionRecord::Modify(TransformToolCallModificationRecord::new(
                    modification.arguments.clone(),
                    modification
                        .authorization
                        .map(AuthorizationOverrideRecord::from),
                ))
            }
        };
        HookDecisionRecord::TransformToolCall(record)
    }

    fn from_record(record: &HookDecisionRecord) -> Option<Self> {
        match record {
            HookDecisionRecord::TransformToolCall(TransformToolCallDecisionRecord::Continue) => {
                Some(Self::Continue)
            }
            HookDecisionRecord::TransformToolCall(TransformToolCallDecisionRecord::Modify(
                modification,
            )) => {
                let authorization = match modification.authorization.as_ref() {
                    Some(record) => Some(AuthorizationOverride::from_record(record)?),
                    None => None,
                };
                Some(Self::Modify(TransformToolCallModification::new(
                    modification.arguments.clone(),
                    authorization,
                )))
            }
            _ => None,
        }
    }
}

impl From<AuthorizationOverride> for AuthorizationOverrideRecord {
    fn from(override_: AuthorizationOverride) -> Self {
        match override_ {
            AuthorizationOverride::PreAuthorize => Self::PreAuthorize,
            AuthorizationOverride::Set { kind, danger } => Self::Set { kind, danger },
        }
    }
}

impl AuthorizationOverride {
    /// Converts a journaled record back to the runtime override.
    ///
    /// Unknown (future) record variants fail closed as `None`; a persisted
    /// override must never default toward pre-authorization.
    fn from_record(record: &AuthorizationOverrideRecord) -> Option<Self> {
        match record {
            AuthorizationOverrideRecord::PreAuthorize => Some(Self::PreAuthorize),
            AuthorizationOverrideRecord::Set { kind, danger } => Some(Self::Set {
                kind: *kind,
                danger: *danger,
            }),
            _ => None,
        }
    }
}

impl JournalDecision for DecideToolCallDecision {
    fn to_record(&self) -> HookDecisionRecord {
        let record = match self {
            Self::Execute => DecideToolCallDecisionRecord::Execute,
            Self::Block { reason } => DecideToolCallDecisionRecord::Block {
                reason: reason.clone(),
            },
        };
        HookDecisionRecord::DecideToolCall(record)
    }

    fn from_record(record: &HookDecisionRecord) -> Option<Self> {
        match record {
            HookDecisionRecord::DecideToolCall(DecideToolCallDecisionRecord::Execute) => {
                Some(Self::Execute)
            }
            HookDecisionRecord::DecideToolCall(DecideToolCallDecisionRecord::Block { reason }) => {
                Some(Self::Block {
                    reason: reason.clone(),
                })
            }
            _ => None,
        }
    }
}

impl JournalDecision for AfterToolCallDecision {
    fn to_record(&self) -> HookDecisionRecord {
        let record = match self {
            Self::Keep => AfterToolCallDecisionRecord::Keep,
            Self::ReplaceResult { result } => AfterToolCallDecisionRecord::ReplaceResult {
                result: result.clone(),
            },
        };
        HookDecisionRecord::AfterToolCall(record)
    }

    fn from_record(record: &HookDecisionRecord) -> Option<Self> {
        match record {
            HookDecisionRecord::AfterToolCall(AfterToolCallDecisionRecord::Keep) => {
                Some(Self::Keep)
            }
            HookDecisionRecord::AfterToolCall(AfterToolCallDecisionRecord::ReplaceResult {
                result,
            }) => Some(Self::ReplaceResult {
                result: result.clone(),
            }),
            _ => None,
        }
    }
}

impl JournalDecision for PrepareNextTurnDecision {
    fn to_record(&self) -> HookDecisionRecord {
        let record = match self {
            Self::Continue => PrepareNextTurnDecisionRecord::Continue,
            Self::Stop => PrepareNextTurnDecisionRecord::Stop,
            Self::Inject { messages } => PrepareNextTurnDecisionRecord::Inject {
                messages: messages.clone(),
            },
            Self::Compact { upto, summary } => PrepareNextTurnDecisionRecord::Compact {
                upto: *upto,
                summary: summary.clone(),
            },
        };
        HookDecisionRecord::PrepareNextTurn(record)
    }

    fn from_record(record: &HookDecisionRecord) -> Option<Self> {
        match record {
            HookDecisionRecord::PrepareNextTurn(PrepareNextTurnDecisionRecord::Continue) => {
                Some(Self::Continue)
            }
            HookDecisionRecord::PrepareNextTurn(PrepareNextTurnDecisionRecord::Stop) => {
                Some(Self::Stop)
            }
            HookDecisionRecord::PrepareNextTurn(PrepareNextTurnDecisionRecord::Inject {
                messages,
            }) => Some(Self::Inject {
                messages: messages.clone(),
            }),
            HookDecisionRecord::PrepareNextTurn(PrepareNextTurnDecisionRecord::Compact {
                upto,
                summary,
            }) => Some(Self::Compact {
                upto: *upto,
                summary: summary.clone(),
            }),
            _ => None,
        }
    }
}

/// Computes the payload digest of one tool hook invocation: the SHA-256 of the
/// canonical JSON of the exact [`ToolCall`] the hook observes.
pub(crate) fn tool_call_digest(tool_call: &ToolCall) -> HookInputDigest {
    let encoded =
        serde_json::to_vec(tool_call).expect("serializing a tool call to JSON is infallible");
    sha256_digest(&encoded)
}

/// Computes the digest of a hook point without a dedicated payload
/// (`transform_context`, `prepare_next_turn`): the address itself, as the
/// stable bytes of the serialized point and the iteration index.
pub(crate) fn hook_address_digest(iteration: u64, point: HookPoint) -> HookInputDigest {
    let mut bytes =
        serde_json::to_vec(&point).expect("serializing a hook point to JSON is infallible");
    bytes.extend_from_slice(b":");
    bytes.extend_from_slice(iteration.to_string().as_bytes());
    sha256_digest(&bytes)
}

fn sha256_digest(bytes: &[u8]) -> HookInputDigest {
    let digest = Sha256::digest(bytes);
    let hex = digest
        .iter()
        .fold(String::with_capacity(64), |mut output, byte| {
            write!(output, "{byte:02x}").expect("writing to a string cannot fail");
            output
        });
    hex.parse()
        .expect("sha-256 output is a canonical input digest")
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use stratum_core::{ChatMessage, ContextPatch};

    use super::*;

    #[test]
    fn tool_call_digest_is_stable_and_payload_sensitive() {
        let call = ToolCall {
            call_id: CallId::from("call-1"),
            name: "echo".to_owned(),
            arguments: json!({"a": 1, "b": 2}),
        };
        let same = ToolCall {
            call_id: CallId::from("call-1"),
            name: "echo".to_owned(),
            arguments: json!({"b": 2, "a": 1}),
        };
        let different = ToolCall {
            arguments: json!({"a": 1}),
            ..call.clone()
        };

        assert_eq!(tool_call_digest(&call), tool_call_digest(&same));
        assert_ne!(tool_call_digest(&call), tool_call_digest(&different));
    }

    #[test]
    fn address_digest_depends_on_point_and_iteration() {
        let digest = hook_address_digest(0, HookPoint::TransformContext);
        assert_eq!(digest, hook_address_digest(0, HookPoint::TransformContext));
        assert_ne!(digest, hook_address_digest(1, HookPoint::TransformContext));
        assert_ne!(digest, hook_address_digest(0, HookPoint::PrepareNextTurn));
    }

    #[test]
    fn decisions_round_trip_through_their_journal_records() {
        let transform = TransformContextDecision::Patch(ContextPatch::RewriteHistory {
            upto: 2,
            summary: ChatMessage::assistant("summary"),
        });
        let transform_tool = TransformToolCallDecision::Modify(TransformToolCallModification::new(
            Some(json!({"value": 2})),
            Some(AuthorizationOverride::PreAuthorize),
        ));
        let decide = DecideToolCallDecision::Block {
            reason: "denied".to_owned(),
        };
        let after = AfterToolCallDecision::ReplaceResult {
            result: json!({"redacted": true}),
        };
        let prepare = PrepareNextTurnDecision::Inject {
            messages: vec![ChatMessage::user("note")],
        };
        let compact = PrepareNextTurnDecision::Compact {
            upto: 2,
            summary: ChatMessage::system("summary so far"),
        };

        assert_eq!(
            TransformContextDecision::from_record(&transform.to_record()),
            Some(transform)
        );
        assert_eq!(
            TransformToolCallDecision::from_record(&transform_tool.to_record()),
            Some(transform_tool)
        );
        assert_eq!(
            DecideToolCallDecision::from_record(&decide.to_record()),
            Some(decide)
        );
        assert_eq!(
            AfterToolCallDecision::from_record(&after.to_record()),
            Some(after)
        );
        assert_eq!(
            PrepareNextTurnDecision::from_record(&prepare.to_record()),
            Some(prepare)
        );
        assert_eq!(
            PrepareNextTurnDecision::from_record(&compact.to_record()),
            Some(compact)
        );
    }

    #[test]
    fn records_of_another_point_never_rebuild_a_decision() {
        let record = HookDecisionRecord::DecideToolCall(DecideToolCallDecisionRecord::Execute);

        assert!(TransformContextDecision::from_record(&record).is_none());
        assert!(TransformToolCallDecision::from_record(&record).is_none());
        assert!(AfterToolCallDecision::from_record(&record).is_none());
        assert!(PrepareNextTurnDecision::from_record(&record).is_none());
        assert_eq!(
            DecideToolCallDecision::from_record(&record),
            Some(DecideToolCallDecision::Execute)
        );
    }

    #[test]
    fn journal_replay_enforces_pending_completion_order() {
        let mut journal = HookJournal::default();
        let address = HookAddress::new(0, HookPoint::TransformContext, None);
        let invocation_id = HookInvocationId::new();
        let digest = hook_address_digest(0, HookPoint::TransformContext);

        journal
            .record_pending(address.clone(), invocation_id, digest.clone())
            .expect("first pending should record");
        assert_eq!(
            journal.record_pending(address.clone(), HookInvocationId::new(), digest),
            Err(ResumeError::DuplicateHookInvocation)
        );
        assert_eq!(
            journal.record_failed(&HookInvocationId::new(), HookFailure::TimedOut),
            Err(ResumeError::UnknownHookInvocation)
        );
        journal
            .record_completed(
                &invocation_id,
                HookDecisionRecord::TransformContext(TransformContextDecisionRecord::Unchanged),
            )
            .expect("pending should complete");
        assert_eq!(
            journal.record_failed(&invocation_id, HookFailure::TimedOut),
            Err(ResumeError::DuplicateHookInvocation)
        );
        assert!(matches!(
            journal.lookup(&address).map(|entry| &entry.state),
            Some(JournalState::Completed(_))
        ));
    }
}
