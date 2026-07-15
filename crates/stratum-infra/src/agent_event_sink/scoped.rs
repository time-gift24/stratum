//! Adapter from local agent-loop events to externally scoped stream envelopes.

use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use async_trait::async_trait;
use serde_json::Value;
use stratum_core::{
    AgentEvent, AgentId, AgentTelemetryEvent, DurableAgentEvent, EventSource, LlmCallRole,
    LlmEvent, RunId, RuntimeEvent, StreamEnvelope, TurnId,
};
use tracing::warn;

use super::{DurableEventSink, DurableEventSinkError, TelemetryEventSink};
use crate::EventStreamBus;

const TELEMETRY_PUBLISH_TIMEOUT: Duration = Duration::from_millis(100);

/// Adds run, agent, and turn scope before publishing agent-loop events.
pub struct ScopedAgentEventSink {
    agent_id: AgentId,
    agent_name: String,
    run_id: RunId,
    turn_id: TurnId,
    event_bus: Arc<dyn EventStreamBus>,
}

impl ScopedAgentEventSink {
    /// Creates a sink bound to one agent turn.
    #[must_use]
    pub fn new(
        agent_id: AgentId,
        agent_name: impl Into<String>,
        run_id: RunId,
        turn_id: TurnId,
        event_bus: Arc<dyn EventStreamBus>,
    ) -> Self {
        Self {
            agent_id,
            agent_name: agent_name.into(),
            run_id,
            turn_id,
            event_bus,
        }
    }

    fn durable_agent_event(
        &self,
        event: DurableAgentEvent,
    ) -> Result<AgentEvent, DurableEventSinkError> {
        let event_type = event.event_type();
        let event = match event {
            DurableAgentEvent::LoopStarted => AgentEvent::Started {
                turn_id: self.turn_id,
            },
            DurableAgentEvent::MessageAppended { message } => AgentEvent::Message {
                turn_id: self.turn_id,
                message,
            },
            DurableAgentEvent::ToolApprovalRequested {
                approval_id,
                call_id,
                tool_name,
                arguments,
                tool_kind,
                danger_level,
            } => AgentEvent::ToolApprovalRequested {
                approval_id,
                agent_name: self.agent_name.clone(),
                call_id,
                tool_name,
                arguments,
                tool_kind,
                danger_level,
            },
            DurableAgentEvent::ToolApprovalResolved {
                approval_id,
                decision,
            } => AgentEvent::ToolApprovalResolved {
                approval_id,
                decision,
            },
            DurableAgentEvent::ToolExecutionStarted { call_id, tool_name } => {
                AgentEvent::ToolExecutionStarted {
                    turn_id: self.turn_id,
                    call_id,
                    tool_name,
                }
            }
            DurableAgentEvent::IterationCompleted { iteration, usage } => {
                AgentEvent::IterationCompleted {
                    turn_id: self.turn_id,
                    iteration,
                    usage,
                }
            }
            DurableAgentEvent::LoopFinished {
                finish_reason,
                usage,
            } => AgentEvent::Finished {
                finish_reason,
                usage,
            },
            DurableAgentEvent::LoopFailed { error_text, usage } => {
                AgentEvent::Failed { error_text, usage }
            }
            DurableAgentEvent::LoopCancelled { usage } => AgentEvent::Cancelled { usage },
            _ => return Err(DurableEventSinkError::UnsupportedEvent { event_type }),
        };
        Ok(event)
    }

    fn telemetry_agent_event(&self, event: AgentTelemetryEvent) -> Option<AgentEvent> {
        let event_type = event.event_type();
        let event = match event {
            AgentTelemetryEvent::LlmStarted { llm_call_id } => AgentEvent::Llm {
                llm_call_id,
                event: LlmEvent::Started,
            },
            AgentTelemetryEvent::TextDelta { llm_call_id, delta } => AgentEvent::Llm {
                llm_call_id,
                event: LlmEvent::TextDelta {
                    role: LlmCallRole::Assistant,
                    delta,
                },
            },
            AgentTelemetryEvent::ReasoningDelta { llm_call_id, delta } => AgentEvent::Llm {
                llm_call_id,
                event: LlmEvent::ReasoningDelta { delta },
            },
            AgentTelemetryEvent::ToolCallDelta {
                llm_call_id,
                call_id,
                name,
                arguments_delta,
            } => AgentEvent::Llm {
                llm_call_id,
                event: LlmEvent::ToolCallDelta {
                    call_id,
                    name,
                    arguments_delta,
                },
            },
            AgentTelemetryEvent::LlmFinished {
                llm_call_id,
                finish_reason,
                usage,
            } => AgentEvent::Llm {
                llm_call_id,
                event: LlmEvent::Finished {
                    finish_reason,
                    usage,
                },
            },
            _ => {
                warn!(
                    agent_id = %self.agent_id,
                    run_id = %self.run_id,
                    turn_id = %self.turn_id,
                    event_type,
                    "ignored unsupported agent telemetry event"
                );
                return None;
            }
        };
        Some(event)
    }

    fn envelope(&self, event: AgentEvent) -> StreamEnvelope {
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "agent_name".to_owned(),
            Value::String(self.agent_name.clone()),
        );
        metadata.insert(
            "turn_id".to_owned(),
            Value::String(self.turn_id.to_string()),
        );

        StreamEnvelope {
            business_seq: None,
            run_id: self.run_id,
            timestamp: SystemTime::now().into(),
            source: EventSource::Run,
            event: RuntimeEvent::Agent {
                agent_id: self.agent_id,
                event,
            },
            metadata,
        }
    }
}

#[async_trait]
impl DurableEventSink for ScopedAgentEventSink {
    async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError> {
        let event = self.durable_agent_event(event)?;
        self.event_bus.publish(self.envelope(event)).await?;
        Ok(())
    }
}

#[async_trait]
impl TelemetryEventSink for ScopedAgentEventSink {
    async fn emit(&self, event: AgentTelemetryEvent) {
        let event_type = event.event_type();
        let Some(event) = self.telemetry_agent_event(event) else {
            return;
        };
        match tokio::time::timeout(
            TELEMETRY_PUBLISH_TIMEOUT,
            self.event_bus.publish(self.envelope(event)),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                warn!(
                    agent_id = %self.agent_id,
                    run_id = %self.run_id,
                    turn_id = %self.turn_id,
                    event_type,
                    error = %error,
                    "failed to publish agent telemetry event"
                );
            }
            Err(error) => {
                warn!(
                    agent_id = %self.agent_id,
                    run_id = %self.run_id,
                    turn_id = %self.turn_id,
                    event_type,
                    error = %error,
                    "agent telemetry publish timed out"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        future::pending,
        sync::{Arc, Mutex},
        time::Duration,
    };

    use async_trait::async_trait;
    use futures_util::stream;
    use serde_json::json;
    use stratum_core::{
        AgentEvent, AgentId, AgentTelemetryEvent, ApprovalDecision, ApprovalId, CallId,
        ChatMessage, DangerLevel, DurableAgentEvent, EventSource, LlmCallId, LlmCallRole, LlmEvent,
        ReplayStart, RunId, RuntimeEvent, StreamEnvelope, TokenUsage, ToolKind, ToolName, TurnId,
    };

    use crate::{
        DurableEventSink, DurableEventSinkError, EventStream, EventStreamBus, EventStreamBusError,
        ScopedAgentEventSink, TelemetryEventSink,
    };

    #[derive(Default)]
    struct RecordingEventStreamBus {
        published: Mutex<Vec<StreamEnvelope>>,
    }

    impl RecordingEventStreamBus {
        fn take_published(&self) -> Vec<StreamEnvelope> {
            std::mem::take(
                &mut *self
                    .published
                    .lock()
                    .expect("recording event stream bus lock should not be poisoned"),
            )
        }
    }

    #[async_trait]
    impl EventStreamBus for RecordingEventStreamBus {
        async fn publish(&self, envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
            self.published
                .lock()
                .expect("recording event stream bus lock should not be poisoned")
                .push(envelope);
            Ok(())
        }

        async fn subscribe_agent(
            &self,
            _agent_id: AgentId,
            _replay_start: ReplayStart,
        ) -> Result<EventStream, EventStreamBusError> {
            Ok(Box::pin(stream::empty()))
        }
    }

    #[derive(Default)]
    struct FailingEventStreamBus {
        published: Mutex<Vec<StreamEnvelope>>,
    }

    struct NeverCompletingEventStreamBus;

    #[async_trait]
    impl EventStreamBus for NeverCompletingEventStreamBus {
        async fn publish(&self, _envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
            pending().await
        }

        async fn subscribe_agent(
            &self,
            _agent_id: AgentId,
            _replay_start: ReplayStart,
        ) -> Result<EventStream, EventStreamBusError> {
            Ok(Box::pin(stream::empty()))
        }
    }

    impl FailingEventStreamBus {
        fn take_published(&self) -> Vec<StreamEnvelope> {
            std::mem::take(
                &mut *self
                    .published
                    .lock()
                    .expect("failing event stream bus lock should not be poisoned"),
            )
        }
    }

    #[async_trait]
    impl EventStreamBus for FailingEventStreamBus {
        async fn publish(&self, envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
            self.published
                .lock()
                .expect("failing event stream bus lock should not be poisoned")
                .push(envelope);
            Err(EventStreamBusError::MissingAgentScope)
        }

        async fn subscribe_agent(
            &self,
            _agent_id: AgentId,
            _replay_start: ReplayStart,
        ) -> Result<EventStream, EventStreamBusError> {
            Ok(Box::pin(stream::empty()))
        }
    }

    #[tokio::test]
    async fn durable_event_is_scoped_and_returns_bus_error() {
        let agent_id = AgentId::new();
        let run_id = RunId::new();
        let turn_id = TurnId::new();
        let recorder = Arc::new(FailingEventStreamBus::default());
        let event_bus: Arc<dyn EventStreamBus> = recorder.clone();
        let sink = ScopedAgentEventSink::new(
            agent_id,
            "review-agent",
            run_id,
            turn_id,
            Arc::clone(&event_bus),
        );

        let error = sink
            .append(DurableAgentEvent::LoopStarted)
            .await
            .expect_err("durable publish failure must reach the caller");

        assert!(matches!(
            error,
            DurableEventSinkError::EventStreamBus(EventStreamBusError::MissingAgentScope)
        ));
        let [envelope] = recorder
            .take_published()
            .try_into()
            .expect("exactly one envelope should be published");
        assert_eq!(envelope.run_id, run_id);
        assert_eq!(envelope.source, EventSource::Run);
        assert_eq!(
            envelope.metadata.get("agent_name"),
            Some(&json!("review-agent"))
        );
        assert_eq!(
            envelope.event,
            RuntimeEvent::Agent {
                agent_id,
                event: AgentEvent::Started { turn_id },
            }
        );
    }

    #[tokio::test]
    async fn telemetry_event_is_published_without_exposing_bus_error() {
        let agent_id = AgentId::new();
        let run_id = RunId::new();
        let turn_id = TurnId::new();
        let recorder = Arc::new(FailingEventStreamBus::default());
        let event_bus: Arc<dyn EventStreamBus> = recorder.clone();
        let sink = ScopedAgentEventSink::new(
            agent_id,
            "review-agent",
            run_id,
            turn_id,
            Arc::clone(&event_bus),
        );
        let llm_call_id = LlmCallId::from("llm-call-1");

        sink.emit(AgentTelemetryEvent::TextDelta {
            llm_call_id: llm_call_id.clone(),
            delta: "hello".to_owned(),
        })
        .await;

        let [envelope] = recorder
            .take_published()
            .try_into()
            .expect("exactly one envelope should be published");
        assert_eq!(envelope.run_id, run_id);
        assert_eq!(
            envelope.event,
            RuntimeEvent::Agent {
                agent_id,
                event: AgentEvent::Llm {
                    llm_call_id,
                    event: LlmEvent::TextDelta {
                        role: LlmCallRole::Assistant,
                        delta: "hello".to_owned(),
                    },
                },
            }
        );
    }

    #[tokio::test]
    async fn every_turn_scoped_envelope_includes_stable_turn_metadata() {
        let agent_id = AgentId::new();
        let run_id = RunId::new();
        let turn_id = TurnId::new();
        let recorder = Arc::new(RecordingEventStreamBus::default());
        let event_bus: Arc<dyn EventStreamBus> = recorder.clone();
        let sink = ScopedAgentEventSink::new(
            agent_id,
            "review-agent",
            run_id,
            turn_id,
            Arc::clone(&event_bus),
        );
        let approval_id = ApprovalId::new();

        sink.append(DurableAgentEvent::LoopFinished {
            finish_reason: "stop".to_owned(),
            usage: TokenUsage::default(),
        })
        .await
        .expect("terminal event should publish");
        sink.append(DurableAgentEvent::ToolApprovalRequested {
            approval_id,
            call_id: CallId::from("tool-call-1"),
            tool_name: ToolName::from("write_file"),
            arguments: json!({"path": "notes.txt"}),
            tool_kind: ToolKind::Write,
            danger_level: DangerLevel::High,
        })
        .await
        .expect("approval request should publish");
        sink.append(DurableAgentEvent::ToolApprovalResolved {
            approval_id,
            decision: ApprovalDecision::Approve,
        })
        .await
        .expect("approval resolution should publish");
        sink.emit(AgentTelemetryEvent::LlmStarted {
            llm_call_id: LlmCallId::from("llm-call-1"),
        })
        .await;

        let envelopes = recorder.take_published();
        assert_eq!(envelopes.len(), 4);
        for envelope in &envelopes {
            assert_eq!(
                envelope.metadata.get("agent_name"),
                Some(&json!("review-agent"))
            );
            assert_eq!(
                envelope.metadata.get("turn_id"),
                Some(&json!(turn_id.to_string()))
            );
        }
        assert!(matches!(
            envelopes[0].event,
            RuntimeEvent::Agent {
                event: AgentEvent::Finished { .. },
                ..
            }
        ));
        assert!(matches!(
            envelopes[1].event,
            RuntimeEvent::Agent {
                event: AgentEvent::ToolApprovalRequested { .. },
                ..
            }
        ));
        assert!(matches!(
            envelopes[2].event,
            RuntimeEvent::Agent {
                event: AgentEvent::ToolApprovalResolved { .. },
                ..
            }
        ));
        assert!(matches!(
            envelopes[3].event,
            RuntimeEvent::Agent {
                event: AgentEvent::Llm { .. },
                ..
            }
        ));
    }

    #[tokio::test]
    async fn telemetry_emit_returns_when_bus_publish_never_completes() {
        let event_bus: Arc<dyn EventStreamBus> = Arc::new(NeverCompletingEventStreamBus);
        let sink = ScopedAgentEventSink::new(
            AgentId::new(),
            "review-agent",
            RunId::new(),
            TurnId::new(),
            event_bus,
        );

        tokio::time::timeout(
            Duration::from_secs(1),
            sink.emit(AgentTelemetryEvent::LlmStarted {
                llm_call_id: LlmCallId::from("llm-call-1"),
            }),
        )
        .await
        .expect("best-effort telemetry should not wait indefinitely for the event bus");
    }

    #[tokio::test]
    async fn durable_message_maps_to_external_message_event() {
        let agent_id = AgentId::new();
        let run_id = RunId::new();
        let turn_id = TurnId::new();
        let recorder = Arc::new(RecordingEventStreamBus::default());
        let event_bus: Arc<dyn EventStreamBus> = recorder.clone();
        let sink = ScopedAgentEventSink::new(
            agent_id,
            "review-agent",
            run_id,
            turn_id,
            Arc::clone(&event_bus),
        );
        let message = ChatMessage::assistant("done");

        sink.append(DurableAgentEvent::MessageAppended {
            message: message.clone(),
        })
        .await
        .expect("recording event stream bus should accept the message");

        let [envelope] = recorder
            .take_published()
            .try_into()
            .expect("exactly one envelope should be published");
        assert_eq!(
            envelope.event,
            RuntimeEvent::Agent {
                agent_id,
                event: AgentEvent::Message { turn_id, message },
            }
        );
    }

    #[tokio::test]
    async fn durable_tool_start_and_iteration_map_to_external_events() {
        let agent_id = AgentId::new();
        let run_id = RunId::new();
        let turn_id = TurnId::new();
        let recorder = Arc::new(RecordingEventStreamBus::default());
        let event_bus: Arc<dyn EventStreamBus> = recorder.clone();
        let sink = ScopedAgentEventSink::new(
            agent_id,
            "review-agent",
            run_id,
            turn_id,
            Arc::clone(&event_bus),
        );
        let usage = TokenUsage {
            input_tokens: 1,
            output_tokens: 2,
            total_tokens: 3,
        };

        sink.append(DurableAgentEvent::ToolExecutionStarted {
            call_id: CallId::from("tool-call-1"),
            tool_name: ToolName::from("echo"),
        })
        .await
        .expect("tool start should publish");
        sink.append(DurableAgentEvent::IterationCompleted {
            iteration: 4,
            usage,
        })
        .await
        .expect("iteration completion should publish");

        let [started, completed] = recorder
            .take_published()
            .try_into()
            .expect("exactly two envelopes should be published");
        let RuntimeEvent::Agent {
            event: started_event,
            ..
        } = &started.event
        else {
            panic!("tool execution start should be an agent event");
        };
        let RuntimeEvent::Agent {
            event: completed_event,
            ..
        } = &completed.event
        else {
            panic!("iteration completion should be an agent event");
        };
        assert_eq!(started_event.event_type(), "tool_execution_started");
        assert_eq!(completed_event.event_type(), "iteration_completed");
        assert_eq!(
            started.event,
            RuntimeEvent::Agent {
                agent_id,
                event: AgentEvent::ToolExecutionStarted {
                    turn_id,
                    call_id: CallId::from("tool-call-1"),
                    tool_name: ToolName::from("echo"),
                },
            }
        );
        assert_eq!(
            completed.event,
            RuntimeEvent::Agent {
                agent_id,
                event: AgentEvent::IterationCompleted {
                    turn_id,
                    iteration: 4,
                    usage,
                },
            }
        );
    }
}
