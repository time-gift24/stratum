# 0004: Isolate the Studio catalog from the execution ledger

## Status

Accepted.

## Context

Studio needs mutable Providers, credentials, Models, and Agent authoring definitions. The execution ledger is append-only runtime truth, and deployment configuration is an operational input. Neither can safely double as a mutable management catalog without creating competing sources of truth.

## Decision

`stratum-studio` owns an independent PostgreSQL database and migration history. The API always connects it and uses it as the sole source for Provider, Model, credential, and Agent authoring definitions. A new database starts with an explicitly empty catalog; boot configuration, environment API keys, and template files are never imported or used as fallback. `management_enabled` controls only loopback management-route exposure. The API builds a Provider snapshot from one transaction-consistent Studio read whenever new LLM work starts instead of maintaining a mutable process cache; an already-started Turn keeps its cloned provider instance.

Provider endpoints and timeouts remain fixed, trusted adapter policy in the API assembly layer. They are neither deploy configuration nor mutable Studio resources; this keeps the provider set closed and avoids a configurable outbound URL surface.

Credentials are stored only in the Studio database and returned only as `SecretString` to trusted provider assembly. They never appear in management reads, OpenAPI, durable events, NATS, logs, or errors.

## Consequences

Deployments must provision and separately secure a Studio database even when management routes are disabled. Operators provision Provider → Model → Agent definition explicitly through the loopback management API; removing legacy `[agent]` and `[llm]` configuration is a breaking cutover. Changing an Agent behavior requires an explicit new version tag; this preserves the immutable AgentId invariant. Studio remains a bounded authoring module, not a new execution-storage abstraction.
