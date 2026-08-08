//! Per-agent realtime dispatcher.
//!
//! One ordered dispatcher task per locally active Agent. Durable commit
//! receipts (from the sink adapter, the approval handler, the resolver, and
//! admission) carry only the committed high-water; the dispatcher scans the
//! committed rows from Postgres in `event_seq` order, skips internal rows,
//! and publishes product frames — so concurrent writers' wake order never
//! decides the publish order. Telemetry frames are published as they arrive:
//! the kernel emits a call's telemetry before committing its final assistant
//! message, and the single ordered command channel preserves that order.
//!
//! Publish failures are logged once and tolerated: Postgres is the recovery
//! truth and the frontend reconciles from the Agent view and history. When
//! NATS is down the dispatcher degrades to a no-op publisher. Dispatcher
//! tasks start at the current Postgres high-water, so restarts never
//! republish historical rows.

use std::collections::HashMap;
use std::sync::Mutex;

use bytes::Bytes;
use stratum_core::{AgentId, AgentTelemetryEvent, LlmCallId, SessionId, TurnId};
use stratum_infra::NatsAgentTail;
use stratum_postgres::PostgresBackend;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::frames::{AgentStreamFrameV1, ScannedRow, product_event};

/// Bounded per-dispatcher command queue.
const DISPATCHER_CHANNEL_CAPACITY: usize = 1024;

/// Command accepted by one Agent's dispatcher.
#[derive(Debug)]
pub(crate) enum DispatcherCommand {
    /// A durable commit advanced the Agent high-water.
    Durable {
        /// Highest committed event sequence known to the sender.
        high_water: u64,
    },
    /// One volatile LLM telemetry event of the active Turn.
    Telemetry {
        /// Bound Session.
        session_id: SessionId,
        /// Exact Turn.
        turn_id: TurnId,
        /// LLM call identity.
        llm_call_id: LlmCallId,
        /// Call-local telemetry sequence.
        telemetry_seq: u64,
        /// Typed telemetry payload.
        event: AgentTelemetryEvent,
    },
}

/// Sending half of one Agent's dispatcher.
#[derive(Debug, Clone)]
pub(crate) struct DispatcherHandle {
    agent_id: AgentId,
    tx: mpsc::Sender<DispatcherCommand>,
}

impl DispatcherHandle {
    /// Test-only stub over a raw channel.
    #[cfg(test)]
    pub(crate) fn stub(agent_id: AgentId) -> (Self, mpsc::Receiver<DispatcherCommand>) {
        let (tx, rx) = mpsc::channel(16);
        (Self { agent_id, tx }, rx)
    }

    /// Reports one committed durable high-water. The queue is bounded; a
    /// dropped receipt only delays realtime delivery, and Postgres reconcile
    /// converges the client.
    pub(crate) fn receipt(&self, high_water: u64) {
        if self
            .tx
            .try_send(DispatcherCommand::Durable { high_water })
            .is_err()
        {
            tracing::warn!(
                agent_id = %self.agent_id,
                "dispatcher queue is full; dropping a durable receipt"
            );
        }
    }

    /// Queues one telemetry event for publication.
    pub(crate) fn telemetry(
        &self,
        session_id: SessionId,
        turn_id: TurnId,
        llm_call_id: LlmCallId,
        telemetry_seq: u64,
        event: AgentTelemetryEvent,
    ) {
        if self
            .tx
            .try_send(DispatcherCommand::Telemetry {
                session_id,
                turn_id,
                llm_call_id,
                telemetry_seq,
                event,
            })
            .is_err()
        {
            tracing::warn!(
                agent_id = %self.agent_id,
                "dispatcher queue is full; dropping a telemetry event"
            );
        }
    }
}

/// Lazily creates and tracks the per-Agent dispatchers of this process.
pub(crate) struct DispatcherHub {
    handles: Mutex<HashMap<AgentId, DispatcherHandle>>,
    pg: PostgresBackend,
    tail: Option<NatsAgentTail>,
    shutdown: CancellationToken,
}

impl DispatcherHub {
    /// Creates the hub over the shared store and optional tail.
    #[must_use]
    pub(crate) fn new(
        pg: PostgresBackend,
        tail: Option<NatsAgentTail>,
        shutdown: CancellationToken,
    ) -> Self {
        Self {
            handles: Mutex::new(HashMap::new()),
            pg,
            tail,
            shutdown,
        }
    }

    /// Returns the dispatcher of a locally active Agent, creating it at the
    /// supplied frontier when absent.
    pub(crate) fn ensure(&self, agent_id: AgentId, frontier: u64) -> DispatcherHandle {
        let mut handles = self
            .handles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        handles
            .entry(agent_id)
            .or_insert_with(|| self.spawn(agent_id, frontier))
            .clone()
    }

    /// Reports a commit from a writer that may not hold a handle (approval
    /// resolver); a dispatcher created here starts at the receipt itself so
    /// exactly the committing row is scanned.
    pub(crate) fn receipt(&self, agent_id: AgentId, high_water: u64) {
        self.ensure(agent_id, high_water.saturating_sub(1))
            .receipt(high_water);
    }

    fn spawn(&self, agent_id: AgentId, frontier: u64) -> DispatcherHandle {
        let (tx, rx) = mpsc::channel(DISPATCHER_CHANNEL_CAPACITY);
        let io = PgNatsIo {
            agent_id,
            pg: self.pg.clone(),
            tail: self.tail.clone(),
        };
        let shutdown = self.shutdown.clone();
        tokio::spawn(run_dispatcher(agent_id, frontier, rx, io, shutdown));
        DispatcherHandle { agent_id, tx }
    }
}

/// IO boundary of the dispatcher; the second implementation lives in tests.
#[allow(async_fn_in_trait)] // single-call-site trait; dyn compatibility is not needed
pub(crate) trait DispatcherIo {
    /// Publishes one serialized frame to the Agent tail.
    async fn publish(&self, frame: Bytes) -> Result<(), DispatchIoError>;
    /// Reads committed rows in `(from, to]` in ascending order.
    async fn scan(
        &self,
        from_event_seq: u64,
        to_event_seq: u64,
    ) -> Result<Vec<ScannedRow>, DispatchIoError>;
}

/// Dispatcher IO failure; only its kind is ever logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DispatchIoError {
    /// The publish half failed.
    Publish,
    /// The scan half failed.
    Scan,
}

/// Production IO: Postgres scans plus the NATS tail, degrading to a no-op
/// publisher while NATS is down.
struct PgNatsIo {
    agent_id: AgentId,
    pg: PostgresBackend,
    tail: Option<NatsAgentTail>,
}

impl DispatcherIo for PgNatsIo {
    async fn publish(&self, frame: Bytes) -> Result<(), DispatchIoError> {
        match &self.tail {
            Some(tail) => tail
                .publish(&self.agent_id, frame)
                .await
                .map(|_| ())
                .map_err(|_| DispatchIoError::Publish),
            // Realtime is degraded: frames drop silently and clients
            // reconcile from Postgres.
            None => Ok(()),
        }
    }

    async fn scan(
        &self,
        from_event_seq: u64,
        to_event_seq: u64,
    ) -> Result<Vec<ScannedRow>, DispatchIoError> {
        let rows = self
            .pg
            .read_events_range(self.agent_id, from_event_seq, to_event_seq)
            .await
            .map_err(|_| DispatchIoError::Scan)?;
        Ok(rows.into_iter().map(ScannedRow::from).collect())
    }
}

/// One dispatcher's ordered publish loop.
async fn run_dispatcher<IO: DispatcherIo>(
    agent_id: AgentId,
    initial_frontier: u64,
    mut rx: mpsc::Receiver<DispatcherCommand>,
    io: IO,
    shutdown: CancellationToken,
) {
    let mut last_published = initial_frontier;
    loop {
        let command = tokio::select! {
            () = shutdown.cancelled() => break,
            command = rx.recv() => command,
        };
        match command {
            None => break,
            Some(DispatcherCommand::Telemetry {
                session_id,
                turn_id,
                llm_call_id,
                telemetry_seq,
                event,
            }) => {
                let frame = AgentStreamFrameV1::telemetry(
                    agent_id,
                    session_id,
                    turn_id,
                    llm_call_id,
                    telemetry_seq,
                    &event,
                );
                match frame.to_bytes() {
                    Ok(bytes) => {
                        if io.publish(bytes).await.is_err() {
                            tracing::warn!(
                                agent_id = %agent_id,
                                "realtime telemetry publish failed; clients recover via postgres"
                            );
                        }
                    }
                    Err(error) => {
                        tracing::error!(agent_id = %agent_id, error = %error, "telemetry frame failed to serialize");
                    }
                }
            }
            Some(DispatcherCommand::Durable { high_water }) => {
                if high_water <= last_published {
                    continue;
                }
                let rows = match io.scan(last_published, high_water).await {
                    Ok(rows) => rows,
                    Err(_) => {
                        // Keep the frontier: the next receipt rescans.
                        tracing::error!(
                            agent_id = %agent_id,
                            "durable scan failed; realtime delivery is delayed until the next receipt"
                        );
                        continue;
                    }
                };
                for row in rows {
                    if let Some(product) = product_event(&row.event) {
                        let frame =
                            AgentStreamFrameV1::durable(agent_id, &row, product, row.event_version);
                        match frame.to_bytes() {
                            Ok(bytes) => {
                                if io.publish(bytes).await.is_err() {
                                    tracing::warn!(
                                        agent_id = %agent_id,
                                        "realtime durable publish failed; clients recover via postgres"
                                    );
                                }
                            }
                            Err(error) => {
                                tracing::error!(agent_id = %agent_id, error = %error, "durable frame failed to serialize");
                            }
                        }
                    }
                    last_published = row.event_seq;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex as StdMutex};

    use stratum_core::{ChatMessage, DurableAgentEvent, TokenUsage};

    use super::*;

    #[derive(Default)]
    struct MockIo {
        rows: Vec<ScannedRow>,
        published: StdMutex<Vec<Bytes>>,
        fail_publishes: StdMutex<bool>,
    }

    impl MockIo {
        fn row(event_seq: u64, event: DurableAgentEvent) -> ScannedRow {
            ScannedRow {
                event_seq,
                event_version: 1,
                session_id: SessionId::new(),
                turn_id: TurnId::new(),
                created_at: chrono::Utc::now(),
                event,
            }
        }
    }

    impl DispatcherIo for Arc<MockIo> {
        async fn publish(&self, frame: Bytes) -> Result<(), DispatchIoError> {
            if *self.fail_publishes.lock().expect("lock") {
                return Err(DispatchIoError::Publish);
            }
            self.published.lock().expect("lock").push(frame);
            Ok(())
        }

        async fn scan(
            &self,
            from_event_seq: u64,
            to_event_seq: u64,
        ) -> Result<Vec<ScannedRow>, DispatchIoError> {
            Ok(self
                .rows
                .iter()
                .filter(|row| row.event_seq > from_event_seq && row.event_seq <= to_event_seq)
                .cloned()
                .collect())
        }
    }

    fn published_frames(io: &MockIo) -> Vec<serde_json::Value> {
        io.published
            .lock()
            .expect("lock")
            .iter()
            .map(|bytes| serde_json::from_slice(bytes).expect("frame is json"))
            .collect()
    }

    async fn drive(
        io: Arc<MockIo>,
        frontier: u64,
        commands: Vec<DispatcherCommand>,
    ) -> Arc<MockIo> {
        let (tx, rx) = mpsc::channel(16);
        let shutdown = CancellationToken::new();
        let task = {
            let io = Arc::clone(&io);
            tokio::spawn(run_dispatcher(AgentId::new(), frontier, rx, io, shutdown))
        };
        for command in commands {
            tx.send(command).await.expect("dispatcher alive");
        }
        drop(tx);
        task.await.expect("dispatcher finishes");
        io
    }

    #[tokio::test]
    async fn reversed_receipts_publish_in_event_seq_order() {
        let io = Arc::new(MockIo {
            rows: vec![
                MockIo::row(
                    10,
                    DurableAgentEvent::ToolApprovalResolved {
                        approval_id: stratum_core::ApprovalId::new(),
                        decision: stratum_core::ApprovalDecision::Approve,
                    },
                ),
                MockIo::row(
                    11,
                    DurableAgentEvent::MessageAppended {
                        message: ChatMessage::assistant("done"),
                    },
                ),
            ],
            ..MockIo::default()
        });

        let io = drive(
            io,
            9,
            vec![
                DispatcherCommand::Durable { high_water: 11 },
                DispatcherCommand::Durable { high_water: 10 },
            ],
        )
        .await;

        let frames = published_frames(&io);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0]["event_seq"], "10");
        assert_eq!(frames[1]["event_seq"], "11");
    }

    #[tokio::test]
    async fn telemetry_of_a_call_publishes_before_its_final_durable_message() {
        let io = Arc::new(MockIo {
            rows: vec![
                MockIo::row(
                    5,
                    DurableAgentEvent::MessageAppended {
                        message: ChatMessage::assistant("final"),
                    },
                ),
                MockIo::row(
                    6,
                    DurableAgentEvent::LoopFinished {
                        finish_reason: "stop".to_owned(),
                        usage: TokenUsage::default(),
                    },
                ),
            ],
            ..MockIo::default()
        });
        let call = LlmCallId::from("call-1");

        let io = drive(
            io,
            4,
            vec![
                DispatcherCommand::Telemetry {
                    session_id: SessionId::new(),
                    turn_id: TurnId::new(),
                    llm_call_id: call.clone(),
                    telemetry_seq: 0,
                    event: AgentTelemetryEvent::LlmStarted {
                        llm_call_id: call.clone(),
                    },
                },
                DispatcherCommand::Telemetry {
                    session_id: SessionId::new(),
                    turn_id: TurnId::new(),
                    llm_call_id: call.clone(),
                    telemetry_seq: 1,
                    event: AgentTelemetryEvent::TextDelta {
                        llm_call_id: call,
                        delta: "fin".to_owned(),
                    },
                },
                DispatcherCommand::Durable { high_water: 6 },
            ],
        )
        .await;

        let frames = published_frames(&io);
        let kinds: Vec<&str> = frames
            .iter()
            .filter_map(|frame| frame["kind"].as_str())
            .collect();
        assert_eq!(kinds, vec!["telemetry", "telemetry", "durable", "durable"]);
        assert_eq!(frames[2]["event_seq"], "5");
        assert_eq!(frames[3]["event"]["type"], "loop_finished");
    }

    #[tokio::test]
    async fn publish_failure_is_tolerated_and_the_frontier_advances() {
        let io = Arc::new(MockIo {
            rows: vec![
                MockIo::row(
                    3,
                    DurableAgentEvent::MessageAppended {
                        message: ChatMessage::user("hi"),
                    },
                ),
                MockIo::row(
                    4,
                    DurableAgentEvent::MessageAppended {
                        message: ChatMessage::assistant("hello"),
                    },
                ),
            ],
            ..MockIo::default()
        });
        *io.fail_publishes.lock().expect("lock") = true;

        let io = drive(io, 2, vec![DispatcherCommand::Durable { high_water: 4 }]).await;

        assert!(published_frames(&io).is_empty(), "all publishes failed");
        // A later receipt does not rescan published rows.
        let io = drive(io, 4, vec![]).await;
        assert!(published_frames(&io).is_empty());
    }

    #[tokio::test]
    async fn internal_rows_are_scanned_past_but_never_published() {
        let io = Arc::new(MockIo {
            rows: vec![
                MockIo::row(
                    7,
                    DurableAgentEvent::ToolExecutionStarted {
                        call_id: stratum_core::CallId::from("call-1"),
                        tool_name: stratum_core::ToolName::from("echo"),
                    },
                ),
                MockIo::row(
                    8,
                    DurableAgentEvent::MessageAppended {
                        message: ChatMessage::tool(
                            stratum_core::CallId::from("call-1"),
                            serde_json::json!({"ok": true}),
                        ),
                    },
                ),
            ],
            ..MockIo::default()
        });

        let io = drive(io, 6, vec![DispatcherCommand::Durable { high_water: 8 }]).await;

        let frames = published_frames(&io);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0]["event_seq"], "8");
    }
}
