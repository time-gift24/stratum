# wyse-filesystem AGENTS.md

## Scope

`wyse-filesystem` owns the agent-visible virtual filesystem trait, virtual path validation, and the local sandbox backend.

## Design Rules

- Public file APIs accept `VirtualPath`, not raw strings or host paths.
- Keep paths virtual and absolute, for example `/README.md`.
- Do not expose host paths, sandbox roots, or file contents in errors or tracing.
- Backend implementations should implement minimal file primitives only.
- `remove_dir` removes empty directories only.
- Do not add `apply_patch`, mount routers, registries, factories, managers, read-only policy, stream IO, glob/search, watch, snapshot, remote backends, or object storage until a concrete caller needs them.
- Local sandbox operations must reject symlink escapes by default.
