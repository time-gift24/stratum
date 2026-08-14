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

Brings up `docker-compose.test.yml` (compose project `stratum-api-test`) with dynamically published
loopback ports for Postgres 17 and NATS `-js`, injects the resolved endpoints into the test process,
runs `tests/api.rs` and `tests/ontology_api.rs` with `--test-threads=1`, then `down -v`. Dynamic ports prevent collisions with
runner services and ephemeral client sockets. A manually managed `make test-up` stack still defaults
to Postgres 45433 and NATS 44228 unless `STRATUM_API_TEST_PG_HOST_PORT` /
`STRATUM_API_TEST_NATS_HOST_PORT` override them. The suite drives
the real router (tower `oneshot`) against real Postgres/NATS with a scripted mock LLM provider and
covers: the create idempotency matrix, admission CAS/session rules, started-only reconciliation,
full turns, the approval lifecycle, cancel races, history pagination and compaction markers, the
AgentRuntimeView derivation, crash-resume with approval reuse, and the SSE contract
(`stream_ready`, cursor validation/expiry, buffer-overflow reset, realtime degradation).

The one PostgreSQL container initializes separate `stratum_test` and
`stratum_ontology_test` databases. This mirrors production composition and keeps the independent
`stratum-postgres` and `stratum-ontology` SQLx migration histories isolated. Override endpoints
with `STRATUM_API_TEST_PG_URL`, `STRATUM_API_TEST_ONTOLOGY_PG_URL`, or
`STRATUM_API_TEST_NATS_URL` when needed.
