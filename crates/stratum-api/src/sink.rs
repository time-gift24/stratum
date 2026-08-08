//! Per-turn sink adapters between the kernel contracts and Postgres.
//!
//! [`TurnDurableSink`] implements the kernel's `DurableEventSink` for one
//! exact Turn: `LoopStarted` routes to the admission transaction
//! (`begin_turn`), the first user `MessageAppended` carries the effective
//! model as the conditional default-model update, `TranscriptCompacted`
//! resolves its retained pointer from the provenance lineage, and every other
//! event goes through the centralized append. The sink acknowledges only
//! after commit and feeds each commit receipt to the realtime dispatcher.
//!
//! [`TurnTelemetrySink`] implements `TelemetryEventSink`, assigning the
//! call-local `telemetry_seq` from 0 per `llm_call_id`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use stratum_core::{
    AgentId, AgentTelemetryEvent, ChatRole, DurableAgentEvent, LlmCallId, ModelConfig, SessionId,
    TurnId, TurnRuntimeSnapshot,
};
use stratum_infra::{DurableEventSink, DurableEventSinkError, TelemetryEventSink};
use stratum_postgres::{AppendEvent, BeginTurn, CompactionInput, PostgresBackend};
use tokio::sync::oneshot;

use crate::dispatcher::DispatcherHandle;
use crate::error::{ApiError, ErrorKind, kind_of_postgres};
use crate::provenance::ContextLineage;

/// One-shot admission signal: the HTTP handler waits for the first user
/// message of a fresh Turn to commit before answering 202. The sink completes
/// it on success; the sink or the task wrapper completes it with the mapped
/// error on any earlier failure.
type AdmissionSender = Arc<Mutex<Option<oneshot::Sender<Result<(), ApiError>>>>>;

#[derive(Debug, Clone)]
pub(crate) struct AdmissionSignal {
    sender: AdmissionSender,
}

impl AdmissionSignal {
    /// Creates the signal/receiver pair.
    #[must_use]
    pub(crate) fn new() -> (Self, oneshot::Receiver<Result<(), ApiError>>) {
        let (sender, receiver) = oneshot::channel();
        (
            Self {
                sender: Arc::new(Mutex::new(Some(sender))),
            },
            receiver,
        )
    }

    /// Completes the signal successfully (first user message committed).
    pub(crate) fn complete(&self) {
        self.send(Ok(()));
    }

    /// Completes the signal with an error; a completed signal stays completed.
    pub(crate) fn fail(&self, error: ApiError) {
        self.send(Err(error));
    }

    fn send(&self, outcome: Result<(), ApiError>) {
        let sender = self
            .sender
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        if let Some(sender) = sender {
            // A dropped receiver means the HTTP handler already left (for
            // example on shutdown); the durable outcome is unaffected.
            let _ = sender.send(outcome);
        }
    }
}

/// Exact Turn identity shared by the per-turn adapters.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TurnIds {
    /// Owning Agent.
    pub(crate) agent_id: AgentId,
    /// Bound Session.
    pub(crate) session_id: SessionId,
    /// Exact Turn.
    pub(crate) turn_id: TurnId,
}

/// Kernel durable-sink adapter bound to one exact Turn.
pub(crate) struct TurnDurableSink {
    pg: PostgresBackend,
    ids: TurnIds,
    /// Admission state, present only for fresh Turns.
    admission: Option<FreshAdmission>,
    lineage: Mutex<ContextLineage>,
    dispatcher: DispatcherHandle,
}

/// Fresh-turn admission context supplied by the message handler: the CAS
/// expectation, the pinned runtime snapshot, the effective model, and the
/// first-message signal.
pub(crate) struct FreshTurnAdmission {
    /// CAS expectation of the admission transaction.
    pub(crate) expected_current_turn_id: Option<TurnId>,
    /// Runtime snapshot pinned to the `LoopStarted` row.
    pub(crate) snapshot: TurnRuntimeSnapshot,
    /// Effective model of the Turn.
    pub(crate) effective_model: ModelConfig,
    /// First-user-message admission signal.
    pub(crate) signal: AdmissionSignal,
}

/// Live fresh-turn admission state.
struct FreshAdmission {
    expected_current_turn_id: Option<TurnId>,
    snapshot: TurnRuntimeSnapshot,
    effective_model: ModelConfig,
    signal: AdmissionSignal,
    first_user_pending: AtomicBool,
}

impl TurnDurableSink {
    /// Creates the sink for a fresh Turn (admission CAS plus admission
    /// signal).
    #[must_use]
    pub(crate) fn fresh(
        pg: PostgresBackend,
        ids: TurnIds,
        admission: FreshTurnAdmission,
        lineage: ContextLineage,
        dispatcher: DispatcherHandle,
    ) -> Self {
        Self {
            pg,
            ids,
            admission: Some(FreshAdmission {
                expected_current_turn_id: admission.expected_current_turn_id,
                snapshot: admission.snapshot,
                effective_model: admission.effective_model,
                signal: admission.signal,
                first_user_pending: AtomicBool::new(true),
            }),
            lineage: Mutex::new(lineage),
            dispatcher,
        }
    }

    /// Creates the sink for a resumed Turn: no admission CAS, no admission
    /// signal, and no default-model update (a resumed loop never appends
    /// `LoopStarted` and never prompts).
    #[must_use]
    pub(crate) fn resumed(
        pg: PostgresBackend,
        ids: TurnIds,
        lineage: ContextLineage,
        dispatcher: DispatcherHandle,
    ) -> Self {
        Self {
            pg,
            ids,
            admission: None,
            lineage: Mutex::new(lineage),
            dispatcher,
        }
    }

    fn lineage(&self) -> MutexGuard<'_, ContextLineage> {
        self.lineage
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn append_command(
        &self,
        event: DurableAgentEvent,
        compaction: Option<CompactionInput>,
        default_model_update: Option<ModelConfig>,
    ) -> AppendEvent {
        AppendEvent {
            agent_id: self.ids.agent_id,
            session_id: self.ids.session_id,
            turn_id: self.ids.turn_id,
            event,
            approval_hook_invocation_id: None,
            default_model_update,
            compaction,
        }
    }
}

#[async_trait::async_trait]
impl DurableEventSink for TurnDurableSink {
    async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
        match &event {
            DurableAgentEvent::LoopStarted { .. } => {
                let Some(admission) = &self.admission else {
                    // A resumed loop never starts a second loop.
                    return Err(DurableEventSinkError::UnsupportedEvent {
                        event_type: event.event_type(),
                    });
                };
                let result = self
                    .pg
                    .begin_turn(BeginTurn {
                        agent_id: self.ids.agent_id,
                        expected_current_turn_id: admission.expected_current_turn_id,
                        turn_id: self.ids.turn_id,
                        session_id: self.ids.session_id,
                        snapshot: admission.snapshot.clone(),
                    })
                    .await;
                match result {
                    Ok(receipt) => {
                        self.dispatcher.receipt(receipt.event_seq);
                        Ok(())
                    }
                    Err(error) => Err(self.admission_failure(error)),
                }
            }
            DurableAgentEvent::MessageAppended { message } => {
                let first_user = self.admission.as_ref().is_some_and(|admission| {
                    admission.first_user_pending.load(Ordering::Acquire)
                        && message.role == ChatRole::User
                });
                let default_model_update = match (&self.admission, first_user) {
                    (Some(admission), true) => Some(admission.effective_model.clone()),
                    _ => None,
                };
                let result = self
                    .pg
                    .append_event(self.append_command(event, None, default_model_update))
                    .await;
                let receipt = match result {
                    Ok(receipt) => receipt,
                    Err(error) => {
                        if first_user {
                            return Err(self.admission_failure(error));
                        }
                        return Err(DurableEventSinkError::backend(error));
                    }
                };
                self.lineage().record_message(receipt.event_seq);
                if first_user && let Some(admission) = &self.admission {
                    admission.first_user_pending.store(false, Ordering::Release);
                    admission.signal.complete();
                }
                self.dispatcher.receipt(receipt.event_seq);
                Ok(())
            }
            DurableAgentEvent::TranscriptCompacted {
                upto,
                summary,
                compacted_iteration,
            } => {
                // Copy the companion facts first so the event can move into
                // the append command.
                let upto = *upto;
                let summary = summary.clone();
                let compacted_iteration = *compacted_iteration;
                let retained_from_event_seq =
                    self.lineage().retained_from(upto).ok_or_else(|| {
                        DurableEventSinkError::backend(ApiError::new(
                            ErrorKind::DurableStateCorrupt,
                        ))
                    })?;
                let receipt = self
                    .pg
                    .append_event(self.append_command(
                        event,
                        Some(CompactionInput {
                            compacted_iteration,
                            upto,
                            retained_from_event_seq,
                            summary,
                        }),
                        None,
                    ))
                    .await
                    .map_err(DurableEventSinkError::backend)?;
                self.lineage().apply_compaction(upto);
                self.dispatcher.receipt(receipt.event_seq);
                Ok(())
            }
            // Approval facts are written only by the approval handler and the
            // resolver, never by the kernel.
            DurableAgentEvent::ToolApprovalRequested { .. }
            | DurableAgentEvent::ToolApprovalResolved { .. } => {
                Err(DurableEventSinkError::UnsupportedEvent {
                    event_type: event.event_type(),
                })
            }
            _ => {
                let receipt = self
                    .pg
                    .append_event(self.append_command(event, None, None))
                    .await
                    .map_err(DurableEventSinkError::backend)?;
                self.dispatcher.receipt(receipt.event_seq);
                Ok(())
            }
        }
    }
}

impl TurnDurableSink {
    /// Reports a failed admission boundary to the waiting HTTP handler with
    /// its precise classification and hands the kernel a backend failure.
    fn admission_failure(&self, error: stratum_postgres::PostgresError) -> DurableEventSinkError {
        let kind = kind_of_postgres(&error);
        if let Some(admission) = &self.admission {
            admission.signal.fail(ApiError::with_source(kind, error));
        }
        DurableEventSinkError::backend(ApiError::new(kind))
    }
}

/// Kernel telemetry-sink adapter for one exact Turn.
pub(crate) struct TurnTelemetrySink {
    ids: TurnIds,
    dispatcher: DispatcherHandle,
    next_seq: Mutex<HashMap<LlmCallId, u64>>,
}

impl TurnTelemetrySink {
    /// Creates the telemetry adapter.
    #[must_use]
    pub(crate) fn new(ids: TurnIds, dispatcher: DispatcherHandle) -> Self {
        Self {
            ids,
            dispatcher,
            next_seq: Mutex::new(HashMap::new()),
        }
    }
}

impl TelemetryEventSink for TurnTelemetrySink {
    fn emit(&self, event: AgentTelemetryEvent) {
        let llm_call_id = match &event {
            AgentTelemetryEvent::LlmStarted { llm_call_id }
            | AgentTelemetryEvent::TextDelta { llm_call_id, .. }
            | AgentTelemetryEvent::ReasoningDelta { llm_call_id, .. }
            | AgentTelemetryEvent::ToolCallDelta { llm_call_id, .. }
            | AgentTelemetryEvent::LlmFinished { llm_call_id, .. } => llm_call_id.clone(),
            _ => return,
        };
        let telemetry_seq = {
            let mut seqs = self
                .next_seq
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let next = seqs.entry(llm_call_id.clone()).or_insert(0);
            let assigned = *next;
            *next = next.saturating_add(1);
            assigned
        };
        self.dispatcher.telemetry(
            self.ids.session_id,
            self.ids.turn_id,
            llm_call_id,
            telemetry_seq,
            event,
        );
    }
}

#[cfg(test)]
mod tests {
    use stratum_infra::TelemetryEventSink;

    use super::*;
    use crate::dispatcher::{DispatcherCommand, DispatcherHandle};

    #[test]
    fn telemetry_seq_starts_at_zero_per_llm_call() {
        let (dispatcher, mut rx) = DispatcherHandle::stub(AgentId::new());
        let sink = TurnTelemetrySink::new(
            TurnIds {
                agent_id: AgentId::new(),
                session_id: SessionId::new(),
                turn_id: TurnId::new(),
            },
            dispatcher,
        );
        let first_call = LlmCallId::from("call-1");
        let second_call = LlmCallId::from("call-2");
        sink.emit(AgentTelemetryEvent::LlmStarted {
            llm_call_id: first_call.clone(),
        });
        sink.emit(AgentTelemetryEvent::TextDelta {
            llm_call_id: first_call.clone(),
            delta: "a".to_owned(),
        });
        sink.emit(AgentTelemetryEvent::LlmFinished {
            llm_call_id: first_call,
            finish_reason: "stop".to_owned(),
            usage: None,
        });
        sink.emit(AgentTelemetryEvent::LlmStarted {
            llm_call_id: second_call,
        });
        drop(sink);

        let mut seqs = Vec::new();
        while let Ok(DispatcherCommand::Telemetry { telemetry_seq, .. }) = rx.try_recv() {
            seqs.push(telemetry_seq);
        }
        assert_eq!(seqs, vec![0, 1, 2, 0]);
    }

    #[test]
    fn admission_signal_completes_exactly_once() {
        let (signal, receiver) = AdmissionSignal::new();
        signal.complete();
        signal.fail(ApiError::new(ErrorKind::Internal));
        assert!(
            receiver
                .blocking_recv()
                .is_ok_and(|outcome| outcome.is_ok())
        );

        let (signal, receiver) = AdmissionSignal::new();
        signal.fail(ApiError::new(ErrorKind::StoreUnavailable));
        let outcome = receiver.blocking_recv().expect("signal delivered");
        assert_eq!(
            outcome.expect_err("failure delivered").kind(),
            ErrorKind::StoreUnavailable
        );
    }
}
