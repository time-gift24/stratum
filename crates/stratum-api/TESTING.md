# stratum-api testing

## Unit tests (no containers)

```sh
cargo test -p stratum-api --all-targets
```

Covers: dispatcher ordering/tolerance, cursor parsing, the error mapping table, DTO strictness,
the provenance lineage, the historical normalization matrix, frame-mapping safety, registry claim
semantics, and the admission gate.

## Container integration tests

```sh
make -C crates/stratum-api test-integration
```

Brings up `docker-compose.test.yml` (Postgres 17 on 45433, NATS `-js` on 44228; compose project
`stratum-api-test`), runs `tests/api.rs` with `--test-threads=1`, then `down -v`. The suite drives
the real router (tower `oneshot`) against real Postgres/NATS with a scripted mock LLM provider and
covers: the create idempotency matrix, admission CAS/session rules, started-only reconciliation,
full turns, the approval lifecycle, cancel races, history pagination and compaction markers, the
AgentRuntimeView derivation, crash-resume with approval reuse, and the SSE contract
(`stream_ready`, cursor validation/expiry, buffer-overflow reset, realtime degradation).

Override endpoints with `STRATUM_API_TEST_PG_URL` / `STRATUM_API_TEST_NATS_URL` when needed.
