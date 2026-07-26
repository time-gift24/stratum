# Stratum Runtime Protocol

## Beta compatibility policy

The current protocol is a beta baseline. Unsupported state and wire shapes are rejected rather
than migrated. This phase does not design or support migration, downgrade, rollback, dual-read, or
legacy-write paths. In particular, payloads containing the removed `run_id` or `source` fields are
invalid.

## Identity model

| Identity | Meaning |
| --- | --- |
| `SessionId` | UUIDv7 identity of a long-lived, graph-independent collaboration space. |
| `AgentId` | Identity of one Agent and the owner of one conversation history. |
| `TurnId` | Identity created by an Agent for one resumable Turn. |
| `WorkflowVersionId` | Immutable identity of a published Workflow version. |
| `AgentVersionId` | Immutable identity of an Agent definition. |
| `SkillSetVersionId` | Immutable identity of an ordered Skill set. |
| `ExtensionSetVersionId` | Immutable identity of an ordered Extension set. |
| `HookHandlerVersionId` | Immutable identity of a Hook Handler implementation. |
| `HookInvocationId` | Identity of one semantically addressed Handler invocation. |

A Session can outlive any Workflow graph and can contain multiple Agents or Workflow versions.
Phase one permits only one active operation in a Session at a time. This is a phase-one design
constraint, not a permanent product restriction; no subordinate operation/attempt identity is
introduced until actual concurrency requires it.

An Agent runs either directly in a Session or as one Workflow node:

```text
AgentLocation = direct
              | workflow_node { workflow_version_id, node_id }
```

The host supplies immutable `AgentRuntimeContext { session_id, location }`; the Agent creates only
the `TurnId`. A direct conversation is not modeled as an implicit Workflow.

## Stream envelope and event ownership

`StreamEnvelope` has exactly these protocol fields:

| Field | Type | Meaning |
| --- | --- | --- |
| `session_id` | `SessionId` | Session stream partition. |
| `timestamp` | `DateTime<Utc>` | Event creation time. |
| `event` | `RuntimeEvent` | Typed Session, Node, or Agent event. |
| `metadata` | map | Optional non-business runtime metadata. |

`RuntimeEvent` is a tagged union:

- `session { event }` owns Session lifecycle events;
- `node { workflow_version_id, node_id, event }` owns ordinary Workflow-node events;
- `agent { agent_id, turn_id, location, event }` owns Agent lifecycle, message, LLM, Tool,
  approval, and plan events.

Identity fields are required in their owning event family. `metadata` must not contain primary
identities, business payloads, lifecycle state, secrets, credentials, or data needed by the UI.

## Foundational Agent loop projection

`DurableAgentEvent` and `AgentTelemetryEvent` are scope-free events emitted by the foundational
Agent loop. `ScopedAgentEventSink` adds the Session, Agent, Turn, and location identities when it
projects them into `RuntimeEvent::Agent`.

The loop waits for every `DurableAgentEvent` acknowledgement before crossing that correctness
boundary. Durable events cover loop start, complete messages, approvals, tool execution start,
iteration completion, and terminal outcomes. In particular:

- `tool_execution_started { call_id, tool_name }` is committed after lookup, validation, and
  approval but before dispatching the Tool;
- `iteration_completed { iteration, usage }` advances the Store's durable iteration frontier
  before retained forwarding.

`AgentTelemetryEvent` is best-effort observability for supported LLM start, delta, and finish
events. Dropped, timed-out, or failed telemetry must never change loop output, tool dispatch,
durable progress, or terminal state. Neither durable lifecycle events nor telemetry events carry
`message_seq`.

## Committed Agent messages

Only a committed `AgentEvent::Message` contains `message_seq: u64`. The sequence is allocated by
that Agent's Store and is scoped to `(AgentId, message_seq)`:

```text
Agent A: 1, 2, 3, ...
Agent B: 1, 2, 3, ...
Session stream: events from A and B may interleave
```

`message_seq` is not a Session-global order and cannot be attached to lifecycle, LLM, Node, or
Session events. `AgentStore::append_message` accepts an unsequenced `NewAgentMessage` and returns
the committed, publishable envelope. A retained-bus forwarding failure never rolls back the durable
message commit.

Agent histories are independent even when Agents share one Session. Session state and results may
be shared by future Hooks, but conversation history is not implicitly shared.

## EventCursor and fixed-barrier recovery

`EventCursor` is an opaque position in the retained Session transport. It is used only by
`ReplayStart::{All, New, After}` and SSE `Last-Event-ID`/`after_cursor`. It is never compared with
`message_seq`, persisted as Agent recovery state, or used to resume a Turn.

Consumer-first recovery follows this boundary:

1. subscribe to the Session stream and buffer events;
2. read each selected Agent history using its fixed `last_seq` barrier;
3. classify buffered messages using that message owner's `(AgentId, message_seq)` and barrier;
4. discard duplicates, apply messages beyond the relevant Agent barrier, then continue live;
5. retain non-message events by transport order without inventing a business sequence.

An expired retained cursor is an explicit error. Retrying without that cursor affects only
transport replay; it does not change durable Turn recovery.

## Persisted Turn runtime snapshot

Starting a Turn atomically persists Session, Turn, location, and the exact runtime snapshot:

- `AgentVersionId`;
- fully resolved `ModelConfig`;
- SHA-256 `ToolSetFingerprint` over ordered specs, authorization outcomes, and implementation
  identities;
- `SkillSetVersionId`;
- `ExtensionSetVersionId`;
- ordered `HookHandlerVersionId` values.

Resume preserves the original Session, Turn, and location and validates every pinned component
before any model request, Tool execution, or future Hook execution. Missing or mismatched pinned
components fail closed. The filesystem Store accepts only the current strict state and message
shape.

## Hook invocation baseline

The four decision-affecting Hook points are `transform_context`, `before_tool_call`,
`after_tool_call`, and `prepare_next_turn`. A semantic Hook invocation address binds:

- Session, Agent, and Turn;
- Hook point;
- Handler position and immutable Handler version;
- operation identity inside the Turn;
- canonical input digest.

Pending retries keep the same invocation identity. Completed results are reused only when version
and input digest match. Terminal failure, timeout, cancellation, version mismatch, input mismatch,
invalid output, and unavailable pinned Handler are typed terminal outcomes.

Hook journal state belongs to Session/Turn execution state. It is not Agent conversation history and
is not reconstructed from EventBus observations. H3/P1 will provide its storage implementation;
this baseline deliberately adds no journal backend.

## Extension trust baseline

- Skills are declarative and can use only already-authorized Tools.
- Script Hooks run out of process with an explicit isolation policy.
- Linked Rust Hooks are trusted application components and must match runtime compatibility.
- Remote Hook Services use authenticated service identity and `HookInvocationId` as an idempotency
  key.
- Exposed errors are sanitized and must not contain secrets, credentials, raw sensitive inputs, or
  host paths.

## SSE projection

SSE `id` is the opaque transport cursor, `event` is the nested runtime event type, and `data` is the
complete `StreamEnvelope` JSON. A Session can be subscribed directly at
`/v1/sessions/{session_id}/events`; Agent-specific endpoints may resolve the Agent's persisted
Session and use the same Session subscription.
