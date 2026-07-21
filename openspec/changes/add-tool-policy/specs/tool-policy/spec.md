## ADDED Requirements

### Requirement: Explicit immutable tool policy
Every runtime tool registry SHALL select one immutable `ToolPolicy` before it is shared. The supported policies SHALL map trusted registration metadata to decisions as follows: `DenyAll` denies every call, `RequireApproval` requires approval for every call, `AllowLowRiskReads` allows only `ToolKind::Read` with `DangerLevel::Low` and requires approval for all other calls, and `AllowAll` allows every call. No default construction path SHALL implicitly allow execution.

#### Scenario: Deny-all policy
- **WHEN** any registered tool is prepared under `DenyAll`
- **THEN** the policy decision is `Deny`

#### Scenario: Require-approval policy
- **WHEN** any registered tool is prepared under `RequireApproval`
- **THEN** the policy decision is `RequireApproval`

#### Scenario: Low-risk read policy allows the narrow case
- **WHEN** a tool registered as `Read + Low` is prepared under `AllowLowRiskReads`
- **THEN** the policy decision is `Allow`

#### Scenario: Low-risk read policy gates every other case
- **WHEN** a tool not registered as exactly `Read + Low` is prepared under `AllowLowRiskReads`
- **THEN** the policy decision is `RequireApproval`

#### Scenario: Allow-all policy
- **WHEN** any registered tool is prepared under `AllowAll`
- **THEN** the policy decision is `Allow`

### Requirement: Lookup and validation precede policy
The registry SHALL resolve the registered tool and its trusted `ToolKind` and `DangerLevel`, then synchronously validate the exact `ToolInput`, before evaluating policy. A missing tool or invalid input SHALL produce a recoverable tool failure without evaluating policy, requesting approval, recording execution-start, or dispatching the tool.

#### Scenario: Missing tool stops before policy
- **WHEN** an Agent prepares a call whose tool name is not registered
- **THEN** it receives a tool-not-found failure and no policy, approval, execution-start, or tool-call operation occurs

#### Scenario: Invalid input stops before policy
- **WHEN** the registered tool rejects the supplied input during side-effect-free validation
- **THEN** the invalid-input failure is returned to the model without policy, approval, execution-start, or dispatch

#### Scenario: Registration metadata is authoritative
- **WHEN** provider arguments contain values that claim a different tool kind or danger level
- **THEN** policy uses only the immutable metadata supplied by trusted registration

### Requirement: Prepared calls bind policy to exact execution
Successful preparation SHALL privately bind the resolved tool, trusted metadata, and owned validated input into exactly one of `Denied`, `ApprovalRequired`, or `Authorized` invocation states. `stratum-tools` SHALL expose one concrete registry as the sole constructor of these states rather than an externally implementable registry trait. The registry interface exposed to Agent callers SHALL NOT expose raw tool lookup or dispatch methods that bypass this state transition. An authorized call SHALL be non-cloneable and SHALL consume itself when executed.

#### Scenario: Allowed call is bound to its validated input
- **WHEN** policy returns `Allow` for a validated call
- **THEN** preparation returns an authorized invocation that can execute only the bound tool with the bound call ID and arguments

#### Scenario: Authorized capability is single-use
- **WHEN** an authorized invocation is executed
- **THEN** ownership is consumed so the same invocation cannot be dispatched a second time

#### Scenario: Tests extend through mock tools
- **WHEN** an Agent test needs controlled tool behavior
- **THEN** it registers a mock `Tool` in the concrete registry rather than implementing a registry or constructing an invocation state directly

#### Scenario: Denial cannot be upgraded
- **WHEN** policy returns `Deny`
- **THEN** no API on the denied invocation can convert an approval response into an authorized invocation

#### Scenario: Approval authorizes the prepared call only
- **WHEN** a required approval is granted
- **THEN** the resulting authorized invocation retains the same tool, metadata, call ID, and arguments that were shown in the approval request

### Requirement: Policy denial fails closed as a tool result
A `Deny` decision SHALL prevent approval and dispatch and SHALL produce the stable model-visible payload `{"error":{"type":"tool_policy_denied","message":"tool call denied by policy"}}`. The denial SHALL NOT emit `ToolApprovalRequested`, `ToolApprovalResolved`, or `ToolExecutionStarted`.

#### Scenario: Denied call is not dispatched
- **WHEN** a prepared call has the `Deny` decision
- **THEN** the denial payload is committed as the tool result and the tool is never invoked

### Requirement: Approval remains a separate interaction
`RequireApproval` SHALL cause Agent orchestration, not `stratum-tools`, to perform the existing asynchronous and cancellable approval interaction. The request SHALL use the bound call ID, tool name, arguments, `ToolKind`, and `DangerLevel`. Trusted Agent orchestration SHALL be responsible for invoking the public consuming transition only after it accepts an approval according to that runtime's event contract; the invocation type does not prove approval provenance or persistence. Approval rejection SHALL preserve that runtime's existing rejection payload and SHALL NOT authorize or dispatch the call. This change SHALL NOT normalize the legacy payload `{"error":{"type":"approval_rejected","message":"user rejected tool call"}}` and the new `ToolExecutor` payload `{"error":"tool approval rejected"}`.

#### Scenario: Approval request uses bound facts
- **WHEN** a prepared call requires approval
- **THEN** the approval event and approval handler request contain the exact bound call identity, arguments, and trusted metadata

#### Scenario: Approved call advances to execution
- **WHEN** the new `ToolExecutor` resolves an approval request as `Approve` and durably acknowledges the required resolution event
- **THEN** its trusted orchestration converts that approval-required invocation into an authorized invocation

#### Scenario: Legacy approval keeps its current resolution ordering
- **WHEN** the legacy Agent receives `Approve` or `Reject` through its existing approval-response handshake
- **THEN** it clears the pending approval, acknowledges the waiter, awaits one `ToolApprovalResolved` publication attempt with non-fatal publication failure, and only afterward converts the bound invocation or produces the rejection result

#### Scenario: Rejected call does not execute
- **WHEN** the approval request is resolved as `Reject`
- **THEN** that runtime's unchanged approval-rejected tool result is committed without execution-start or dispatch

#### Scenario: Approval failure fails closed
- **WHEN** approval is cancelled, its backend fails, or a durable approval event required by `ToolExecutor` is not acknowledged
- **THEN** no authorized invocation is produced and the tool is not dispatched

### Requirement: Authorized execution preserves kernel durability
The new `ToolExecutor` SHALL preserve its existing cancellation and durability boundaries around authorized calls. It SHALL durably acknowledge `ToolExecutionStarted` before invoking the authorized call; after that acknowledgement it SHALL await the tool outcome and the `AgentLoop` SHALL durably append the resulting tool message before another tool or model request starts.

#### Scenario: Execution start precedes dispatch
- **WHEN** an allowed or approved call reaches the execution boundary
- **THEN** `ToolExecutionStarted` is durably acknowledged before the capability makes its single dispatch attempt

#### Scenario: Cancellation before start prevents dispatch
- **WHEN** cancellation is observed before `ToolExecutionStarted` is acknowledged
- **THEN** the authorized invocation is not executed

#### Scenario: Started execution is driven to an outcome
- **WHEN** cancellation occurs after `ToolExecutionStarted` is acknowledged
- **THEN** the executor continues polling the tool and the loop records its outcome rather than dropping or racing the execution future

### Requirement: Both Agent paths enforce the same policy decisions
The new `ToolExecutor` and the legacy `Agent` SHALL both use the registry preparation and authorized-invocation contract for model-originated calls. They SHALL agree on missing, invalid, policy-denied, approval-required, allowed, and no-dispatch-on-rejection semantics while retaining their existing event sinks, runtime-specific approval-rejection payloads, ordering, and recovery contracts.

#### Scenario: Hosted API retains explicit approval
- **WHEN** the hosted API or REPL composes its tool registry after this change
- **THEN** it explicitly selects `RequireApproval` and continues exposing the existing approve/reject event and endpoint behavior

#### Scenario: Legacy validation precedes approval
- **WHEN** the legacy Agent receives invalid input for a registered tool
- **THEN** it returns the validation failure without creating a pending approval

#### Scenario: Legacy resume reevaluates a missing call
- **WHEN** legacy resume finds a committed assistant tool call without its tool result
- **THEN** it prepares the call under the currently composed policy and does not reuse any pre-crash in-memory approval

### Requirement: Tool exposure remains independent from execution policy
The agent definition's tool list SHALL remain the allowlist for provider-visible tool exposure. Runtime policy and registration metadata SHALL NOT be added to `ToolSpec`, the LLM request schema, the HTTP approval schema, or persisted `ResolvedAgentDefinition` by this change. Policy SHALL NOT authorize an unregistered tool.

#### Scenario: Unregistered tool stays unavailable
- **WHEN** policy is `AllowAll` but the requested tool is absent from the agent's registered allowlist
- **THEN** preparation returns tool-not-found rather than an authorized invocation

#### Scenario: Existing wire contracts remain stable
- **WHEN** a registered call requires human approval
- **THEN** existing approval request/resolution event shapes and the approve/reject HTTP request shape remain unchanged
