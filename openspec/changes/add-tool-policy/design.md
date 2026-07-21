## Context

`stratum-tools::ToolRegistry` currently combines five responsibilities: catalog lookup, provider specification enumeration, registration metadata, permission-mode evaluation, and raw dispatch. Its `authorization()` API compresses the result into `Option<(ToolKind, DangerLevel)>`: `None` means allow and `Some` means ask for approval. It cannot express denial, and callers that invoke `call()` directly bypass the permission check.

There are also two callers with different orchestration:

- The session-independent `AgentLoop` owns a concrete `ToolExecutor`. The executor performs lookup, validation, approval, durable `ToolExecutionStarted`, and dispatch; the loop durably appends the returned tool message.
- The hosted API and REPL still use the legacy `Agent`, which directly interprets `ToolRegistry::authorization()`, owns an in-memory approval oneshot, and then calls the registry. It does not currently perform explicit registry validation before asking for approval.

The new contract must preserve the kernel's ordering and cancellation invariants, retain the legacy HTTP/SSE approval protocol, and keep provider-visible `ToolSpec` independent from runtime authorization metadata. The security boundary protects model-originated calls and normal runtime composition; it is not a sandbox against trusted Rust code that deliberately retains and directly invokes a `Tool` instance.

## Goals / Non-Goals

**Goals:**

- Give every prepared tool call an explicit `Deny`, `RequireApproval`, or `Allow` policy outcome.
- Make lookup and side-effect-free validation precede policy evaluation.
- Bind the resolved tool, trusted registration metadata, and exact validated input through approval to one consuming execution capability.
- Remove raw dispatch from the registry caller surface so an Agent cannot accidentally skip policy.
- Preserve approval durability, cancellation, sequential execution, and committed tool-result ordering in the new kernel.
- Make the legacy Agent and the new ToolExecutor consume the same preparation contract while preserving their existing external protocols and recovery models.
- Require composition roots to choose an immutable policy explicitly and retain the current hosted `RequireApproval` behavior.

**Non-Goals:**

- A policy trait, plugin interface, rule chain, CEL/Rego/OPA integration, or remote/async evaluator.
- Rules based on user, tenant, Agent, run, turn, time, environment, or an untyped context map.
- Argument- or resource-level rules. Raw JSON paths are not canonical resource facts and must not become an authorization language in this change.
- Remembered approvals, persisted pending approvals, policy revision snapshots, or changes to legacy at-least-once resume semantics.
- Merging the legacy Agent into `AgentLoop`, changing the HTTP approval endpoint, changing wire events, or changing the frontend.
- Preventing a trusted in-process registrant from retaining and directly calling the `Tool` value it registered.

## Decisions

### 1. `stratum-tools` owns a closed policy, concrete registry, and invocation states

Introduce an immutable `ToolPolicy` enum in `stratum-tools` with four policies:

| Policy | `Read + Low` | Other registered tools |
| --- | --- | --- |
| `DenyAll` | `Deny` | `Deny` |
| `RequireApproval` | `RequireApproval` | `RequireApproval` |
| `AllowLowRiskReads` | `Allow` | `RequireApproval` |
| `AllowAll` | `Allow` | `Allow` |

The result is a separate `ToolPolicyDecision::{Deny, RequireApproval, Allow}`. Policy evaluation is synchronous and infallible because the first implementation is only this local matrix. The registry stores the selected policy together with immutable registration metadata and evaluates it during preparation.

Replace the public `ToolRegistry` trait and its single `BuiltinToolRegistry` implementation with one public concrete `ToolRegistry`. The concrete registry remains extensible through registration of `Arc<dyn Tool>`, but only `stratum-tools` can construct prepared invocation states. Agent tests that currently implement a mock registry instead register a mock `Tool` in a real registry. This resolves the conflict between private capability fields and a public trait whose external implementers would otherwise need a safe way to manufacture those capabilities. It also removes a single-implementation trait that does not isolate an external boundary.

`ToolPolicy` will not implement an implicit allow default. `ToolRegistry::default()`, if retained for test and builder ergonomics, uses `DenyAll`; production composition uses an explicit constructor.

This replaces `ToolPermissionMode`. A policy trait is intentionally deferred: the repository has only one real evaluator, and four enum branches do not justify four implementations plus dynamic dispatch. A trait can be introduced when a second concrete evaluator exists; evaluator failures must then fail closed.

**Alternatives considered:**

- Put policy on each `Tool`: rejected because tools describe effects, while authorization belongs to the host and can differ between deployments.
- Put policy in `Agent`: rejected because it would increase Agent/Tool coupling and duplicate policy rules across the two Agent paths.
- Keep `authorization() -> Option<_>`: rejected because it cannot represent denial and makes metadata disappear on the allow path.
- Keep an externally implementable registry trait: rejected because external implementations would either need a public authorized-state constructor that bypasses the gate or could not implement `prepare`; the repository has only one real registry implementation.
- Introduce a general rule engine now: rejected because there is no caller or stable typed context for it.

### 2. Preparation returns a typed, input-bound invocation state

Replace the normal sequence of `authorization`, `validate`, `get`, and `call` with one synchronous registry operation conceptually shaped as:

```text
prepare(tool_name, owned ToolInput)
  ├─ resolve registered tool and trusted ToolMetadata
  ├─ validate exact input without side effects
  ├─ evaluate immutable ToolPolicy
  └─ return one of:
       DeniedToolCall
       ApprovalRequiredToolCall
       AuthorizedToolCall
```

The concrete public shape may be a `PreparedToolCall` enum containing those state types. Their tool handle, metadata, and `ToolInput` fields remain private. They expose borrowed facts needed for events and structured results but are not `Clone`.

- `DeniedToolCall` has no conversion to an executable state.
- `ApprovalRequiredToolCall` exposes a consuming transition into `AuthorizedToolCall`. Trusted Agent orchestration is responsible for calling that transition only after accepting `Approve` and, in the new `ToolExecutor`, durably acknowledging the resolution; rejection drops the pending call.
- `AuthorizedToolCall::execute(self, cancellation)` consumes the capability and permits at most one dispatch attempt through that capability with the bound tool and input.

The concrete registry does not expose `authorization`, `validate`, `get`, or raw `call` to normal Agent callers. `Tool::call` remains public for tool authors and focused tests and continues to revalidate its own input; this is an explicitly trusted low-level boundary, not an Agent dispatch path.

The small invocation state types are justified because they structurally encode three important invariants: policy cannot be skipped accidentally, a `Deny` has no upgrade path, and the input shown for approval is the input eventually executed. They do not prove the provenance of a human approval or that an event was persisted; those remain trusted Agent orchestration contracts verified by ordering tests. They also provide single-use capability semantics, not exactly-once external effects. Passing `Option<ApprovalDecision>` into a raw `call()` was rejected because it leaves the structural relationships conventional and easy to misuse.

### 3. Policy runs after deterministic validation and before approval

Both Agent paths use this order:

```text
lookup + validate + policy
  ├─ Deny ────────────────> structured tool result; no approval or dispatch
  ├─ Allow ───────────────> execution-start boundary; dispatch
  └─ RequireApproval
       ├─ request approval
       ├─ Reject ─────────> structured tool result; no dispatch
       └─ Approve ────────> execution-start boundary; dispatch
```

Missing tools and invalid input terminate before policy, approval, execution-start, or dispatch. Registration-provided `ToolKind` and `DangerLevel` are the trusted facts; model arguments cannot override them.

For the new `ToolExecutor`, the existing durable ordering remains authoritative:

1. `ToolApprovalRequested` is acknowledged before an approval handler is polled.
2. `ToolApprovalResolved` is acknowledged before an approved call can advance.
3. Cancellation is rechecked at the existing pre-start boundaries.
4. `ToolExecutionStarted` is acknowledged before `AuthorizedToolCall::execute` begins.
5. Once execution has started, its future is awaited and the `AgentLoop` durably appends the result before another tool or model call.

A policy denial is a normal, model-visible result with the stable payload:

```json
{
  "error": {
    "type": "tool_policy_denied",
    "message": "tool call denied by policy"
  }
}
```

It does not add a new durable policy event. The resulting tool message is the durable record, while approval and execution events retain their existing meanings. Avoiding a new core event also avoids coupling policy to projection and transport code.

`ToolApproval` remains the asynchronous, cancellable interaction boundary only. Its documentation will stop calling it the authorization policy so that a human approval result is not confused with `ToolPolicyDecision`.

### 4. Both Agent paths adopt the same registry contract without a runtime merger

`ToolExecutor` matches the prepared state and retains all current kernel ordering. The legacy `Agent::execute_tool_call` also switches to `prepare` and therefore gains pre-approval validation and the same allow/ask/deny matrix. It keeps its existing in-memory oneshot, `AgentEvent` projection, HTTP/REPL decision flow, and at-least-once recovery behavior.

The legacy approval request retains its current required publication/handshake behavior. After a decision arrives, the legacy Agent preserves this exact order: clear the pending approval, acknowledge the HTTP/REPL waiter, await one `ToolApprovalResolved` publication attempt while treating publication failure as non-fatal, and only then convert an approved invocation or produce the rejection result. This is an awaited best-effort attempt, not fire-and-forget publication. Durable acknowledgement of the approval resolution is a `ToolExecutor` invariant only; this change does not silently strengthen the legacy event contract.

Approval rejection payloads remain runtime-specific because they already differ and normalization is unrelated to the policy boundary. The legacy Agent retains `{"error":{"type":"approval_rejected","message":"user rejected tool call"}}`; the new `ToolExecutor` retains `{"error":"tool approval rejected"}`. Both paths share the no-authorization/no-dispatch semantics, while only the newly introduced policy-denial payload is standardized by this change.

The change will not wrap the legacy Agent in the new `ToolExecutor`: their event sinks, agent-name projection, result commit ownership, and recovery contracts differ. Duplicating a small state match during the temporary compatibility period is safer than adding an adapter layer that pretends those contracts are equivalent.

On legacy resume, a missing tool-result suffix is prepared again under the currently composed immutable policy. A previous in-memory pending approval is not recovered or reused, matching current behavior. No persisted data migration or policy snapshot is introduced.

### 5. Exposure, policy, and approval remain separate controls

The agent definition's `tools` list remains the exposure allowlist and determines which provider specs are registered. `ToolPolicy` decides whether a call to an already registered tool is allowed, denied, or requires approval. Human approval can authorize only `RequireApproval`; it cannot make an unregistered or policy-denied tool executable.

Provider-visible `ToolSpec` remains name, description, and input schema only. Policy, `ToolKind`, and `DangerLevel` stay runtime-only and do not alter the LLM protocol.

Hosted API and REPL registries explicitly select `ToolPolicy::RequireApproval`, so their behavior remains unchanged. This change does not add policy fields to `stratum-config` or persisted `ResolvedAgentDefinition`.

## Risks / Trade-offs

- **Three invocation state types increase the public Rust API surface.** → Keep their fields private, make them consuming and non-cloneable, and expose only the facts required by Agent orchestration. They replace several existing registry methods rather than layering over them.
- **Registration metadata can misclassify a tool.** → Treat registration as a trusted composition boundary, keep metadata immutable after sharing, and test every builtin registration. Dynamic resource facts are deliberately out of scope.
- **The initial policy cannot express path, argument, identity, or tenant rules.** → Prefer injected capabilities such as scoped filesystems for current containment. Add canonical tool-produced effect facts only when a concrete resource-policy requirement exists; do not parse raw JSON paths as policy.
- **Legacy and kernel orchestration remain duplicated.** → Share policy decisions and the prepared invocation contract, test common no-dispatch semantics plus each runtime's existing rejection payload and event ordering, and avoid a speculative compatibility adapter while legacy remains temporary.
- **A permanently denied registered tool is still visible to the model.** → Keep exposure and execution policy explicit. Remove such a tool from the agent allowlist when it should not be advertised; retain runtime denial as the fail-closed boundary.
- **Trusted Rust code can still bypass the Agent boundary by retaining a `Tool`.** → Document the threat model. This change prevents normal Agent and registry callers from accidental raw dispatch; it is not an in-process sandbox.
- **A policy change can affect a resumed missing call.** → Re-evaluate under the policy selected by current composition and request fresh approval when required, matching existing non-persisted approval semantics.

## Migration Plan

1. Add policy, metadata, prepared-state, and authorized-call types to `stratum-tools`; replace the single-implementation registry trait with a concrete registry and cover the complete policy matrix with unit tests. Existing typed `ToolError` remains the error channel for lookup, validation, and execution; policy denial is an invocation state, not a new error enum.
2. Migrate `ToolExecutor` to prepared calls while preserving durable ordering, cancellation boundaries, and result ownership.
3. Migrate the legacy Agent to the same preparation API and add parity tests for invalid, denied, allowed, approved, and rejected calls.
4. Update hosted API and REPL registry factories to select `RequireApproval` explicitly. Migrate `build_default_agent`, mocks, and tests only to the concrete registry type while continuing to inject an already configured registry.
5. Update crate `AGENTS.md` files and protocol-facing documentation to record the ownership and threat-model decisions.

There is no persisted schema or wire migration. Rollback consists of reverting the Rust API and composition changes; existing agent definitions and message history remain readable because neither contains policy state.

## Open Questions

None block the initial implementation. Argument/resource policies, identity context, policy configuration, and persisted approval recovery require separate concrete use cases and proposals.
