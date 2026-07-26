# Session runtime acceptance

## Deterministic vertical test

```sh
make -C crates/stratum-api test-session-runtime
```

The test uses `openai:deterministic`, a temporary local filesystem Store, the real API router and
SSE projection, an in-memory Session EventBus, and a controlled provider. It verifies:

- one Session remains stable across host restart while each Turn receives a different `TurnId`;
- the persisted first-Turn runtime snapshot is byte-for-byte equivalent after restore;
- reconnecting after a saved cursor changes only retained transport replay;
- fixed-barrier history plus live messages deduplicates by `(AgentId, message_seq)`;
- a concurrent operation in the same Session is rejected without replacing active state;
- a second Agent can later join the Session, is visible in the shared stream, and starts an
  independent history at `message_seq = 1`;
- legacy `run_id`/`source` request payloads are rejected.

## Live DeepSeek acceptance

Export `DEEPSEEK_API_KEY` in zsh, then run:

```sh
make -C crates/stratum-api test-deepseek-session
```

The command does not put the key in arguments, config files, fixtures, snapshots, or logs. The
ignored test uses `deepseek:deepseek-v4-flash`, completes Turn 1, rebuilds the host, reconnects SSE
from the saved cursor, and completes Turn 2 in the same Session with a distinct `TurnId`. It checks
identity, terminal state, persisted four-message history, restart recovery, and scans isolated
persisted files to ensure they do not contain the credential.

Validation recorded on 2026-07-26: both the deterministic flow and the opt-in live DeepSeek flow
passed. Successful tests delete their isolated data; failed tests retain and print only the safe
diagnostic directory path.
