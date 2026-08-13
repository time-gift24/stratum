## 1. Architecture and workspace setup

- [x] 1.1 Narrow the root Constitution so `stratum-postgres` owns Agent Runtime execution persistence while `stratum-ontology` alone may own its bounded-context PostgreSQL tables, and add `docs/adr/0001-ontology-owns-its-persistence.md`.
- [x] 1.2 Add the accepted Ontology language/API reference under `docs/ontology`, clarify that deleted child-ID non-reuse is the client generator's responsibility rather than a tombstone guarantee, update `TODO.md` with deferred capabilities, and keep frontend implementation outside this change.
- [x] 1.3 Add `crates/stratum-ontology` to the Cargo workspace with a thin `lib.rs`, separate `error.rs`, feature-free public surface, and the minimum shared dependencies.
- [x] 1.4 Add SQLx workspace dependency with only Tokio/Rustls, PostgreSQL, UUID, chrono, migration and macro features; do not add an ORM, graph library, repository trait or benchmark framework.
- [x] 1.5 Create `crates/stratum-ontology/AGENTS.md` documenting crate ownership, forbidden dependencies, five-table authority and the concrete in-process interface.

## 2. Domain model and validation

- [x] 2.1 Implement distinct UUIDv7 newtypes for Ontology, Object Type, Property and Link Type IDs with strict parsing and common serialization traits.
- [x] 2.2 Implement the complete Ontology, Object Type, owned Property, Link Type, Canvas Position, value-type and cardinality domain types without revision, instance or physical-binding fields.
- [x] 2.3 Implement one-pass Candidate validation for names, Unicode scalar lengths, IDs, ownership, same-document endpoints, finite positions, independent name scopes and all count limits.
- [x] 2.4 Implement typed stable violation codes, RFC 6901 paths and deterministic path/code sorting while retaining every independently detectable violation.
- [x] 2.5 Add domain unit tests covering empty schemas, Property name reuse across owners, duplicate scopes, dangling links/positions, scalar enums, all boundaries and deterministic multi-violation ordering.

## 3. PostgreSQL schema and concrete store

- [x] 3.1 Add the initial embedded migration for exactly `ontologies`, `ontology_object_types`, `ontology_properties`, `ontology_link_types` and `ontology_canvas_positions` with the designed keys, checks and cascades.
- [x] 3.2 Add composite same-Ontology Link endpoint constraints and only the list, ordered assembly and source/target traversal indexes required by the fixed access paths.
- [x] 3.3 Implement the concrete `OntologyStore::connect` path, fixed pool defaults and embedded migration execution with redacted typed connection/migration errors.
- [x] 3.4 Implement create and one-statement paginated list, including deployment-wide name conflicts, deterministic supported sorting, out-of-range pages and RFC 3339 timestamps.
- [x] 3.5 Implement complete Ontology reads through ordinary typed queries in a Repeatable Read read-only transaction and preserve every presentation order.
- [x] 3.6 Map known typed-ID primary-key conflicts to `409 ontology_entity_id_conflict` without pre-scanning IDs, parsing database error text or exposing constraint details.
- [x] 3.7 Implement Read Committed root revision CAS, dependency-ordered child deletion and chunked full Candidate inserts in one transaction, distinguishing missing from stale without diff/upsert/COPY/staging.
- [x] 3.8 Implement conditional aggregate deletion and guarantee all root/child rows are removed only when the expected revision is current.
- [x] 3.9 Implement depth 0–5 bidirectional frontier traversal and induced-subgraph assembly in a Repeatable Read read-only transaction using standard collections and indexed typed queries.

## 4. PostgreSQL verification assets

- [x] 4.1 Add crate-local `docker-compose.test.yml` with project name `stratum-ontology-test`, PostgreSQL healthcheck and an isolated test database.
- [x] 4.2 Add a crate-local `Makefile` that defaults to `podman compose`, accepts `COMPOSE="docker compose"`, runs ignored integration tests and always tears down volumes.
- [x] 4.3 Add ignored store integration tests for migrations, CRUD/round-trip ordering, relational constraints, name/ID scopes, hard deletion and all error classifications.
- [x] 4.4 Add ignored transaction tests proving stale CAS writes nothing, post-CAS failure rolls back revision/children, concurrent same-revision saves commit exactly once, and different Ontologies do not share the gate.
- [x] 4.5 Add ignored consistency/neighborhood tests for concurrent reads, reverse links, cycles, multiple paths, depth zero, self-links, induced edges and missing origins.
- [x] 4.6 Add an ignored maximum-fixture measurement using 500 Object Types, 10,000 Properties and 2,000 Link Types; after warm-up, calculate p95 without a new benchmark dependency and assert full/depth-5 database-side reads are at most 100 ms.

## 5. HTTP API and process composition

- [x] 5.1 Add strict HTTP DTOs and conversions for the exact full-resource/list/pagination/create/Candidate/neighborhood wire shapes, and extend shared `ErrorResponse.error` with `violations` serialized only for Ontology 422; reject unknown fields and explicit null descriptions.
- [x] 5.2 Implement canonical strong ETag generation/parsing without signing or issued-tag state, require one strong `If-Match`, and map absent, invalid HTTP syntax/weak/list/wildcard, any non-current strong value and missing resource to 428/400/412/404.
- [x] 5.3 Implement the list/create/get/replace/delete handlers under `/v1/ontologies`, including `Location`, ETag, no-body 204 responses and exact status/error mapping.
- [x] 5.4 Implement the persisted neighborhood handler with depth default/range, no ETag and a response shape that cannot be mistaken for a complete Candidate.
- [x] 5.5 Apply a 2 MiB limit only to Ontology POST/PUT while preserving the existing 64 KiB limit on unrelated API routes.
- [x] 5.6 Add required strict `[ontology].database_url` configuration, update example/Docker/test configs, initialize `OntologyStore` and migrations at startup, and pass the concrete store through API state.
- [x] 5.7 Add safe tracing spans and one boundary log per handled failure without recording ETags, database URLs, names, descriptions or Candidate contents.

## 6. OpenAPI and HTTP verification

- [x] 6.1 Annotate every Ontology handler and wire DTO with utoipa schemas under the `Ontology` tag, including enum/format/range/regex constraints, headers and every actual error response.
- [x] 6.2 Replace the existing OpenAPI test/spec assumption of exactly 11 endpoints with coverage of every router-mounted operation, and verify Ontology 204 responses declare no body.
- [x] 6.3 Add router tests for exact CRUD/list/neighborhood payloads and headers, strict parsing, pagination/sorting, 2 MiB isolation, shared error-envelope omission rules, and the complete status/error matrix including both 409 codes.
- [x] 6.4 Add router/store tests proving 422 returns sorted RFC 6901 violations, 412 and validation failures preserve data/ETag, same-document PUT advances ETag, and response DTO round trips preserve fields and array order.

## 7. Final validation and archive readiness

- [x] 7.1 Run `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo build --workspace` and `cargo test --workspace --all-targets`; fix every failure without suppressing justified lints.
- [x] 7.2 Run the crate PostgreSQL integration suite and maximum-fixture p95 check through its Makefile; record the measured full and depth-5 p95 values.
- [x] 7.3 Run `openspec validate --all --strict` and confirm the parallel frontend change is neither a backend acceptance dependency nor merged into backend/main specs by this change.
- [x] 7.4 Dispatch a separate sub-agent using `constitution-review` to inspect the complete implementation diff clause by clause; fix all red-flags and violations, then rerun affected validation.
- [x] 7.5 Reconcile the final implementation contract into `crates/stratum-ontology/AGENTS.md`, verify no third-party product branding or deferred capability leaked in, and report the change ready for `/opsx:archive` without archiving it automatically.

## 8. Review remediation

- [x] 8.1 Run both new ignored PostgreSQL integration suites in CI with unconditional `down -v`, and preserve the existing `stratum-api` AgentRuntime and DeepSeek Makefile entry points.
- [x] 8.2 Validate the Ontology PostgreSQL URL in `stratum-config` and prevent its credential-bearing value from appearing through `Debug`.
- [x] 8.3 Add production liveness and dependency-aware readiness for the Agent Store, NATS and Ontology PostgreSQL, and log router-generated 4xx/5xx responses exactly once at the required level.
- [x] 8.4 Preserve the SQLx source for known name/entity conflicts and classify PostgreSQL shutdown/startup-unavailable SQLSTATEs as `503 ontology_store_unavailable`.
- [x] 8.5 Ensure store tracing cannot record or reconstruct ETags from `expected_revision` and entity identity fields.
- [x] 8.6 Accept every RFC entity-tag byte allowed by HTTP while keeping weak, wildcard, list and malformed `If-Match` values at 400 and valid non-current strong tags at 412.
- [x] 8.7 Express UUIDv7, not generic UUID, for every typed Ontology ID in generated OpenAPI and lock the schema with tests.
- [x] 8.8 Add a concurrent replacement/neighborhood integration test that proves every neighborhood response comes from one Repeatable Read snapshot.
- [x] 8.9 Replace the star-shaped depth-five performance fixture with a true five-hop maximum fixture and rerun the p95 gate without adding benchmark infrastructure.
- [x] 8.10 Resolve the public-type extensibility review finding with the smallest contract-safe design and archive that decision in the crate conventions.
- [x] 8.11 Rerun formatting, clippy, workspace build/tests, both PostgreSQL suites, strict OpenSpec validation and an independent full-diff Constitution review; fix every remaining red flag or violation before restoring archive readiness.
