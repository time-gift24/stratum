use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

use async_trait::async_trait;
use chrono::Utc;
use stratum_core::{
    AgentEvent, AgentId, AgentLocation, AgentRuntimeContext, ChatMessage, EventRecord, HistoryPage,
    HistoryQuery, NewAgentMessage, ReplayStart, RuntimeEvent, SessionEvent, SessionId,
    StreamEnvelope, TokenUsage, TurnId, TurnRuntimeSnapshot,
};
use stratum_infra::{EventStream, EventStreamBus, EventStreamBusError, StoreEventStreamBus};
use stratum_store::{AgentState, AgentStatus, AgentStore, StoreError};

struct RecordingStore {
    state: Mutex<AgentState>,
    append_calls: AtomicUsize,
}

impl RecordingStore {
    fn new(agent_id: AgentId) -> Self {
        Self {
            state: Mutex::new(AgentState::new_configured(
                agent_id,
                "agent".to_owned(),
                stratum_core::ModelConfig::new(
                    "openai:test".parse().expect("model id parses"),
                    serde_json::Map::new(),
                ),
            )),
            append_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl AgentStore for RecordingStore {
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
        state.next_iteration = iteration + 1;
        state.usage = usage;
        Ok(state.clone())
    }

    async fn append_message(&self, message: NewAgentMessage) -> Result<StreamEnvelope, StoreError> {
        self.append_calls.fetch_add(1, Ordering::SeqCst);
        Ok(message.into_envelope(1))
    }

    async fn history_page(&self, _query: HistoryQuery) -> Result<HistoryPage, StoreError> {
        Ok(HistoryPage {
            through_seq: 0,
            events: Vec::new(),
            next_front_seq: 0,
            has_more: false,
        })
    }
}

#[derive(Default)]
struct RecordingBus {
    published: Mutex<Vec<StreamEnvelope>>,
    subscriptions: Mutex<Vec<(SessionId, ReplayStart)>>,
    fail_publish: bool,
}

#[async_trait]
impl EventStreamBus for RecordingBus {
    async fn publish(&self, envelope: StreamEnvelope) -> Result<(), EventStreamBusError> {
        if self.fail_publish {
            return Err(EventStreamBusError::CursorOverflow);
        }
        self.published
            .lock()
            .expect("published lock")
            .push(envelope);
        Ok(())
    }

    async fn subscribe_session(
        &self,
        session_id: SessionId,
        replay_start: ReplayStart,
    ) -> Result<EventStream, EventStreamBusError> {
        self.subscriptions
            .lock()
            .expect("subscriptions lock")
            .push((session_id, replay_start));
        Ok(Box::pin(futures_util::stream::empty::<
            Result<EventRecord, EventStreamBusError>,
        >()))
    }
}

fn agent_envelope(
    session_id: SessionId,
    agent_id: AgentId,
    turn_id: TurnId,
    event: AgentEvent,
) -> StreamEnvelope {
    StreamEnvelope {
        session_id,
        timestamp: Utc::now(),
        event: RuntimeEvent::Agent {
            agent_id,
            turn_id,
            location: AgentLocation::Direct,
            event,
        },
        metadata: Default::default(),
    }
}

#[tokio::test]
async fn committed_messages_are_forwarded_without_a_second_store_append() {
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let store = Arc::new(RecordingStore::new(agent_id));
    let inner = Arc::new(RecordingBus::default());
    let bus = StoreEventStreamBus::new(store.clone(), inner.clone());
    let committed = agent_envelope(
        session_id,
        agent_id,
        turn_id,
        AgentEvent::Message {
            message_seq: 1,
            message: ChatMessage::user("hello"),
        },
    );

    bus.publish(committed.clone()).await.expect("publish");

    assert_eq!(store.append_calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        *inner.published.lock().expect("published lock"),
        [committed]
    );
}

#[tokio::test]
async fn terminal_agent_event_is_persisted_before_forwarding() {
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let store = Arc::new(RecordingStore::new(agent_id));
    let inner = Arc::new(RecordingBus::default());
    let bus = StoreEventStreamBus::new(store.clone(), inner.clone());
    let usage = TokenUsage {
        input_tokens: 2,
        output_tokens: 3,
        total_tokens: 5,
    };

    bus.publish(agent_envelope(
        session_id,
        agent_id,
        turn_id,
        AgentEvent::Finished {
            finish_reason: "stop".to_owned(),
            usage,
        },
    ))
    .await
    .expect("publish");

    let state = store.load_agent().await.expect("load state");
    assert_eq!(state.status, AgentStatus::Finished);
    assert_eq!(state.session_id, Some(session_id));
    assert_eq!(state.turn_id, Some(turn_id));
    assert_eq!(state.usage, usage);
    assert_eq!(inner.published.lock().expect("published lock").len(), 1);
}

#[tokio::test]
async fn iteration_completion_advances_the_durable_frontier_before_forwarding() {
    let agent_id = AgentId::new();
    let session_id = SessionId::new();
    let turn_id = TurnId::new();
    let store = Arc::new(RecordingStore::new(agent_id));
    let inner = Arc::new(RecordingBus::default());
    let bus = StoreEventStreamBus::new(store.clone(), inner.clone());
    let usage = TokenUsage {
        input_tokens: 3,
        output_tokens: 5,
        total_tokens: 8,
    };

    bus.publish(agent_envelope(
        session_id,
        agent_id,
        turn_id,
        AgentEvent::IterationCompleted {
            iteration: 4,
            usage,
        },
    ))
    .await
    .expect("publish iteration completion");

    let state = store.load_agent().await.expect("load state");
    assert_eq!(state.next_iteration, 5);
    assert_eq!(state.usage, usage);
    assert_eq!(inner.published.lock().expect("published lock").len(), 1);
}

#[tokio::test]
async fn committed_event_remains_successful_when_retention_forwarding_fails() {
    let agent_id = AgentId::new();
    let store = Arc::new(RecordingStore::new(agent_id));
    let inner = Arc::new(RecordingBus {
        fail_publish: true,
        ..Default::default()
    });
    let bus = StoreEventStreamBus::new(store, inner);

    bus.publish(agent_envelope(
        SessionId::new(),
        agent_id,
        TurnId::new(),
        AgentEvent::Started,
    ))
    .await
    .expect("durably committed event tolerates retention failure");
}

#[tokio::test]
async fn session_events_propagate_inner_publish_errors() {
    let store = Arc::new(RecordingStore::new(AgentId::new()));
    let inner = Arc::new(RecordingBus {
        fail_publish: true,
        ..Default::default()
    });
    let bus = StoreEventStreamBus::new(store, inner);
    let envelope = StreamEnvelope {
        session_id: SessionId::new(),
        timestamp: Utc::now(),
        event: RuntimeEvent::Session {
            event: SessionEvent::Created,
        },
        metadata: Default::default(),
    };

    assert!(matches!(
        bus.publish(envelope).await,
        Err(EventStreamBusError::CursorOverflow)
    ));
}

#[tokio::test]
async fn subscription_delegates_the_session_scope() {
    let store = Arc::new(RecordingStore::new(AgentId::new()));
    let inner = Arc::new(RecordingBus::default());
    let bus = StoreEventStreamBus::new(store, inner.clone());
    let session_id = SessionId::new();

    let _stream = bus
        .subscribe_session(session_id, ReplayStart::New)
        .await
        .expect("subscribe");

    assert_eq!(
        *inner.subscriptions.lock().expect("subscriptions lock"),
        [(session_id, ReplayStart::New)]
    );
}
