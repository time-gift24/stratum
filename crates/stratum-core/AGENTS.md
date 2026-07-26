# stratum-core runtime protocol invariants

## Foundational agent-loop event contracts

- `DurableAgentEvent` and `AgentTelemetryEvent` are local, scope-free events emitted by the
  foundational `AgentLoop`; they are not wire envelopes and must not acquire session/agent/turn
  fields merely for a transport adapter.
- `DurableAgentEvent` names loop correctness boundaries. The loop waits for the injected durable
  sink acknowledgement before advancing. This does not mean every variant becomes an AgentStore
  history row; persistence policy belongs to the composition and projection layers.
- `AgentTelemetryEvent` is best-effort observability. Dropped, timed-out, unsupported, or failed
  telemetry must never change loop output, tool dispatch, durable frontier, or terminal status.
- `ToolExecutionStarted` is durable and occurs after tool lookup/input validation/approval but before
  dispatch. `IterationCompleted` is durable and identifies the exact iteration and cumulative usage
  whose frontier may advance.
- Both enums use stable snake_case `type` names. Adding a variant requires updating `event_type()`,
  serde tests, the scoped projection, protocol docs, and downstream exhaustive projections.

## Wire scope and projection

- `StreamEnvelope` is the transport-facing type. `RuntimeEvent::Agent` nests `AgentEvent`; consumers
  must inspect the nested event type rather than infer it from metadata.
- `ScopedAgentEventSink` receives the host-provided `AgentRuntimeContext` and carries typed
  `session_id`, `agent_id`, `turn_id`, and `AgentLocation` scope in the envelope.
- `ScopedAgentEventSink` supplies session/agent/turn scope, projects durable loop events to `AgentEvent`,
  and projects supported telemetry to nested `LlmEvent`. Unsupported telemetry is a safe no-op with
  a warning; unsupported durable events are an error.
- Agent-scoped `message_seq`, retained `EventCursor`, and loop `iteration` are independent order domains and
  must never be compared or converted.

## Hosted model configuration

- `stratum-core` owns the common `ModelConfig` snapshot: a provider-scoped `ModelId` and its
  provider-specific parameter object travel together across runtime and persistence boundaries.

## Dependency boundaries

```mermaid
flowchart LR
    Web["Web composition"] --> CP["FilesystemAgentStore"]
    Web --> JS["JetStream EventStreamBus"]
    CP --> Decorator["StoreEventStreamBus"]
    JS --> Decorator
    CP -->|"AgentStore"| Agent["Agent"]
    Decorator -->|"EventStreamBus"| Agent
    Agent -->|"resume reads and iteration CAS"| CP
    Agent -->|"unsequenced events"| Decorator
    Decorator -->|"CAS complete messages"| CP
    Decorator -->|"retained events"| JS
```

- Web owns mounted-root selection, ACLs, writer authorization, initialization of
  `FilesystemAgentStore`, construction of the independent JetStream bus,
  and injection of both the store as the Agent's `AgentStore` and the decorator
  as its `EventStreamBus`.
- The Agent uses `AgentStore` directly only to load resumable state and fixed
  history and to advance the durable iteration frontier. It publishes every
  event through `EventStreamBus` without a business sequence.
- `StoreEventStreamBus` commits complete messages before forwarding the
  committed envelope, persists required lifecycle state, and passes other
  events directly to its inner retained bus.
- JetStream is an independent file-backed, limits-retained cache. Neither it nor
  `FilesystemAgentStore` owns the other.
- `message_seq` lives only inside committed `AgentEvent::Message` values. `EventCursor` is an independent
  retained-transport position.
- `AgentEvent::IterationCompleted` is projected by `StoreEventStreamBus` through
  `complete_iteration` before retained forwarding. `ToolExecutionStarted` is required by the loop
  sink contract but is not a complete-message history record and receives no `message_seq`.

## Complete-message commit

```mermaid
sequenceDiagram
    participant A as Agent
    participant D as StoreEventStreamBus
    participant C as FilesystemAgentStore
    participant J as JetStream

    A->>D: publish unsequenced complete message
    D->>C: append_message(envelope)
    C-->>D: committed Agent message with message_seq
    D->>J: publish committed envelope
Note over D,J: retained publish failure does not roll back the commit
```

## Turn resume

```mermaid
sequenceDiagram
    participant W as Web or scheduler
    participant A as Agent
    participant S as AgentStore
    participant T as Tool
    participant B as EventStreamBus

    W->>A: resume()
    A->>S: load_agent()
    S-->>A: Running state with session_id, turn_id, next_iteration, last_seq L
    loop page the fixed range through L
        A->>S: history_page(after_seq, through_seq = L)
        S-->>A: sequenced complete messages through L
    end
    A->>A: validate history, frontier, and ordered tool-result prefixes
    Note right of A: invalid prefixes fail closed and only a missing suffix is resumable
    A->>A: restore the same session_id and turn_id
    alt durable boundary
        A->>B: continue LLM events from next_iteration without immediate CAS
    else unadvanced terminal assistant
        A->>S: complete_iteration(session_id, turn_id, next_iteration, usage)
        S-->>A: CAS advances next_iteration
        A->>B: publish Finished without LLM
    else already-advanced terminal assistant
        A->>B: publish Finished without CAS or LLM
    else unadvanced tool-call iteration
        A->>A: keep the exact committed result prefix
        loop each missing tool result in call order
            A->>T: execute tool call at least once
            T-->>A: tool result
            A->>B: publish unsequenced tool result message
            B-->>A: complete-message commit acknowledged
        end
        A->>S: complete_iteration(session_id, turn_id, next_iteration, usage)
        S-->>A: CAS advances next_iteration
        A->>B: continue next LLM events
    end
```

Resume uses `last_seq` from the loaded state as an immutable history barrier.
`next_iteration` is the durable iteration frontier, not merely the next LLM
request: all lower iterations have committed stable boundaries, while history at
the frontier may still require reconciliation. Committed tool results must form
the exact ordered prefix of the immediately preceding assistant `tool_calls`.
Unknown, duplicate, sparse, or out-of-order results fail closed as invalid resume
history; only the missing suffix executes. The active turn shape and frontier
select one of the four branches above. Complete messages and lifecycle events
published during resumption still pass through `StoreEventStreamBus` for commit
and retained delivery.

## Fixed-barrier recovery

```mermaid
sequenceDiagram
    participant W as Web reader
    participant J as JetStream
    participant C as FilesystemAgentStore

    W->>J: subscribe_session and begin buffering
    W->>C: history_page through_seq = None
    C-->>W: first page and fixed last_seq barrier L
    loop while the fixed range has more pages
        W->>C: history_page through_seq = L
        C-->>W: next page through L
    end
    W->>W: discard buffered stable duplicates at or below L
    W->>W: apply remaining buffered events
    W->>J: continue the same live subscription
```

Fixed-barrier recovery stays in Web or another external reader composition; no
runtime recovery manager owns both stores. The reader never compares an
`EventCursor` with `message_seq`.

## Session and Hook runtime identity

- `SessionId` is the UUIDv7 identity of a long-lived, graph-independent collaboration space.
  Agents and Workflow versions may change while the Session remains stable.
- A host supplies immutable `AgentRuntimeContext { session_id, location }`; the Agent creates the
  `TurnId`. `AgentLocation` is either `Direct` or a typed `WorkflowNode` location.
- `StreamEnvelope` is Session-scoped and contains no `EventSource`, top-level sequence, or legacy
  run identity. `RuntimeEvent` ownership is expressed only through its Session, Node, and Agent
  variants and their required identities.
- LLM, Tool, approval, plan, lifecycle, and message events belong to the Agent event family.
- Only committed `AgentEvent::Message` values contain a required `message_seq`. Its identity is
  `(AgentId, message_seq)`; two Agents in one Session can both have message sequence 1.
- `EventCursor` is an opaque retained-transport position. Never compare it with `message_seq` or
  use it as persisted recovery state.
- A resumable Turn pins Agent version, resolved `ModelConfig`, ToolSet fingerprint, SkillSet
  version, ExtensionSet version, and ordered Hook Handler versions.
- A Hook invocation address binds Session, Agent, Turn, Hook point, Handler position/version,
  operation identity, and input digest. Pending retry identity is stable; mismatches fail closed.
- The current beta protocol rejects unsupported state and payload shapes. Do not add migration,
  downgrade, rollback, dual-read, or legacy-write paths.
