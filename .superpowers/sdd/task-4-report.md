status: DONE_WITH_CONCERNS

files_changed:
- Cargo.toml
- Cargo.lock
- crates/wyse-agent/Cargo.toml
- crates/wyse-agent/src/lib.rs
- crates/wyse-agent/src/error.rs
- crates/wyse-agent/src/definition.rs
- crates/wyse-agent/src/loop.rs

commits:
- 6601dfe feat: add agent public api

tests_run:
- `cargo test -p wyse-agent stream_rejects_non_user_message` -> FAIL (expected red state before implementation: missing `Agent` / `AgentError`)
- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets` -> PASS with `dead_code` warnings in `crates/wyse-agent/src/definition.rs`
- `cargo test -p wyse-agent stream_rejects_non_user_message` -> PASS

self_review_notes:
- Kept all production changes scoped to `Cargo.toml`, `Cargo.lock`, and `crates/wyse-agent/**` as requested.
- Implemented the final `MissingBuilderField { field: &'static str }` variant directly.
- `Agent::stream` is only the Task 4 shell: validates user role, checks `active`, subscribes to the event bus, and returns `AgentStream`.
- Preserved the required storage types on `Agent`: `Arc<dyn LlmProvider>`, `Arc<dyn ToolRegistry>`, and `Arc<dyn EventStreamBus>`.
- Added public docs and `# Errors` sections for fallible public APIs.

concerns:
- `cargo clippy --workspace --all-targets` is clean on errors but reports `dead_code` warnings for shell-only `Agent` fields that will be used by later streaming-loop tasks.
- Per the Task 4 brief, `Agent::stream` clears the `active` flag immediately after subscribing, so concurrent-run protection is only a temporary shell behavior until the internal loop is implemented in later tasks.

---

status: FIXED_TASK_4_FINDINGS

fix_timestamp: 2026-07-09
scope: crates/wyse-agent/src/definition.rs

changes:
- Added a private RAII active guard (`ActiveRunGuard`) field on `AgentStream`.
- Kept `AgentStream` owning an `_active_guard: ActiveRunGuard` that resets shared `active` on `Drop`.
- Changed `Agent::stream` to clear `active` only when `subscribe_run` fails, and return `EventBus` error with conversion.
- Removed immediate `self.active.store(false, ...)` from the success path so active state persists while stream exists.
- Added tests:
  - `stream_resets_active_on_subscribe_failure`
  - `stream_keeps_active_until_drop`

tests_run:
- `cargo test -p wyse-agent --all-targets` -> PASS
  - 3 tests passed in `crates/wyse-agent` (`definition::tests::stream_rejects_non_user_message`, `stream_resets_active_on_subscribe_failure`, `stream_keeps_active_until_drop`)
  - rustc warnings only: existing `dead_code` warnings for unused shell fields in `crates/wyse-agent/src/definition.rs`

notes:
- Changes are limited to Task 4 scope; no loop/task-5 behavior was added.
