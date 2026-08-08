# stratum-core runtime protocol invariants

## Scope

`stratum-core` keeps only domain types, ID newtypes, the kernel event enums
(`DurableAgentEvent` / `AgentTelemetryEvent`), and hook addressing. The old
transport DTO surface — `StreamEnvelope`, `RuntimeEvent`, `AgentEvent`,
`EventCursor`, `ReplayStart`, `EventRecord`, `NewAgentMessage`,
`HistoryQuery`/`HistoryPage`, and `message_seq` — is deleted and must not be
reintroduced here; wire framing belongs to the API layer.

## Foundational agent-loop event contracts

- `DurableAgentEvent` and `AgentTelemetryEvent` are local, scope-free events emitted by the
  foundational `AgentLoop`; they are not wire envelopes and must not acquire session/agent/turn
  fields merely for a transport adapter.
- `DurableAgentEvent` names loop correctness boundaries. The loop waits for the injected durable
  sink acknowledgement before advancing. Which variants are persisted and how they are read back
  is decided by the concrete `stratum-postgres` execution storage and the API composition.
- `AgentTelemetryEvent` is best-effort observability. Dropped, timed-out, unsupported, or failed
  telemetry must never change loop output, tool dispatch, durable frontier, or terminal status.
- `ToolExecutionStarted` is durable and occurs after tool lookup/input validation/approval but before
  dispatch. `IterationCompleted` is durable and identifies the exact iteration and cumulative usage
  whose frontier may advance.
- Both enums use stable snake_case `type` names. Adding a variant requires updating `event_type()`,
  serde tests, and downstream exhaustive projections in the storage and API layers.

## Hosted model configuration

- `stratum-core` owns the common `ModelConfig` snapshot: a provider-scoped `ModelId` and its
  provider-specific parameter object travel together across runtime and persistence boundaries.

## Session and Hook runtime identity

- `SessionId` is the UUIDv7 identity of a long-lived, graph-independent collaboration space.
  Agents and Workflow versions may change while the Session remains stable.
- A host supplies immutable `AgentRuntimeContext { session_id, location }`; the Agent creates the
  `TurnId`. `AgentLocation` is either `Direct` or a typed `WorkflowNode` location.
- A resumable Turn pins Agent version, resolved `ModelConfig`, ToolSet fingerprint, SkillSet
  version, ExtensionSet version, and ordered Hook Handler versions.
- A Hook invocation address binds Session, Agent, Turn, Hook point, Handler position/version,
  operation identity, and input digest. Pending retry identity is stable; mismatches fail closed.
- The current beta protocol rejects unsupported state and payload shapes. Do not add migration,
  downgrade, rollback, dual-read, or legacy-write paths.
