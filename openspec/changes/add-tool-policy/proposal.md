## Why

Tool authorization is currently an ambiguous `Option<(ToolKind, DangerLevel)>` returned by the registry, while the same public registry also exposes raw execution that bypasses that check. The legacy `Agent` used by the hosted API and the new `ToolExecutor` interpret this contract independently, so Stratum needs one typed, fail-closed pre-dispatch policy boundary before more stateful tools are exposed.

## What Changes

- Introduce a closed, immutable `ToolPolicy` with explicit `Deny`, `RequireApproval`, and `Allow` outcomes while preserving the current allow-all, low-risk-read, and require-approval behaviors.
- Replace the split registry lookup, validation, authorization, and raw-call sequence with a prepared invocation that binds the resolved tool, trusted registration metadata, and validated input; only an authorized, consuming invocation can dispatch the tool.
- Keep authorization policy separate from asynchronous human approval: `stratum-tools` owns policy evaluation and invocation state, while `stratum-agent` owns approval interaction, cancellation, and durable event ordering.
- Apply the same prepared invocation contract to both the new `ToolExecutor` and the legacy `Agent` path used by the hosted API and REPL without merging the two runtimes.
- Preserve existing provider-visible tool specs, approval events, HTTP approval API, frontend behavior, and legacy resume semantics. Policy denial becomes a stable structured tool result and never emits approval or execution-start events.
- Remove the implicit allow default; composition roots must choose a policy explicitly, with any retained `Default` behavior denying all calls.
- **BREAKING**: Replace the externally implementable `ToolRegistry` trait and single `BuiltinToolRegistry` implementation with one concrete `ToolRegistry`; replace `authorization`, `validate`, `get`, and raw `call` access with the prepared/authorized invocation API, and replace permission-mode construction with explicit `ToolPolicy` construction.
- Keep rule DSLs, remote policy engines, identity/tenant context, argument/resource rules, remembered approvals, and policy configuration schema outside this change.

## Capabilities

### New Capabilities

- `tool-policy`: Typed pre-dispatch policy evaluation, prepared/authorized tool invocation, approval gating, denial behavior, and consistent enforcement across both agent execution paths.

### Modified Capabilities

None.

## Impact

- `stratum-tools`: policy and decision types, trusted tool metadata, registry preparation, authorized invocation, and registry tests.
- `stratum-agent`: `ToolExecutor` orchestration, legacy `Agent` tool dispatch, approval terminology, cancellation/durability invariants, and kernel/legacy tests.
- `stratum-api` and `stratum-agent-builtin`: explicit policy selection while preserving `RequireApproval` behavior and the existing approval API.
- Public Rust APIs for tool registry construction and dispatch are intentionally changed; `stratum-core`, `stratum-config`, `stratum-infra`, wire events, and `stratum-web` require no capability changes.
