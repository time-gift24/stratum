# ADR 0001: Ontology owns its bounded-context persistence

## Status

Accepted.

## Decision

`stratum-postgres` owns Agent Runtime execution persistence. The library-only
`stratum-ontology` module is the sole bounded exception: it owns Ontology domain rules and the
five PostgreSQL relations `ontologies`, `ontology_object_types`, `ontology_properties`,
`ontology_link_types`, and `ontology_canvas_positions`, together with their migrations and
transactions.

`stratum-api` calls one concrete `OntologyStore` in-process. This decision does not create a
second binary, network protocol, repository trait, adapter, facade, manager, JSONB canonical
document, duplicated projection, or snapshot/version store.

## Consequences

Ontology persistence stays local to the domain invariants that require it, while Agent Runtime
persistence remains in `stratum-postgres`. `stratum-ontology` may not become a general-purpose
storage entry point or own tables outside its bounded context.

Execution storage and Ontology may share one PostgreSQL server, but they use separate databases.
Each crate owns an independent embedded SQLx migration set and therefore must not share the
`_sqlx_migrations` table.

## Dependency note

`sqlx` is the single direct database dependency because it provides PostgreSQL connections,
transactions, typed runtime queries, and embedded migrations without an ORM or an additional
abstraction. Version 0.8.6 is licensed under MIT OR Apache-2.0. The selected feature set is
limited to Tokio/Rustls, PostgreSQL, UUID, chrono, migrations, and macros; every query binds
external values rather than constructing SQL from them.
