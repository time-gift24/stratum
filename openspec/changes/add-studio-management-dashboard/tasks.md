## 1. Rebase and architecture

- [x] 1.1 Create an isolated `codex/` worktree, rebase PR #61 on current `origin/main`, and preserve current runtime architecture during conflict resolution.
- [x] 1.2 Re-read the Constitution, Rust, product, visual, and deep-module constraints; identify the legacy filesystem catalog and HostState as incompatible.
- [x] 1.3 Confirm the Postgres-backed management direction and reconcile this change's proposal, design, specifications, and task plan.

## 2. Studio persistence domain

- [ ] 2.1 Add `stratum-studio` as a library-only independent domain module with its own database, migrations, typed errors, and crate conventions.
- [ ] 2.2 Amend the Constitution and related `AGENTS.md` files to define the narrow Studio database exception without weakening the execution-ledger rules.
- [ ] 2.3 Promote the shared validated Agent template name type to the core layer, retaining strict parsing and all existing callers.
- [ ] 2.4 Implement concrete `StudioStore` commands and queries for catalog seed, Agent definitions, Providers, and Models; serialize catalog writes with a database transaction and revision row.
- [ ] 2.5 Implement canonical representations and strong ETags, reference checks, revision conflicts, and typed blockers without exposing credentials.
- [ ] 2.6 Add strict `[studio]` configuration for an independent database URL and loopback-only management enablement.
- [ ] 2.7 Seed an empty Studio catalog once from boot LLM config and the read-only template catalog; fail closed on invalid persisted catalog data.
- [ ] 2.8 Add Studio unit and ignored container integration tests for migrations, transactions, catalog seed, credential redaction, and reference integrity.

## 3. Runtime and HTTP integration

- [ ] 3.1 Replace the obsolete management files and old HostState imports with a small runtime catalog module that hides static and Studio-backed adapters.
- [ ] 3.2 Ensure a Turn clones its configured provider `Arc` before execution; a Studio update only changes subsequently started Turns.
- [ ] 3.3 Implement loopback-only management routes, DTOs, error mapping, OpenAPI paths, ETags, pagination, and safe provider connection probes.
- [ ] 3.4 Keep `/v1/agent-templates` and `/v1/models` compatible by projecting the active runtime catalog.
- [ ] 3.5 Add `stratum-api` tests for route registration, CRUD, pagination, 412/409 behavior, seed/restart, secret non-disclosure, and runtime compatibility.

## 4. Studio web interface

- [ ] 4.1 Extend the typed API client with management resources while preserving existing conversation and ontology behavior.
- [ ] 4.2 Refactor Studio feature state into pure mapping/reducer modules with a small interface for loaded, dirty, saving, conflict, invalid, and test states.
- [ ] 4.3 Implement accessible Studio dashboard, Agent editor, Provider/Model Settings, query-preserving pagination/search, error recovery, and dirty navigation protection.
- [ ] 4.4 Add the required Agent version input and explain that definition edits affect future AgentRuntime creation; never render persisted secrets.
- [ ] 4.5 Reconnect Studio to product navigation using existing tokens and protected-component constraints; verify narrow-screen and reduced-motion behavior.

## 5. Verification and archival preparation

- [ ] 5.1 Run Rust format, clippy, unit tests, and required ignored Studio integration tests.
- [ ] 5.2 Run `pnpm lint`, `pnpm typecheck`, `pnpm test`, and `pnpm build`; validate the final Studio surface in light/dark desktop and mobile states.
- [ ] 5.3 Run `openspec validate --all --strict` and update crate and web `AGENTS.md` files with final conventions.
- [ ] 5.4 Run a Constitution review of the complete diff, fix every red flag/violation, then prepare the change for archive.
