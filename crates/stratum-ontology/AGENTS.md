# stratum-ontology conventions

- This library-only crate owns the Ontology aggregate, deterministic candidate
  validation, and only these five PostgreSQL relations: `ontologies`,
  `ontology_object_types`, `ontology_properties`, `ontology_link_types`, and
  `ontology_canvas_positions`.
- `OntologyStore` is the only persistence interface. It is a concrete,
  in-process PostgreSQL type called directly by `stratum-api`; do not add a
  repository trait, port/adapter, facade, manager, RPC client, binary, or a
  generic persistence abstraction.
- Keep PostgreSQL rows normalized. Do not add a canonical JSONB document,
  duplicate projection, snapshot/version table, diff/upsert path, staging
  table, COPY path, cache, graph library, or ORM.
- Object Type, Property, and Link Type IDs remain distinct UUIDv7 newtypes.
  Properties are nested under one Object Type; Link Types only model semantic
  endpoints and directional cardinalities, never physical joins or data-source
  bindings.
- The public aggregate structs and scalar enums are deliberately exhaustive:
  they encode this closed metamodel and its exact persistence/API conversion
  contract. Adding a field or variant is a coordinated schema change, not a
  backward-compatible extension; do not add `#[non_exhaustive]` or builders
  merely to speculate about future metadata.
- Complete reads and neighborhoods use read-only Repeatable Read transactions.
  Full replacement and deletion use a Read Committed root revision
  compare-and-swap in one atomic PostgreSQL transaction; successful saves
  delete and reinsert child rows in dependency order.
- A depth 1–5 neighborhood loads the bounded Link Type set once, performs the
  bidirectional BFS in Rust, and reuses those ordered rows for the induced
  subgraph. Property reads use one ordered set of aligned PostgreSQL column
  arrays to avoid 10,000-row protocol overhead; validate every array length
  before reconstructing the aggregate.
- `OntologyStore::is_ready` is a non-mutating `SELECT 1` probe used only by
  process readiness; it must never create or repair schema state.
- HTTP DTOs, ETags, OpenAPI, request limits, process configuration, and error
  response rendering belong in `stratum-api`. Do not leak database errors,
  connection URLs, names, descriptions, candidates, or ETags to tracing.
- Keep real PostgreSQL tests ignored under `tests/` and run them through this
  crate's `Makefile` and `docker-compose.test.yml`.
