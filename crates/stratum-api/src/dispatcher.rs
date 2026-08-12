//! Per-AgentRuntime realtime dispatcher.
//!
//! One ordered dispatcher task per locally active AgentRuntime. Durable commit
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
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bytes::Bytes;
use stratum_core::{AgentId, AgentRuntimeId, AgentTelemetryEvent, SessionId, TurnId};
use stratum_infra::NatsAgentRuntimeTail;
use stratum_postgres::{PostgresBackend, PostgresError};
use tokio::sync::{Semaphore, mpsc};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::{DispatchError, PersistedVariantError};
use crate::frames::{
    AgentRuntimeProductEventV1, AgentRuntimeStreamFrameV1, ScannedRow, product_event,
};
use crate::state::{RuntimeTasks, spawn_runtime_task};

/// Bounded per-dispatcher command queue.
const DISPATCHER_CHANNEL_CAPACITY: usize = 1024;

/// Idle publish attempts tolerated after every producer handle has gone away.
/// The durable ledger remains authoritative; abandoning this volatile
/// generation lets a later writer start from a fresh committed PG barrier.
const MAX_IDLE_PUBLISH_FAILURES: usize = 3;

/// Command accepted by one AgentRuntime's dispatcher.
#[derive(Debug)]
pub(crate) enum DispatcherCommand {
    /// Publishes committed durable rows through this receipt's fixed target.
    DurableWake {
        /// Committed AgentRuntime-wide high-water observed by this receipt.
        through: u64,
    },
    /// One volatile LLM telemetry event of the active Turn.
    Telemetry {
        /// Durable high-water observed before this telemetry was enqueued.
        durable_before: u64,
        /// Bound Session.
        session_id: SessionId,
        /// Exact Turn.
        turn_id: TurnId,
        /// Call-local telemetry sequence.
        telemetry_seq: u64,
        /// Typed telemetry payload.
        event: AgentTelemetryEvent,
    },
}

/// Sending half of one AgentRuntime's dispatcher.
#[derive(Debug, Clone)]
pub(crate) struct DispatcherHandle {
    agent_runtime_id: AgentRuntimeId,
    agent_id: AgentId,
    tx: mpsc::Sender<DispatcherCommand>,
    durable_target: Arc<AtomicU64>,
}

impl DispatcherHandle {
    /// Reports one committed durable high-water without making the Postgres
    /// acknowledgement wait for realtime transport capacity. A full queue may
    /// coalesce this wake into `durable_target`; the dispatcher reloads that
    /// target whenever its accepted command queue drains and before retiring.
    pub(crate) fn receipt(&self, high_water: u64) {
        self.durable_target.fetch_max(high_water, Ordering::Release);
        match self.tx.try_send(DispatcherCommand::DurableWake {
            through: high_water,
        }) {
            Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
            Err(mpsc::error::TrySendError::Closed(_)) => {
                tracing::warn!(
                    agent_runtime_id = %self.agent_runtime_id,
                    agent_id = %self.agent_id,
                    "dispatcher is closed"
                );
            }
        }
    }

    /// Queues one telemetry event for publication.
    pub(crate) fn telemetry(
        &self,
        session_id: SessionId,
        turn_id: TurnId,
        telemetry_seq: u64,
        event: AgentTelemetryEvent,
    ) {
        let durable_before = self.durable_target.load(Ordering::Acquire);
        match self.tx.try_send(DispatcherCommand::Telemetry {
            durable_before,
            session_id,
            turn_id,
            telemetry_seq,
            event,
        }) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => tracing::warn!(
                agent_runtime_id = %self.agent_runtime_id,
                agent_id = %self.agent_id,
                "dispatcher queue is full; dropping a telemetry event"
            ),
            Err(mpsc::error::TrySendError::Closed(_)) => tracing::warn!(
                agent_runtime_id = %self.agent_runtime_id,
                agent_id = %self.agent_id,
                "dispatcher is closed; dropping a telemetry event"
            ),
        }
    }
}

/// One live dispatcher registration. The slot's strong sender serializes all
/// receipts for the AgentRuntime; the task retires it after a configured idle
/// period only when no external handle exists.
struct DispatcherEntry {
    generation: Uuid,
    agent_id: AgentId,
    tx: mpsc::Sender<DispatcherCommand>,
    durable_target: Arc<AtomicU64>,
}

/// Stable per-runtime gate. `ensure` holds this gate while it reads the
/// committed PG barrier and installs a generation, so two concurrent callers
/// cannot start overlapping dispatchers.
struct RuntimeSlot {
    /// Async initialization/retirement gate. A permit may span the committed
    /// PG read; the entry mutex below never spans an await.
    gate: Semaphore,
    entry: Mutex<Option<DispatcherEntry>>,
}

type RuntimeSlots = Arc<Mutex<HashMap<AgentRuntimeId, Arc<RuntimeSlot>>>>;

/// Lazily creates the per-AgentRuntime dispatchers of this process. Tasks are owned
/// by the process `JoinSet`; an idle retirement handshake breaks the map
/// sender / task receiver lifecycle without allowing two live dispatchers.
pub(crate) struct DispatcherHub {
    slots: RuntimeSlots,
    pg: PostgresBackend,
    tail: Option<NatsAgentRuntimeTail>,
    shutdown: CancellationToken,
    tasks: RuntimeTasks,
    idle_timeout: Duration,
}

impl DispatcherHub {
    /// Creates the hub over the shared store and optional tail.
    #[must_use]
    pub(crate) fn new(
        pg: PostgresBackend,
        tail: Option<NatsAgentRuntimeTail>,
        shutdown: CancellationToken,
        tasks: RuntimeTasks,
        idle_timeout: Duration,
    ) -> Self {
        Self {
            slots: Arc::new(Mutex::new(HashMap::new())),
            pg,
            tail,
            shutdown,
            tasks,
            idle_timeout,
        }
    }

    /// Returns the dispatcher of a locally active AgentRuntime. When absent,
    /// the per-runtime gate linearizes a committed PG state read with
    /// generation installation. Callers therefore never supply a frontier.
    ///
    /// This method performs only the PG read and local registration; it never
    /// waits for NATS publication.
    ///
    /// # Errors
    ///
    /// Returns the typed Postgres error when the runtime state cannot be read.
    pub(crate) async fn ensure(
        &self,
        agent_runtime_id: AgentRuntimeId,
    ) -> Result<DispatcherHandle, PostgresError> {
        let slot = {
            let mut slots = self
                .slots
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Arc::clone(slots.entry(agent_runtime_id).or_insert_with(|| {
                Arc::new(RuntimeSlot {
                    gate: Semaphore::new(1),
                    entry: Mutex::new(None),
                })
            }))
        };
        // INVARIANT: RuntimeSlot::gate is private to this module and is never
        // closed, so acquisition failure is a programmer error.
        let _permit = slot
            .gate
            .acquire()
            .await
            .expect("runtime dispatcher gate is never closed");
        {
            let entry = slot
                .entry
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(entry) = entry.as_ref() {
                return Ok(DispatcherHandle {
                    agent_runtime_id,
                    agent_id: entry.agent_id,
                    tx: entry.tx.clone(),
                    durable_target: Arc::clone(&entry.durable_target),
                });
            }
        }

        let state = self.pg.read_agent_runtime_state(agent_runtime_id).await?;
        let (registration, handle) = self.spawn(
            agent_runtime_id,
            state.agent_id,
            state.last_event_seq,
            Arc::clone(&slot),
        );
        *slot
            .entry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(registration);
        Ok(handle)
    }

    /// Clears the strong registrations after the process-owned task set drains.
    pub(crate) fn clear(&self) {
        self.slots
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    fn spawn(
        &self,
        agent_runtime_id: AgentRuntimeId,
        agent_id: AgentId,
        frontier: u64,
        slot: Arc<RuntimeSlot>,
    ) -> (DispatcherEntry, DispatcherHandle) {
        let (tx, rx) = mpsc::channel(DISPATCHER_CHANNEL_CAPACITY);
        let durable_target = Arc::new(AtomicU64::new(frontier));
        let io = PgNatsIo {
            agent_runtime_id,
            pg: self.pg.clone(),
            tail: self.tail.clone(),
        };
        let shutdown = self.shutdown.clone();
        let slots = Arc::clone(&self.slots);
        let generation = Uuid::now_v7();
        let idle_timeout = self.idle_timeout;
        let task_durable_target = Arc::clone(&durable_target);
        spawn_runtime_task(&self.tasks, async move {
            run_dispatcher(
                frontier,
                rx,
                io,
                shutdown,
                DispatcherTaskContext {
                    agent_runtime_id,
                    agent_id,
                    slots,
                    slot,
                    generation,
                    idle_timeout,
                    durable_target: task_durable_target,
                },
            )
            .await;
        });
        let entry = DispatcherEntry {
            generation,
            agent_id,
            tx: tx.clone(),
            durable_target: Arc::clone(&durable_target),
        };
        (
            entry,
            DispatcherHandle {
                agent_runtime_id,
                agent_id,
                tx,
                durable_target,
            },
        )
    }
}

/// Production IO: Postgres scans plus the NATS tail, degrading to a no-op
/// publisher while NATS is down.
struct PgNatsIo {
    agent_runtime_id: AgentRuntimeId,
    pg: PostgresBackend,
    tail: Option<NatsAgentRuntimeTail>,
}

impl PgNatsIo {
    async fn publish(&self, frame: Bytes) -> Result<(), DispatchError> {
        match &self.tail {
            Some(tail) => tail
                .publish(&self.agent_runtime_id, frame)
                .await
                .map(|_| ())
                .map_err(DispatchError::Publish),
            // Realtime is degraded: frames drop silently and clients
            // reconcile from Postgres.
            None => Ok(()),
        }
    }

    async fn scan(
        &self,
        from_event_seq: u64,
        to_event_seq: u64,
    ) -> Result<Vec<ScannedRow>, DispatchError> {
        let rows = self
            .pg
            .read_events_range(self.agent_runtime_id, from_event_seq, to_event_seq)
            .await
            .map_err(DispatchError::Scan)?;
        Ok(rows.into_iter().map(ScannedRow::from).collect())
    }
}

/// Lifecycle and ordering context owned by one dispatcher task.
struct DispatcherTaskContext {
    agent_runtime_id: AgentRuntimeId,
    agent_id: AgentId,
    slots: RuntimeSlots,
    slot: Arc<RuntimeSlot>,
    generation: Uuid,
    idle_timeout: Duration,
    durable_target: Arc<AtomicU64>,
}

/// One dispatcher's ordered publish loop.
async fn run_dispatcher(
    initial_frontier: u64,
    mut rx: mpsc::Receiver<DispatcherCommand>,
    io: PgNatsIo,
    shutdown: CancellationToken,
    context: DispatcherTaskContext,
) {
    let mut frontier = DurableFrontier(initial_frontier);
    let mut idle_publish_failures = 0_usize;
    loop {
        let command = tokio::select! {
            () = shutdown.cancelled() => break,
            command = rx.recv() => command,
            () = tokio::time::sleep(context.idle_timeout) => {
                flush_coalesced_target(&context, &rx, &io, &mut frontier).await;
                let target = context.durable_target.load(Ordering::Acquire);
                if frontier.is_caught_up(target) {
                    idle_publish_failures = 0;
                    if retire_idle_dispatcher(&context, &mut rx).await {
                        prune_empty_slot(&context).await;
                        return;
                    }
                } else if rx.is_empty()
                    && generation_has_no_external_handles(&context).await
                {
                    if record_idle_publish_failure(&mut idle_publish_failures)
                        && retire_idle_dispatcher(&context, &mut rx).await
                    {
                        tracing::warn!(
                            agent_runtime_id = %context.agent_runtime_id,
                            agent_id = %context.agent_id,
                            unpublished_through_event_seq = target,
                            "abandoning an idle realtime generation after bounded publish failures; postgres remains authoritative"
                        );
                        prune_empty_slot(&context).await;
                        return;
                    }
                } else {
                    idle_publish_failures = 0;
                }
                continue;
            }
        };
        match command {
            None => break,
            Some(DispatcherCommand::DurableWake { through }) => {
                flush_durable(
                    context.agent_runtime_id,
                    context.agent_id,
                    &io,
                    through,
                    &mut frontier,
                )
                .await;
            }
            Some(DispatcherCommand::Telemetry {
                durable_before,
                session_id,
                turn_id,
                telemetry_seq,
                event,
            }) => {
                let durable_ready = flush_durable(
                    context.agent_runtime_id,
                    context.agent_id,
                    &io,
                    durable_before,
                    &mut frontier,
                )
                .await;
                if !durable_ready {
                    tracing::warn!(
                        agent_runtime_id = %context.agent_runtime_id,
                        agent_id = %context.agent_id,
                        "telemetry was suppressed behind an unpublished durable event"
                    );
                } else {
                    let frame = AgentRuntimeStreamFrameV1::telemetry(
                        context.agent_runtime_id,
                        context.agent_id,
                        session_id,
                        turn_id,
                        durable_before,
                        telemetry_seq,
                        &event,
                    );
                    if let Some(frame) = frame {
                        match frame.to_bytes() {
                            Ok(bytes) => {
                                if let Err(error) = io.publish(bytes).await {
                                    tracing::warn!(
                                        agent_runtime_id = %context.agent_runtime_id,
                                        agent_id = %context.agent_id,
                                        error = %error,
                                        "realtime telemetry publish failed; clients recover via postgres"
                                    );
                                }
                            }
                            Err(error) => {
                                tracing::error!(
                                    agent_runtime_id = %context.agent_runtime_id,
                                    agent_id = %context.agent_id,
                                    error = %error,
                                    "telemetry frame failed to serialize"
                                );
                            }
                        }
                    } else {
                        tracing::warn!(
                            agent_runtime_id = %context.agent_runtime_id,
                            agent_id = %context.agent_id,
                            "unsupported telemetry event was omitted from the v1 stream"
                        );
                    }
                }
            }
        }
        flush_coalesced_target(&context, &rx, &io, &mut frontier).await;
    }
    unregister_generation(&context, &mut rx).await;
    prune_empty_slot(&context).await;
}

/// Records one idle retry that still could not reach the durable target and
/// reports when the bounded abandonment threshold has been reached.
fn record_idle_publish_failure(failures: &mut usize) -> bool {
    *failures = failures.saturating_add(1);
    *failures >= MAX_IDLE_PUBLISH_FAILURES
}

/// Flushes a receipt that was coalesced into the atomic target while the
/// bounded command queue was full. Waiting until the queue drains preserves
/// the fixed telemetry-before-final ordering of every accepted command.
async fn flush_coalesced_target(
    context: &DispatcherTaskContext,
    rx: &mpsc::Receiver<DispatcherCommand>,
    io: &PgNatsIo,
    frontier: &mut DurableFrontier,
) {
    if let Some(target) = coalesced_target_after_drain(rx, &context.durable_target, *frontier) {
        flush_durable(
            context.agent_runtime_id,
            context.agent_id,
            io,
            target,
            frontier,
        )
        .await;
    }
}

fn coalesced_target_after_drain(
    rx: &mpsc::Receiver<DispatcherCommand>,
    durable_target: &AtomicU64,
    frontier: DurableFrontier,
) -> Option<u64> {
    // Snapshot first. If telemetry is accepted after this load, flushing only
    // this older target cannot overtake it. Conversely, observing a later
    // assistant-final target implies that call's synchronous telemetry enqueue
    // already completed, so the subsequent empty check sees its command.
    let target = durable_target.load(Ordering::Acquire);
    drained_target(rx, target, frontier)
}

fn drained_target(
    rx: &mpsc::Receiver<DispatcherCommand>,
    observed_target: u64,
    frontier: DurableFrontier,
) -> Option<u64> {
    if !rx.is_empty() {
        return None;
    }
    (!frontier.is_caught_up(observed_target)).then_some(observed_target)
}

/// Flushes every durable row through `target` before volatile telemetry may
/// pass. A product serialization or publish failure leaves the frontier
/// immediately before that row, so the next wake retries it and later
/// telemetry cannot overtake it. Duplicate delivery after an ambiguous NATS
/// acknowledgement is safe because durable `event_seq` is the client dedupe
/// identity.
async fn flush_durable(
    agent_runtime_id: AgentRuntimeId,
    agent_id: AgentId,
    io: &PgNatsIo,
    target: u64,
    frontier: &mut DurableFrontier,
) -> bool {
    if frontier.is_caught_up(target) {
        return true;
    }
    let rows = match io.scan(frontier.sequence(), target).await {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!(
                agent_runtime_id = %agent_runtime_id,
                agent_id = %agent_id,
                error = %error,
                "durable scan failed; realtime delivery is delayed until the next wake"
            );
            return false;
        }
    };
    publish_scanned_rows(
        agent_runtime_id,
        agent_id,
        rows,
        target,
        frontier,
        |bytes| io.publish(bytes),
    )
    .await
}

async fn publish_scanned_rows<F, Fut>(
    agent_runtime_id: AgentRuntimeId,
    agent_id: AgentId,
    rows: Vec<ScannedRow>,
    target: u64,
    frontier: &mut DurableFrontier,
    mut publish: F,
) -> bool
where
    F: FnMut(Bytes) -> Fut,
    Fut: Future<Output = Result<(), DispatchError>>,
{
    for row in rows {
        let Some(expected_event_seq) = frontier.sequence().checked_add(1) else {
            tracing::error!(
                agent_runtime_id = %agent_runtime_id,
                agent_id = %agent_id,
                "durable frontier overflowed; realtime delivery is halted"
            );
            return false;
        };
        if row.event_seq != expected_event_seq || row.event_seq > target {
            tracing::error!(
                agent_runtime_id = %agent_runtime_id,
                agent_id = %agent_id,
                expected_event_seq,
                actual_event_seq = row.event_seq,
                target_event_seq = target,
                "durable scan returned a non-contiguous row; realtime delivery is halted"
            );
            return false;
        }
        let product =
            match apply_product_projection(row.event_seq, product_event(&row.event), frontier) {
                Ok(product) => product,
                Err(error) => {
                    tracing::error!(
                        agent_runtime_id = %agent_runtime_id,
                        agent_id = %agent_id,
                        event_seq = row.event_seq,
                        error = %error,
                        "durable product projection failed; realtime delivery is halted"
                    );
                    return false;
                }
            };
        if let Some(product) = product {
            let frame = AgentRuntimeStreamFrameV1::durable(
                agent_runtime_id,
                agent_id,
                &row,
                product,
                row.event_version,
            );
            let bytes = match frame.to_bytes() {
                Ok(bytes) => bytes,
                Err(error) => {
                    tracing::error!(
                        agent_runtime_id = %agent_runtime_id,
                        agent_id = %agent_id,
                        event_seq = row.event_seq,
                        error = %error,
                        "durable frame failed to serialize"
                    );
                    return false;
                }
            };
            if let Err(error) = frontier.complete_product(row.event_seq, publish(bytes).await) {
                tracing::warn!(
                    agent_runtime_id = %agent_runtime_id,
                    agent_id = %agent_id,
                    event_seq = row.event_seq,
                    error = %error,
                    "realtime durable publish failed; later telemetry remains blocked"
                );
                return false;
            }
        }
    }
    frontier.is_caught_up(target)
}

/// Applies the explicit product/internal classification of one contiguous
/// durable row. Only a known internal event advances without a publish; a
/// projection error preserves the prior frontier for retry and blocks later
/// telemetry.
fn apply_product_projection(
    event_seq: u64,
    projection: Result<Option<AgentRuntimeProductEventV1>, PersistedVariantError>,
    frontier: &mut DurableFrontier,
) -> Result<Option<AgentRuntimeProductEventV1>, DispatchError> {
    match projection.map_err(DispatchError::Projection)? {
        Some(product) => Ok(Some(product)),
        None => {
            frontier.advance_internal(event_seq);
            Ok(None)
        }
    }
}

/// Published durable frontier. Product rows advance only after a successful
/// NATS acknowledgement; internal rows advance after scanning because they
/// deliberately have no realtime projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DurableFrontier(u64);

impl DurableFrontier {
    const fn sequence(self) -> u64 {
        self.0
    }

    const fn is_caught_up(self, target: u64) -> bool {
        self.0 >= target
    }

    fn complete_product<E>(&mut self, event_seq: u64, result: Result<(), E>) -> Result<(), E> {
        result?;
        self.0 = event_seq;
        Ok(())
    }

    fn advance_internal(&mut self, event_seq: u64) {
        self.0 = event_seq;
    }
}

/// Reports whether this generation has no producer handle outside its slot.
async fn generation_has_no_external_handles(context: &DispatcherTaskContext) -> bool {
    let Ok(_permit) = context.slot.gate.acquire().await else {
        return false;
    };
    let entry = context
        .slot
        .entry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    entry
        .as_ref()
        .is_some_and(|entry| entry.generation == context.generation && entry.tx.strong_count() == 1)
}

/// Atomically unregisters an idle generation. The per-runtime gate excludes a
/// concurrent `ensure` while the generation and live-handle count are checked.
/// Already accepted commands must have drained, otherwise an idle timer could
/// win `select!` over a ready receiver and strand the command.
async fn retire_idle_dispatcher(
    context: &DispatcherTaskContext,
    rx: &mut mpsc::Receiver<DispatcherCommand>,
) -> bool {
    let Ok(_permit) = context.slot.gate.acquire().await else {
        return false;
    };
    let mut entry = context
        .slot
        .entry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let can_retire = entry.as_ref().is_some_and(|entry| {
        entry.generation == context.generation && entry.tx.strong_count() == 1 && rx.is_empty()
    });
    if can_retire {
        *entry = None;
        rx.close();
    }
    can_retire
}

/// Removes this generation during shutdown or receiver closure, without
/// touching a replacement that may already have been installed.
async fn unregister_generation(
    context: &DispatcherTaskContext,
    rx: &mut mpsc::Receiver<DispatcherCommand>,
) {
    let Ok(_permit) = context.slot.gate.acquire().await else {
        return;
    };
    let mut entry = context
        .slot
        .entry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if entry
        .as_ref()
        .is_some_and(|entry| entry.generation == context.generation)
    {
        *entry = None;
        rx.close();
    }
}

/// Prunes an empty gate only when nobody can be waiting to install through
/// that exact `Arc`. Holding both the gate and outer map lock makes removal
/// linear with a concurrent slot lookup.
async fn prune_empty_slot(context: &DispatcherTaskContext) {
    let Ok(_permit) = context.slot.gate.acquire().await else {
        return;
    };
    let entry = context
        .slot
        .entry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if entry.is_some() {
        return;
    }
    let mut slots = context
        .slots
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let owns_map_entry = slots
        .get(&context.agent_runtime_id)
        .is_some_and(|slot| Arc::ptr_eq(slot, &context.slot));
    // One strong reference is held by the map and one by this task context.
    // Any additional reference belongs to an `ensure` caller that must retain
    // the same gate until it has checked or installed a generation.
    if owns_map_entry && Arc::strong_count(&context.slot) == 2 {
        slots.remove(&context.agent_runtime_id);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub(crate) fn stub_handle(
        agent_runtime_id: AgentRuntimeId,
        agent_id: AgentId,
    ) -> (DispatcherHandle, mpsc::Receiver<DispatcherCommand>) {
        let (tx, rx) = mpsc::channel(16);
        (
            DispatcherHandle {
                agent_runtime_id,
                agent_id,
                tx,
                durable_target: Arc::new(AtomicU64::new(0)),
            },
            rx,
        )
    }
}

#[cfg(test)]
mod tests {
    use stratum_core::{ChatMessage, DurableAgentEvent, LlmCallId};
    use stratum_infra::AgentRuntimeTailError;

    use super::*;

    #[tokio::test]
    async fn idle_retirement_waits_for_external_handles_and_closes_atomically() {
        let (tx, mut rx) = mpsc::channel::<DispatcherCommand>(1);
        let agent_runtime_id = AgentRuntimeId::new();
        let agent_id = AgentId::new();
        let generation = Uuid::now_v7();
        let slot = Arc::new(RuntimeSlot {
            gate: Semaphore::new(1),
            entry: Mutex::new(Some(DispatcherEntry {
                generation,
                agent_id,
                tx: tx.clone(),
                durable_target: Arc::new(AtomicU64::new(0)),
            })),
        });
        let context = test_context(agent_runtime_id, agent_id, generation, Arc::clone(&slot));

        assert!(!retire_idle_dispatcher(&context, &mut rx).await);
        drop(tx);
        assert!(retire_idle_dispatcher(&context, &mut rx).await);
        assert!(rx.recv().await.is_none());
        assert!(
            slot.entry
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_none()
        );
    }

    #[tokio::test]
    async fn idle_retirement_consumes_an_accepted_command_before_retiring() {
        let (tx, mut rx) = mpsc::channel::<DispatcherCommand>(1);
        let agent_runtime_id = AgentRuntimeId::new();
        let agent_id = AgentId::new();
        let generation = Uuid::now_v7();
        let slot = Arc::new(RuntimeSlot {
            gate: Semaphore::new(1),
            entry: Mutex::new(Some(DispatcherEntry {
                generation,
                agent_id,
                tx: tx.clone(),
                durable_target: Arc::new(AtomicU64::new(0)),
            })),
        });
        let context = test_context(agent_runtime_id, agent_id, generation, slot);
        tx.try_send(DispatcherCommand::DurableWake { through: 1 })
            .expect("queue has one slot");
        drop(tx);

        assert!(!retire_idle_dispatcher(&context, &mut rx).await);
        assert!(matches!(
            rx.recv().await,
            Some(DispatcherCommand::DurableWake { through: 1 })
        ));
        assert!(retire_idle_dispatcher(&context, &mut rx).await);
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn retirement_gate_serializes_replacement_and_old_cleanup_cannot_remove_it() {
        let (tx, mut rx) = mpsc::channel::<DispatcherCommand>(1);
        let agent_runtime_id = AgentRuntimeId::new();
        let agent_id = AgentId::new();
        let old_generation = Uuid::now_v7();
        let slot = Arc::new(RuntimeSlot {
            gate: Semaphore::new(1),
            entry: Mutex::new(Some(DispatcherEntry {
                generation: old_generation,
                agent_id,
                tx: tx.clone(),
                durable_target: Arc::new(AtomicU64::new(0)),
            })),
        });
        let context = Arc::new(test_context(
            agent_runtime_id,
            agent_id,
            old_generation,
            Arc::clone(&slot),
        ));
        drop(tx);

        let permit = slot.gate.acquire().await.expect("test gate stays open");
        let task_context = Arc::clone(&context);
        let retirement = tokio::spawn(async move {
            let retired = retire_idle_dispatcher(&task_context, &mut rx).await;
            (retired, rx)
        });
        tokio::task::yield_now().await;
        assert!(
            !retirement.is_finished(),
            "retirement waits for the same per-runtime gate"
        );
        drop(permit);
        let (retired, mut old_rx) = retirement.await.expect("retirement task joins");
        assert!(retired);
        assert!(old_rx.recv().await.is_none());

        let new_generation = Uuid::now_v7();
        let (new_tx, _new_rx) = mpsc::channel(1);
        let replacement_permit = slot.gate.acquire().await.expect("test gate stays open");
        *slot
            .entry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(DispatcherEntry {
            generation: new_generation,
            agent_id,
            tx: new_tx,
            durable_target: Arc::new(AtomicU64::new(0)),
        });
        drop(replacement_permit);

        unregister_generation(&context, &mut old_rx).await;
        let entry = slot
            .entry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(
            entry.as_ref().map(|entry| entry.generation),
            Some(new_generation),
            "old generation cleanup cannot remove its replacement"
        );
    }

    #[test]
    fn idle_publish_failure_retries_are_bounded_before_abandonment() {
        let mut failures = 0;

        for _ in 1..MAX_IDLE_PUBLISH_FAILURES {
            assert!(!record_idle_publish_failure(&mut failures));
        }
        assert!(record_idle_publish_failure(&mut failures));
        assert_eq!(failures, MAX_IDLE_PUBLISH_FAILURES);
        assert!(
            record_idle_publish_failure(&mut failures),
            "the saturated threshold remains eligible for abandonment"
        );
    }

    #[test]
    fn failed_durable_publish_keeps_later_telemetry_blocked() {
        let mut frontier = DurableFrontier(4);

        let result = frontier.complete_product(5, Err::<(), _>("nats unavailable"));

        assert!(result.is_err());
        assert_eq!(frontier.sequence(), 4);
        assert!(!frontier.is_caught_up(5));
    }

    #[test]
    fn unsupported_product_projection_does_not_advance_the_durable_frontier() {
        let mut frontier = DurableFrontier(4);

        let error = apply_product_projection(
            5,
            Err(PersistedVariantError::UnsupportedDurableProductEvent),
            &mut frontier,
        )
        .expect_err("unsupported persisted variant fails closed");

        assert!(matches!(
            error,
            DispatchError::Projection(PersistedVariantError::UnsupportedDurableProductEvent)
        ));
        assert_eq!(frontier.sequence(), 4);
        assert!(!frontier.is_caught_up(5));
    }

    #[tokio::test]
    async fn async_flush_failure_blocks_telemetry_until_the_durable_row_retries() {
        let agent_runtime_id = AgentRuntimeId::new();
        let agent_id = AgentId::new();
        let row = ScannedRow {
            event_seq: 5,
            event_version: 1,
            session_id: SessionId::new(),
            turn_id: TurnId::new(),
            created_at: chrono::Utc::now(),
            event: DurableAgentEvent::MessageAppended {
                message: ChatMessage::assistant("old final"),
            },
        };
        let mut frontier = DurableFrontier(4);

        let ready = publish_scanned_rows(
            agent_runtime_id,
            agent_id,
            vec![row.clone()],
            5,
            &mut frontier,
            |_| async {
                Err(DispatchError::Publish(AgentRuntimeTailError::Nats {
                    source: Box::new(std::io::Error::other("nats unavailable")),
                }))
            },
        )
        .await;

        assert!(!ready, "run_dispatcher must suppress the next telemetry");
        assert_eq!(frontier.sequence(), 4);

        let ready = publish_scanned_rows(
            agent_runtime_id,
            agent_id,
            vec![row],
            5,
            &mut frontier,
            |_| async { Ok(()) },
        )
        .await;
        assert!(ready);
        assert_eq!(frontier.sequence(), 5);
    }

    #[tokio::test]
    async fn telemetry_keeps_the_durable_barrier_from_its_enqueue_position() {
        let (handle, mut rx) = test_support::stub_handle(AgentRuntimeId::new(), AgentId::new());
        handle.durable_target.store(4, Ordering::Release);
        handle.telemetry(
            SessionId::new(),
            TurnId::new(),
            0,
            AgentTelemetryEvent::LlmStarted {
                llm_call_id: LlmCallId::from("call-1"),
            },
        );
        handle.receipt(5);

        assert!(matches!(
            rx.recv().await,
            Some(DispatcherCommand::Telemetry {
                durable_before: 4,
                ..
            })
        ));
        assert!(matches!(
            rx.recv().await,
            Some(DispatcherCommand::DurableWake { through: 5 })
        ));
    }

    #[tokio::test]
    async fn durable_scan_gap_does_not_advance_the_frontier() {
        let agent_runtime_id = AgentRuntimeId::new();
        let agent_id = AgentId::new();
        let mut frontier = DurableFrontier(4);

        let ready = publish_scanned_rows(
            agent_runtime_id,
            agent_id,
            vec![internal_row(6)],
            6,
            &mut frontier,
            |_| async { Ok(()) },
        )
        .await;

        assert!(!ready);
        assert_eq!(frontier.sequence(), 4);
    }

    #[tokio::test]
    async fn out_of_order_durable_scan_stops_before_the_first_hole() {
        let agent_runtime_id = AgentRuntimeId::new();
        let agent_id = AgentId::new();
        let mut frontier = DurableFrontier(4);

        let ready = publish_scanned_rows(
            agent_runtime_id,
            agent_id,
            vec![internal_row(5), internal_row(7)],
            7,
            &mut frontier,
            |_| async { Ok(()) },
        )
        .await;

        assert!(!ready);
        assert_eq!(frontier.sequence(), 5);
    }

    #[tokio::test]
    async fn full_queue_coalesces_the_last_durable_wake_until_drain() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.try_send(DispatcherCommand::DurableWake { through: 3 })
            .expect("queue has one slot");
        let handle = DispatcherHandle {
            agent_runtime_id: AgentRuntimeId::new(),
            agent_id: AgentId::new(),
            tx,
            durable_target: Arc::new(AtomicU64::new(3)),
        };

        handle.receipt(9);
        assert_eq!(handle.durable_target.load(Ordering::Acquire), 9);
        assert_eq!(rx.len(), 1, "the full queue cannot accept a second wake");
        assert_eq!(
            coalesced_target_after_drain(&rx, handle.durable_target.as_ref(), DurableFrontier(3)),
            None,
            "fixed queued commands must drain before the coalesced target"
        );
        assert!(matches!(
            rx.recv().await,
            Some(DispatcherCommand::DurableWake { through: 3 })
        ));
        assert_eq!(
            coalesced_target_after_drain(&rx, handle.durable_target.as_ref(), DurableFrontier(3)),
            Some(9),
            "the worker reloads the dropped wake once the queue drains"
        );
    }

    #[tokio::test]
    async fn coalesced_flush_never_loads_a_future_final_after_declaring_drain() {
        let (tx, mut rx) = mpsc::channel(2);
        let durable_target = AtomicU64::new(4);
        let observed_before_telemetry = durable_target.load(Ordering::Acquire);

        tx.try_send(DispatcherCommand::Telemetry {
            durable_before: 4,
            session_id: SessionId::new(),
            turn_id: TurnId::new(),
            telemetry_seq: 0,
            event: AgentTelemetryEvent::LlmStarted {
                llm_call_id: LlmCallId::from("call-1"),
            },
        })
        .expect("telemetry enters after the worker snapshots the old target");
        durable_target.store(5, Ordering::Release);

        assert_eq!(
            drained_target(&rx, observed_before_telemetry, DurableFrontier(4)),
            None,
            "the queued telemetry is consumed before any coalesced final"
        );
        assert!(matches!(
            rx.recv().await,
            Some(DispatcherCommand::Telemetry {
                durable_before: 4,
                ..
            })
        ));
        assert_eq!(
            coalesced_target_after_drain(&rx, &durable_target, DurableFrontier(4)),
            Some(5),
            "only the post-telemetry drain may observe the final target"
        );
    }

    #[test]
    fn durable_product_projection_preserves_row_order_and_omits_internal_rows() {
        let rows = [
            ScannedRow {
                event_seq: 7,
                event_version: 1,
                session_id: SessionId::new(),
                turn_id: TurnId::new(),
                created_at: chrono::Utc::now(),
                event: DurableAgentEvent::ToolExecutionStarted {
                    call_id: stratum_core::CallId::from("call-1"),
                    tool_name: stratum_core::ToolName::from("echo"),
                },
            },
            ScannedRow {
                event_seq: 8,
                event_version: 1,
                session_id: SessionId::new(),
                turn_id: TurnId::new(),
                created_at: chrono::Utc::now(),
                event: DurableAgentEvent::MessageAppended {
                    message: ChatMessage::assistant("done"),
                },
            },
        ];

        let projected = rows
            .iter()
            .filter_map(|row| {
                product_event(&row.event)
                    .expect("known event projects")
                    .map(|event| (row.event_seq, event))
            })
            .collect::<Vec<_>>();

        assert_eq!(projected.len(), 1);
        assert_eq!(projected[0].0, 8);
    }

    fn internal_row(event_seq: u64) -> ScannedRow {
        ScannedRow {
            event_seq,
            event_version: 1,
            session_id: SessionId::new(),
            turn_id: TurnId::new(),
            created_at: chrono::Utc::now(),
            event: DurableAgentEvent::ToolExecutionStarted {
                call_id: stratum_core::CallId::from("call-1"),
                tool_name: stratum_core::ToolName::from("echo"),
            },
        }
    }

    fn test_context(
        agent_runtime_id: AgentRuntimeId,
        agent_id: AgentId,
        generation: Uuid,
        slot: Arc<RuntimeSlot>,
    ) -> DispatcherTaskContext {
        DispatcherTaskContext {
            agent_runtime_id,
            agent_id,
            slots: Arc::new(Mutex::new(HashMap::from([(
                agent_runtime_id,
                Arc::clone(&slot),
            )]))),
            slot,
            generation,
            idle_timeout: Duration::from_secs(1),
            durable_target: Arc::new(AtomicU64::new(0)),
        }
    }
}
