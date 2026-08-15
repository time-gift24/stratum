# 0004: Isolate the Studio catalog from the execution ledger

## Status

Accepted.

## Context

Studio needs mutable Providers, credentials, Models, and Agent authoring definitions. The execution ledger is append-only runtime truth, while template files are read-only boot inputs. Neither can safely be used as a management database.

## Decision

`stratum-studio` owns an independent PostgreSQL database and migration history. On its first enabled boot, the API imports the configured providers and read-only template definitions. Later Studio data is authoritative for new AgentRuntime creation. Provider registries are swapped atomically; an already-started Turn keeps its cloned provider instance.

Credentials are stored only in the Studio database and returned only as `SecretString` to trusted provider assembly. They never appear in management reads, OpenAPI, durable events, NATS, logs, or errors.

## Consequences

Deployments must provision and separately secure a Studio database. Changing an Agent behavior requires an explicit new version tag; this preserves the immutable AgentId invariant. Studio remains a bounded authoring module, not a new execution-storage abstraction.
