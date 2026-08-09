# stratum-infra conventions

## Scope

- `stratum-infra` contains external infrastructure adapters. Keep interface definitions in
  capability `definition.rs` files, errors in `error.rs`, and concrete backends/adapters in named
  modules.
- The retained surface is narrow: the kernel sink contracts (`DurableEventSink`,
  `TelemetryEventSink`) and the concrete Agent-scoped NATS tail transport. The old
  `FilesystemAgentStore`, filesystem durable sink, `compact.jsonl` checkpoint,
  `StoreEventStreamBus` decorator, and the generic memory/NATS `EventStreamBus` are deleted and
  must not be reintroduced.

## Durable and telemetry sinks

- `DurableEventSink::append` is a correctness boundary: return only after the configured consumer
  acknowledges the event, and preserve the source error chain. Do not downgrade durable failure to
  logging.
- `TelemetryEventSink::emit` is synchronous best-effort and cannot fail or block the loop on
  transport I/O.
- This crate owns only the two kernel contract traits and `DurableEventSinkError`; the concrete
  durable implementation lives in `stratum-postgres`, and the per-agent realtime dispatcher that
  turns committed rows into tail frames lives in `stratum-api`. The old scoped filesystem/bus sink
  pipeline is deleted and must not be reintroduced.

## Agent-scoped NATS tail

- The NATS transport is the narrow concrete `agent_tail::NatsAgentTail`: it publishes and
  subscribes short retained per-agent frame streams on one JetStream stream with configurable
  finite age/bytes/message-count limits and discard-old retention. It is not a durable history and
  never guarantees replay across restarts; the Postgres durable ledger is the recovery truth.
- Tail payloads are opaque `Bytes`; `AgentStreamFrameV1` serialization and per-agent ordering of
  product vs telemetry frames are owned by `stratum-api`. The tail guarantees per-subject
  JetStream order only.
- NATS subject naming is centralized in `agent_tail/subject.rs`; business code must never use
  `async-nats` directly.
- A no-cursor subscription delivers only new frames from subscription time (`DeliverPolicy::New`);
  a cursor resumes after its position while retained, and a discarded position fails with the
  typed `AgentTailError::CursorExpired` before any delivery — never a silent fallback to full
  replay. A cursor ahead of the current tail, from another Agent, from an old recreated-stream
  generation, or applied to an empty stream expires before headers and forces cold bootstrap.
- `TailCursor` is an opaque versioned transport position binding `AgentId + JetStream stream
  creation generation + stream sequence` (string-encoded for SSE `id`); it must never be compared
  with `event_seq`/telemetry sequences or persisted as business state.
- `NatsAgentTail::is_available` reflects runtime operations, not just startup construction:
  publish/subscribe/delivery failures degrade readiness and subsequent successful broker work
  restores it. Cursor-expiry validation is a successful broker query and does not mark NATS down.

## Safety and observability

- Infrastructure errors must not contain payloads, prompts, reasoning, tool arguments/results,
  credentials, NATS auth material, or secrets. Structured logs use IDs, event type, cursor, timeout,
  and backend error only.
- Keep publish and subscription work cancellation-safe. Do not hold mutex/RwLock guards across
  `.await`; use bounded behavior wherever transport latency can otherwise block runtime progress.
- Real NATS tests remain ignored integration tests under `tests/` and run through the crate
  `Makefile`/`docker-compose.test.yml`. Unit tests must not require a live broker.
