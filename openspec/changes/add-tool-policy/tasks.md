## 1. Define the policy and invocation model

- [ ] 1.1 Add `ToolPolicy`, `ToolPolicyDecision`, and trusted `ToolMetadata` types in `stratum-tools`, with explicit four-mode mapping and no implicit allow default.
- [ ] 1.2 Add private-field, non-cloneable denied, approval-required, and authorized invocation states that retain one resolved tool and owned validated `ToolInput` and are constructible only inside `stratum-tools`.
- [ ] 1.3 Keep lookup, validation, and execution failures in the existing typed `ToolError`; model policy denial as a prepared state rather than a new error enum, and document the low-level trusted `Tool::call` boundary separately from Agent dispatch.

## 2. Make registry preparation the normal dispatch path

- [ ] 2.1 Replace the public `ToolRegistry` trait and `BuiltinToolRegistry` implementation with one concrete `ToolRegistry`; expose synchronous `prepare`, provider-spec enumeration, and registration, but no raw Agent lookup or dispatch methods.
- [ ] 2.2 Implement `ToolRegistry::prepare` so it performs lookup, validation, policy evaluation, and invocation-state construction in that order, with any retained `Default` using `DenyAll`.
- [ ] 2.3 Add runtime registry tests covering every policy mode, authoritative registration metadata, missing/invalid short-circuiting, exact input binding, and at most one dispatch attempt through an authorized capability.
- [ ] 2.4 Add compile-fail doctests for the negative API guarantees: prepared states are not cloneable, denial has no authorization transition, external code cannot construct invocation states, and the registry exposes no raw dispatch path; do not add a test-only dependency for these checks.
- [ ] 2.5 Update direct builtin-tool tests to invoke `Tool::call` only where the test intentionally exercises the documented trusted low-level boundary.

## 3. Integrate the new AgentLoop tool executor

- [ ] 3.1 Migrate `ToolExecutor` to match prepared invocation states and emit the stable `tool_policy_denied` result without approval or execution-start events.
- [ ] 3.2 Preserve the current approval requested/resolved, cancellation rechecks, durable execution-start, post-start polling, and result-commit ordering for approval-required and allowed calls.
- [ ] 3.3 Update `ToolApproval` documentation to describe an approval interaction handler rather than the authorization policy.
- [ ] 3.4 Extend executor and AgentLoop kernel tests for denied, allowed, approved, rejected, invalid, cancelled, and durable-ack failure paths, including exact operation ordering, zero-dispatch assertions, and preservation of the current flat approval-rejection payload.

## 4. Integrate the legacy Agent path

- [ ] 4.1 Migrate legacy `Agent::execute_tool_call` to registry preparation so validation precedes policy and approval while retaining its existing event bus and in-memory approval interaction.
- [ ] 4.2 Return the same stable policy-denied result and preserve the legacy nested approval-rejection payload, sequential execution, and at-least-once resume behavior.
- [ ] 4.3 Add legacy tests for missing, invalid, denied, allowed, approved, and rejected calls; assert pending-clear → waiter-ack → awaited non-fatal resolved-publication attempt → conversion/rejection ordering and zero dispatch on rejection; prove that a resumed missing suffix is re-prepared under the current policy.

## 5. Update composition roots and compatibility surfaces

- [ ] 5.1 Update hosted API registry composition to select `ToolPolicy::RequireApproval` explicitly while retaining the current `echo` catalog and HTTP/SSE approval contract.
- [ ] 5.2 Update the REPL registry factory to select `ToolPolicy::RequireApproval`; update `build_default_agent`, its callers, and tests only for the concrete registry API while continuing to inject an already configured registry and without adding a second policy parameter or fields to `stratum-config`.
- [ ] 5.3 Run API and REPL approval-flow tests to verify that externally visible event payloads, the legacy nested rejection payload, approve/reject requests, waiter acknowledgement, awaited best-effort resolved publication, and one-shot approval semantics are unchanged.

## 6. Document and verify the change

- [ ] 6.1 Update `crates/stratum-tools/AGENTS.md` with policy ownership, prepared/authorized invocation, trusted registration metadata, default-deny, and in-process threat-model rules.
- [ ] 6.2 Update `crates/stratum-agent/AGENTS.md` and relevant protocol documentation with policy-versus-approval ownership, execution ordering, denial behavior, and unchanged legacy recovery semantics.
- [ ] 6.3 Run `cargo fmt` and the focused `stratum-tools`, `stratum-agent`, `stratum-agent-builtin`, and `stratum-api` test suites.
- [ ] 6.4 Run `cargo clippy --workspace --all-targets` and resolve warnings without silencing relevant lints.
- [ ] 6.5 Before PR merge, review and archive the final design and implementation conventions in the affected crate `AGENTS.md` files.
