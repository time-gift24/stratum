# stratum-tools AGENTS.md

## Scope

`stratum-tools` owns runtime tool traits, builtin tool wrappers, and tool registry behavior.

## Design Rules

- Tool names are provider-visible identities.
- Filesystem-mutating builtin tools require explicit filesystem injection.
- Do not auto-register filesystem-mutating tools in `BuiltinToolRegistry`.
- `ShellTool` receives its default working directory explicitly. Every call starts a fresh non-login `bash -c`, captures complete stdout/stderr, and retains no cwd, environment, process, or session handle for a later call.
- `shell` accepts only `command`, optional `workdir`, and optional `timeout_ms`. It does not own background jobs, persistence, sandbox policy, approval policy, caller-configurable environment injection, stdin, output spill, or truncation.
- Every shell call inherits the host environment afresh; changes made by one call do not carry to another.
- Non-zero exit, signal, and timeout are structured successful tool results so the model receives the diagnostics; parse, spawn, wait, terminate, and explicit cancellation failures remain typed `ToolError`s.
- Recoverable tool-domain failures should return structured tool output when the caller can act on them.
- Keep concrete builtin implementations separate from registry code.
- Do not add remote tool adapters, MCP adapters, or new approval flows until a concrete caller needs them.

## Tool Permissions

- Every registered tool declares `ToolKind` and `DangerLevel`.
- `Allow` authorizes every tool.
- `PartialAllow` authorizes only `Read + Low`; all other calls require approval.
- `RequireApproval` requires approval for every tool.
- Permission mode and registration metadata are immutable after sharing the registry.
- Keep provider-visible specs independent from runtime permission metadata.

## Cancellation

- Every `Tool` synchronously validates all deterministic input conditions through
  `Tool::validate` before approval or execution-start events. Validation is
  side-effect free; `Tool::call` must enforce the same conditions when invoked
  directly.
- `ToolRegistry::call` propagates the same borrowed `CancellationToken` to the
  selected `Tool::call`.
- Builtin tools revalidate input, then check for pre-cancellation immediately
  before starting external work and return the typed cancellation error.
- Cancellation is cooperative. Effects already issued are not rolled back, and
  cancellation does not prove that no external effect occurred.
- Once a caller records that tool execution started, it must keep awaiting the
  operation and record its outcome rather than dropping or racing the future.

## Resumable Tool Identity

- `ToolRegistry::fingerprint` is part of the resumable Turn snapshot. It must deterministically
  cover ordered provider-visible specs, authorization outcomes, and concrete implementation
  identities; a changed fingerprint fails resume before Tool or model work.

## Schema Validation Boundary

- `ToolSpec.input_schema` is the authoritative validation contract for tool arguments.
  `BuiltinToolRegistry::register` compiles the schema once (meta-schema check plus
  `jsonschema::validator_for`) and caches the compiled `jsonschema::Validator`; a tool with an
  uncompilable schema fails registration with `ToolError::InvalidInputSchema` and never enters
  the registry.
- `BuiltinToolRegistry::validate` and `BuiltinToolRegistry::call` run the compiled schema first;
  a schema rejection is a typed `ToolError::InvalidArgument` naming the failing instance path,
  and the tool's custom `Tool::validate` (or `Tool::call`) never runs for schema-invalid input.
  Schema-valid input still flows through `Tool::validate` for per-tool semantic checks.
- Per-tool `validate` implementations keep their existing checks: `Tool::call` invoked directly
  (bypassing the registry) must still reject the same invalid input.
- Dependency rationale: `jsonschema` (MIT) is the de-facto standard JSON Schema validator;
  handwriting draft-compliant validation is not realistic. It is used with
  `default-features = false` so tool schemas never trigger network or filesystem reference
  resolution at registration time. Its `pattern` keyword compiles to the backtracking
  `fancy-regex` engine, which carries a theoretical ReDoS surface for untrusted patterns;
  schemas here come from trusted tool authors, so this is accepted. Schema rejection messages
  keep only the instance path and violation category: the failing instance value is masked
  because it may carry file contents or credentials back to the model and into logs.
