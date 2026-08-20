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
runs the ignored tests from `tests/api.rs`, `tests/ontology_api.rs`, and
`tests/studio_db_only.rs` as crate-internal test modules with `--test-threads=1`, then `down -v`.
`autotests = false` prevents them from becoming external test crates, so their mock Provider
injection can use the crate-private, test-only `AppState` constructor without adding a production
API. Dynamic ports prevent collisions with runner services and
ephemeral client sockets. A manually managed `make test-up` stack still defaults
to Postgres 45433 and NATS 44228 unless `STRATUM_API_TEST_PG_HOST_PORT` /
`STRATUM_API_TEST_NATS_HOST_PORT` override them. The suite drives
the real router (tower `oneshot`) against real Postgres/NATS with a scripted mock LLM provider and
covers: DB-only Studio runtime assembly and fail-closed startup, the create idempotency matrix,
the real management Router's Provider/Model/Agent CRUD, ETag conflicts, reference blockers,
credential redaction, compatibility projections and restart persistence, admission CAS/session rules,
started-only reconciliation, full turns, the approval lifecycle, cancel races, history pagination
and compaction markers, the AgentRuntimeView derivation, crash-resume with approval reuse, and the SSE contract
(`stream_ready`, cursor validation/expiry, buffer-overflow reset, realtime degradation).
The management integration test exercises only the missing-Provider probe path, so it cannot make
an external request; fixed-endpoint Provider probe success, rejection, timeout, and sanitization stay
covered by loopback unit tests.

The one PostgreSQL container initializes separate `stratum_test`, `stratum_ontology_test`,
`stratum_studio_test`, deliberately invalid `stratum_studio_corrupt_test`, and transaction
fault-injection `stratum_studio_postcommit_test` databases. This
mirrors production composition and keeps the independent `stratum-postgres`, `stratum-ontology`,
and `stratum-studio` SQLx migration histories isolated; the corrupt database exists only to prove
that runtime assembly fails closed instead of falling back to config or template files, while the
post-commit database proves Model creation has no fallible catalog read after commit. Override
endpoints with `STRATUM_API_TEST_PG_URL`, `STRATUM_API_TEST_ONTOLOGY_PG_URL`,
`STRATUM_API_TEST_STUDIO_PG_URL`, `STRATUM_API_TEST_CORRUPT_STUDIO_PG_URL`,
`STRATUM_API_TEST_POSTCOMMIT_STUDIO_PG_URL`, or `STRATUM_API_TEST_NATS_URL` when needed.
