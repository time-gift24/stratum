# stratum-filesystem AGENTS.md

## Scope

`stratum-filesystem` owns the agent-visible virtual filesystem trait, virtual path validation, and the local sandbox backend. Its retained scope is business file operations only: `read`/`list`/`write`/`create`/`remove`/`apply-patch`, plus read-only template catalog access for the composing side. Execution persistence (CAS, `record`, `get`/`put`, agent state/history/event logs) is deleted and must not be reintroduced — Postgres is the sole execution store.

## Design Rules

- Public file APIs accept `VirtualPath`, not raw strings or host paths.
- Keep paths virtual and absolute, for example `/README.md`.
- Do not expose host paths, sandbox roots, or file contents in errors or tracing.
- Backend implementations should implement minimal file primitives only.
- The `Filesystem` trait is object-safe so runtime tools can receive explicit `Arc<dyn Filesystem>` dependencies.
- Apply-patch support is a concrete shared filesystem capability; it must keep all paths virtual and all reads/writes behind the `Filesystem` trait.
- Apply-patch errors and output must not expose host paths or file contents.
- `remove_dir` removes empty directories only.
- `write_file` must stay crash-consistent: write to a temporary sibling file, then atomically rename; `list_dir` hides in-flight temporary files.
- Do not add mount routers, registries, factories, managers, read-only policy, stream IO, glob/search, watch, snapshot, remote backends, or object storage until a concrete caller needs them.
- Local sandbox operations must reject symlink escapes by default.
