# stratum-agent AGENTS.md

## Scope

`stratum-agent` contains the session-independent `AgentLoop` kernel and the
legacy stateful `Agent` compatibility path.

## AgentLoop Kernel

- `AgentLoop` consumes a caller-preloaded `LoopContext` plus user prompts. It
  does not own session creation, history loading, an `AgentStore`, or an
  `EventStreamBus`.
- Required transitions use `DurableEventSink`; model deltas and non-critical
  diagnostics use the separate best-effort `TelemetryEventSink`.
- A durable append must be acknowledged before the kernel mutates its in-memory
  transcript or starts the next external action.
- Tool calls execute sequentially through `ToolExecutor`. Lookup and synchronous,
  side-effect-free deterministic input validation happen before the tool hooks;
  hook-transformed arguments are re-validated before `decide_tool_call`.
  `ToolExecutionStarted` must be durable before dispatch, and each tool result
  must be durable before the next tool or model request.
- Cancellation is rechecked after validation and immediately before
  `ToolExecutionStarted`; before started is acknowledged, cancellation prevents
  dispatch and maps to loop cancellation.
- The run's supplied `CancellationToken` controls model-stream acquisition and
  polling in `AgentLoop`; the same token is passed to hook and tool
  operations. Cancellation is cooperative: after `ToolExecutionStarted`, the
  caller must keep polling the loop so it can await and record the outcome.
  A durable start without a result is an unknown outcome and is never retried
  automatically by the kernel.
- `ToolExecutor` is pure mechanics: lookup, validation, durable
  `ToolExecutionStarted`, and dispatch. `execute` is `pub(crate)` and takes the
  resolved tool handle plus the decide-approved final call; its body is only
  the cancellation check, the durable start, and the dispatch. It has no
  authorization concept, holds no approval policy, and emits no approval
  events; execution decisions belong to `decide_tool_call` hooks.
- Only a provider `FinishReason::ToolCalls` authorizes dispatch. If a response contains tool calls
  with `length`, `stop`, or another finish reason, commit structured tool-error messages without
  invoking the tools.
- `LoopLimits` bounds assistant text, reasoning, and each tool-call argument buffer in addition to
  iterations and tool-call count. Enforce limits before appending each streamed fragment; never
  retain an unbounded provider response.
- `ToolExecutor` is the single source of the durable sink used by `AgentLoop`; the builder must not
  accept a second sink that could split tool and loop boundaries across transports.

## Hook Runtime

- `hook_runtime` holds the single `HookRuntime` async trait (`runtime.rs`) and its
  `NoopHookRuntime` default (`noop.rs`); failure types come from
  `stratum-core::HookFailure` and the loop-side mapping lives in
  `agent_loop/error.rs` (`AgentLoopError::Hook`). Do not add per-hook closures
  to the builder: the runtime is the single composition boundary.
- The trait exposes exactly five hooks — `transform_context`,
  `transform_tool_call`, `decide_tool_call`, `after_tool_call`,
  `prepare_next_turn` — taking borrowed inputs and returning owned decisions.
  `AgentLoop` holds one `Arc<dyn HookRuntime>` injected via
  `AgentLoopBuilder::hook_runtime`; without injection the no-op runtime keeps
  the pre-hook message flow unchanged (journal records are still appended for
  every invocation).
- Every hook input embeds the same borrowed `HookSnapshot` (`iteration`,
  `&LoopContext`, `Option<TokenUsage>`). This is the wide-read/narrow-write
  principle: handlers may read ambient loop state, but their effects stay
  confined to the narrow typed decisions. New ambient input fields go into
  `HookSnapshot` only — never into the per-hook input structs, which carry
  just their point-specific payloads (`tool_call`, `tool`, `result`).
- `snapshot.context` is the committed context at that hook's boundary:
  `transform_context` sees committed context plus pending one-shot injects;
  tool hooks see the committed context including already-committed results of
  the current cycle; `after_tool_call` never sees its own uncommitted result
  (that lives in the `result` payload); `prepare_next_turn` sees the cycle's
  full committed results. `snapshot.usage` is the token usage reported by the
  most recent model response, or `None` when the provider never reported; the
  kernel passes it through without accumulating, and handlers needing
  cumulative semantics maintain their own totals.
- The three tool hooks receive a borrowed `ToolHookTarget` (effective
  authorization metadata `ToolKind`/`DangerLevel` plus `ToolSpec`) looked up by
  the kernel before the call; handlers never query the registry themselves.
  `authorization` is the effective per-call value: the registry-declared
  default at `transform_tool_call`, and the transform-overridden value (when
  any) at `decide_tool_call` and `after_tool_call`. The kernel transports the
  value without interpreting it. The registry declaration is only a default
  basis derived from the tool's registered `ToolKind`/`DangerLevel` and the
  registry's `ToolPermissionMode` (`stratum-tools`) — never a verdict: the
  judgment of whether a call needs approval lives in the hook chain.
  `ToolExecutor::hook_lookup` is the kernel's single isolation point over the
  registry: it resolves the missing-tool gate, the tool handle for dispatch,
  and this default declaration in one lookup.
- Identity is kernel-owned: hooks may never change `CallId` or tool names.
  `transform_tool_call` may only continue or return a `Modify` carrying
  optional replacement arguments and/or an optional authorization override
  (`PreAuthorize` or `Set`); a `Modify` with every field unchanged is rejected
  as `HookFailure::InvalidOutput`. The kernel re-validates transformed
  arguments before deciding, and carries the effective authorization to decide
  and after without any sanity checks (including downgrade checks) —
  overriding authorization is the handler's explicit responsibility.
  `decide_tool_call` may
  only `Execute` or `Block` — it can never modify arguments, so approvers
  always see exactly the parameters that will run. A block skips
  `ToolExecutionStarted` and yields the fixed
  `{"error":{"code":"hook_blocked",...}}` model-visible result, which still
  passes through `after_tool_call`. `after_tool_call` may only replace the
  JSON result; the kernel rebuilds the tool message with the original `CallId`.
- User approval is an ordinary `decide_tool_call` handler: approve maps to
  `Execute`, reject maps to `Block`, and the ask-a-human channel is private to
  the handler implementation. The kernel has no approval concept and the new
  kernel path emits no `ToolApprovalRequested`/`ToolApprovalResolved` events
  (the legacy Agent keeps its own approval path). A crash after approval but
  before dispatch is fail-safe: resume either reuses the journaled completed
  decision (the handler is not asked again) or retries the pending invocation
  under its original identity.
- `transform_context` patches and `prepare_next_turn` injection are
  request-only views: they never write back to the committed context, never
  emit durable messages, and never appear in `LoopOutcome.new_messages`.
  A `transform_context` decision is `Unchanged` or `Patch(ContextPatch)`
  (`ReplaceSystemPrompt` / `DropHistory { upto }` / `RewriteHistory { upto,
  summary }`); the kernel validates `upto` as a zero-based,
  left-closed/right-open prefix end into the committed `messages` that must
  stay in bounds and must not cut a tool_call/tool_result pair, rejecting
  invalid patches as `HookFailure::InvalidOutput`. Injected user messages are
  consumed exactly once by the next model request; empty or non-user-role
  injections are rejected as `HookFailure::InvalidOutput`.
- Every hook invocation is journaled into the same `DurableEventSink` stream:
  `HookInvocationPending` (address `(iteration, HookPoint, Option<CallId>)`
  plus a payload-level input digest) commits before the runtime call,
  `HookInvocationCompleted` commits after decision validation and before the
  affected action (model request, `ToolExecutionStarted`, result commit, or
  iteration boundary), and `HookInvocationFailed` commits for typed failures,
  deadlines, and invalid decisions. Tool-hook digests hash the canonical JSON
  of the exact `ToolCall` the hook observes; context hooks digest their
  `(iteration, point)` address. Usage and history never participate.
- `AgentLoop::resume` re-runs a run from its durable event stream: the
  composing side re-supplies the system prompt and configuration, replay
  rebuilds committed context from `MessageAppended`, fixes the frontier at one
  past the maximum `IterationCompleted`, and refuses terminal runs. Committed
  tool results must be the exact ordered prefix of the preceding assistant
  `tool_calls` (unknown, duplicate, sparse, or out-of-order results fail
  closed); the missing suffix re-executes under the at-least-once stance.
  Hook invocations consult the journal first: digest-matching completed
  decisions are reused without calling the runtime, pending invocations retry
  under their original identity, failures are reproduced, and digest
  mismatches fail closed.
- Every hook call goes through the kernel's shared execution helper: pre-call
  cancellation and in-flight cancellation resolve to loop cancellation, the
  absolute deadline maps to `HookFailure::TimedOut`, and runtime failures are
  reported as `AgentLoopError::Hook` carrying only the `HookPoint` and the safe
  failure category — never prompts, tool payloads, or handler internals.
  Deadlines are configured per hook point via `LoopLimits::hook_timeouts`;
  `decide_tool_call` defaults to no deadline (cancellation only) to accommodate
  human approval latency, while all other points default to a fail-closed
  timeout.
- `LoopOutcome.completion` is a `LoopCompletionReason` distinguishing
  `Model(FinishReason)` from `HookStopped`; the durable `LoopFinished` reason
  projects to stable strings such as `hook_stopped`. A hook stop must never be
  disguised as a provider finish reason.
- Deferred to later milestones (do not add here): multi-handler ordering and
  short-circuiting (H2), the unified `stratum-tools` validation boundary (H2),
  kernel-durable history compaction (H5), Skill/Script/service adapters, and
  hook telemetry or EventBus payloads.

## Legacy Agent Compatibility

The following rules describe the existing `Agent`, session, resume, store, and
`EventStreamBus` integration. This remains temporary compatibility code and is
not the ownership model for the new `AgentLoop` kernel.

- The Agent receives an injected `EventStreamBus` for event delivery and an
  injected `AgentStore` for durable resumption.
- The host supplies `AgentRuntimeContext`; the Agent creates only `TurnId`.
- `ModelConfig` may change between Turns. Each Agent instance uses the configuration selected by
  the host for its next Turn, while an active or resumed Turn must keep its pinned snapshot value.
- The Agent commits complete messages through `AgentStore::append_message` before publishing the
  returned sequenced envelope. Lifecycle and streaming events are observation-only envelopes.
- Retained event delivery remains an EventBus responsibility; the Store is durable truth.

## Turn Control (Legacy Agent)

- Use a bounded MPSC channel for interactive commands sent to an active turn.
- Keep cancellation on `CancellationToken` and prioritize it in `tokio::select!`.
- The agent owns approval interaction; `stratum-tools` owns authorization metadata.
- Publish `tool_approval_requested` successfully before waiting.
- Keep user-message queuing separate until its behavior is implemented.

## Resume (Legacy Agent)

- `Agent::resume()` takes no user message. It loads the injected store and continues the unfinished
  Turn with the same persisted Session, Turn, location, and runtime snapshot.
- Resume validates Agent/SkillSet/ExtensionSet/Handler versions, resolved model configuration, and
  ToolSet fingerprint before any model, Tool, or future Hook work. Missing or mismatched pinned
  components fail closed.
- `agent.json` records `next_iteration` as the durable iteration frontier: every
  lower iteration has committed its stable boundary, while the frontier has not.
  It is not simply the next LLM request because committed history may instead
  require tool reconciliation or terminal completion without another LLM call.
  Do not recover this frontier from JetStream metadata.
- Resume rebuilds conversation history only from committed complete messages
  through the fixed `last_seq` captured from the loaded state; realtime deltas
  are never resume state.
- Resume validates the active turn against `next_iteration`. Committed tool
  result messages must be the exact ordered prefix of the immediately preceding
  assistant `tool_calls`. Unknown, duplicate, sparse, or out-of-order results
  are invalid resume history; only the missing suffix executes.
- Advance `next_iteration` with the `agent.json` CAS only after the assistant
  message and every tool result message for the iteration are durably committed.
- Resumed LLM, tool, complete-message, and lifecycle events continue through the
  injected `EventStreamBus`; resume does not publish directly to the store or
  retained transport.
- Tool execution is at-least-once. A process may stop after a tool has produced
  an external side effect but before its result message is committed, causing
  resume to execute that tool again. Every tool implementation must therefore
  guarantee idempotent execution for the same tool call.
- Web or scheduler composition guarantees that only the Agent owner resumes and
  writes the turn; `stratum-agent` does not add a second writer lease.
