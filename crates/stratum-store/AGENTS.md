# stratum-store invariants

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
- `append_message` accepts only `NewAgentMessage`, allocates the next Agent-scoped `message_seq`,
  and returns the committed envelope. Retained-bus failure never rolls back this commit.
- Agent history is isolated by Agent even when multiple Agents share one Session.
- The Store accepts only the current strict beta state and message shapes. Unsupported legacy data
  is rejected; do not add migration, rollback, downgrade, or dual-read code.
