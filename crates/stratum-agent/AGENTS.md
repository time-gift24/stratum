# stratum-agent AGENTS.md

## Scope

`stratum-agent` contains only the session-independent `AgentLoop` kernel. The legacy stateful
`Agent` compatibility path is deleted. Postgres, HTTP, Session, hosting, scheduler, and
pagination must never enter this crate.

## AgentLoop Kernel

- `AgentLoop` consumes a caller-preloaded `LoopContext` plus user prompts. It
  does not own session creation, history loading, durable storage, or any
  realtime transport.
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
  During the live run the kernel never guesses or proactively retries a durable
  start without a result. On explicit resume, the missing durable result suffix
  re-executes under the at-least-once contract; idempotency belongs to the Tool
  or external service, with side-effect protection owned by composition.
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
  the handler implementation. The kernel has no approval concept and never
  emits `ToolApprovalRequested`/`ToolApprovalResolved` itself; those durable
  approval facts are written through the sink by the composition-side approval
  handler in `stratum-api`. A crash after approval but
  before dispatch is fail-safe: resume either reuses the journaled completed
  decision (the handler is not asked again) or retries the pending invocation
  under its original identity.
- `transform_context` patches and `prepare_next_turn` injection are
  request-only views: they never write back to the committed context, never
  emit durable messages, and never appear in `LoopOutcome.new_messages`.
  A `transform_context` decision is `Unchanged` or `Patch(ContextPatch)`
  (`ReplaceSystemPrompt` / `DropHistory { upto }` / `RewriteHistory { upto,
  summary }` / `Composite`); the kernel validates `upto` as a zero-based,
  left-closed/right-open prefix end into the committed `messages` that must
  stay in bounds and must not cut a tool_call/tool_result pair, rejecting
  invalid patches as `HookFailure::InvalidOutput`. A `Composite` validates its
  sub-patches in order against the evolving view each one produces; empty and
  nested compositions are rejected as `HookFailure::InvalidOutput` (a nested
  composition cannot advance the validation view and could otherwise panic at
  apply time). Injected user messages are
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
  past the maximum `IterationCompleted`, and refuses terminal runs. Event
  variants the kernel does not understand fail closed as
  `ResumeError::UnsupportedEvent`; only the approval fact events
  (`ToolApprovalRequested`/`ToolApprovalResolved`) are explicitly skipped
  because they carry no kernel resume state. Committed
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
- `ChainHookRuntime` (`hook_runtime/chain.rs`) is the ordered handler-chain
  `HookRuntime`; the kernel still sees one runtime, so cancellation, deadline,
  and hook-point journal semantics are unchanged. `HookHandler`
  (`hook_runtime/handler.rs`) mirrors the five hook methods with no-op
  defaults plus an immutable `HookHandlerVersionId` descriptor. Chain
  semantics per point: transform/after thread the evolving view through
  handlers in order (Cow, zero-copy when unmodified); decide short-circuits
  on the first `Block`; prepare short-circuits on `Stop` (dropping collected
  injects) and merges multiple `Inject` payloads in handler order. Any handler
  failure or invalid decision fails the whole point closed.
- Chain order is pinned data, not private code: a `ChainHookRuntime` computes
  its `ExtensionSetVersionId` from the ordered handler versions at
  construction, the kernel commits it with `LoopStarted`, and `resume` fails
  closed when the injected runtime reports a different version. Runtimes
  reporting no version skip the check.
- Handler version identity is self-declared by the handler author via
  `HookHandler::descriptor()`; the kernel and chain only consume it. The
  contract has two halves: the id must be *stable* for one handler version
  (create it once at construction or derive it deterministically — never call
  `HookHandlerVersionId::new()` inside `descriptor()` per invocation, which
  would change the chain version every call and make every resume fail), and
  any change to decision behavior must come with a new id. The kernel can
  detect "the id changed" but not "behavior changed while the id stayed" —
  that gap closes when handlers become distributable artifacts whose version
  derives from a content digest (S1/S2) or a pinned service identity (R3).
- `prepare_next_turn` may also return `Compact { upto, summary }`: the handler
  supplies the summary, the kernel executes the durable compaction at the
  iteration boundary. Invariants enforced before commit: `upto` is nonzero,
  in bounds, never splits a tool_call/tool_result pair, and never cuts into
  the current iteration's committed messages; the summary must be a plain
  system message (no tool identity, no reasoning). `Compact.upto` is always
  an index into the committed context shown by the prepare snapshot — never
  reuse indices computed from a patched request view, and recompute after
  every compaction. The kernel wraps the summary with the stable marker
  template (`COMPACTION_MARKER_PREFIX` + newline + body, see
  `agent_loop/compaction.rs`) and commits `TranscriptCompacted` before the
  iteration boundary; the marker message is committed history (and appears in
  `LoopOutcome.new_messages`), so handlers can detect a past compaction at
  the head of the snapshot context.
- Compaction is replay-safe: the event log keeps every original message and
  replay applies `TranscriptCompacted` in order; a crash between the
  journaled `Completed(Compact)` and the compaction event is closed by
  replaying the recorded summary — the handler is never re-invoked and the
  summary is never regenerated. `compacted_iterations` dedupes a crash that
  lands after the compaction event but before the iteration boundary.
  Compaction never changes hook addressing or digests.
- Deferred to later milestones (do not add here): per-handler journal
  granularity (H3b evaluation), Skill/Script/service adapters, and hook
  telemetry or realtime transport payloads.
  Also recorded for evaluation: the resume chain-version check passes when
  the event stream recorded a version but the injected runtime reports none
  (replacing the chain with a version-less runtime bypasses the guard) —
  decide whether that combination should fail closed.

## Prepared Resume Seam

- The only approved kernel seam for resume composition is pure `prepare_resume`: an exact
  `Arc<AgentLoop>` produces an opaque prepared value bound to that same runtime — not `Clone`,
  not `Serialize` — that exposes a single consuming `run(token)` path.
- `prepare_resume` performs no I/O: no durable append, no model/tool/hook calls, and it never
  receives Postgres, Session, hosting, or pagination concerns. The composing side (`stratum-api`)
  builds and validates the typed replay window; the prepared value reuses the existing private
  replay validator so resume composition never duplicates the kernel state machine.
- Fresh run, durable sink acknowledgement, and sequential tool execution semantics are unchanged.
