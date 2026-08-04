# stratum-store invariants

## Scope

- `stratum-store` is the pure persistence contract crate: the `AgentStore` trait,
  `AgentState`/`AgentStatus`, `StoreError`, and the contract constants (`AGENT_STATE_VERSION`,
  `MAX_HISTORY_PAGE_SIZE`). It depends only on `stratum-core`; it must not depend on
  `stratum-infra` or `stratum-filesystem`.
- Durable backends and the store-backed event stream bus live in `stratum-infra`
  (`agent_store::FilesystemAgentStore`, `agent_store::StoreEventStreamBus`). Dependency direction:
  `stratum-core ← stratum-store ← stratum-infra`.
- `StoreError` is the contract error type of every backend. Backend-specific failures are carried
  by `StoreError::Backend` as a boxed source; do not add backend-typed variants to this crate.

## Agent Loop Event Projection

- Persistence is a durable-event consumer; it is not an `AgentLoop` dependency.
- The store-backed consumer applies durable loop-event projections before
  acknowledging or forwarding the event.
- `IterationCompleted` calls `AgentStore::complete_iteration` before forwarding.
- A store or projection failure returns without acknowledgement and without
  forwarding the event.
- After a projection commits successfully, forwarding is best-effort. A
  forwarding error or timeout cannot undo the store commit.
- Durable events without a store projection retain the downstream bus's
  acknowledgement requirement.

## Session Runtime State

- Starting a Turn atomically commits Session, Turn, Agent location, and the exact runtime snapshot;
  a failed candidate must leave the prior durable state intact.
- The accepted snapshot's `ModelConfig` also becomes the Agent's current configuration for later
  Turns. Model parameters may change between Turns but never during resume of the active Turn.
- `append_message` accepts only `NewAgentMessage`, allocates the next Agent-scoped `message_seq`,
  and returns the committed envelope. Retained-bus failure never rolls back this commit.
- Agent history is isolated by Agent even when multiple Agents share one Session.
- The Store accepts only the current strict beta state and message shapes. Unsupported legacy data
  is rejected; do not add migration, rollback, downgrade, or dual-read code.
