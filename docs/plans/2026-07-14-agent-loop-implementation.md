# Agent Loop Kernel Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a session-independent Stratum agent loop that advances LLM and tool turns, commits every critical transition through a fail-closed durable event sink, and emits best-effort telemetry through a separate port.

**Architecture:** Add strongly typed loop events in `stratum-core`, delivery ports and scoped event-stream implementations in `stratum-infra`, and a concrete `AgentLoop` plus `ToolExecutor` in `stratum-agent`. Keep the existing stateful `Agent` and resume path compatible during this phase; the new kernel must not call `AgentStore` or `EventStreamBus` directly. Extend the existing store-backed event consumer so durable loop events update store projections before acknowledgement.

**Tech Stack:** Rust 2024, Tokio, `CancellationToken`, futures streams, `thiserror`, Serde, existing `LlmProvider`, `ToolRegistry`, `AgentStore`, and `EventStreamBus` abstractions.

**Design:** `docs/plans/2026-07-14-agent-loop-design.md`

**Required skills during execution:** `@superpowers:test-driven-development`, `@rust-skills`, and `@superpowers:verification-before-completion`.

---

## Implementation constraints

- Work only in `/Users/wanyaozhong/Projects/wyse-agent-os/.worktrees/plugin-system-design` on `codex/plugin-system-design`.
- Preserve the existing `Agent` API and resume tests. Session creation, history loading, and resume redesign are outside this plan.
- Do not add a crate.
- Do not add parallel tool execution, retries, steering queues, context compaction, or plugin hooks.
- Keep `ToolExecutor` concrete. Use traits only for the already-real LLM, approval, durable-event, telemetry-event, and tool-registry boundaries.
- Define traits and errors in separate files. Put concrete implementations in capability-named files.
- Use `thiserror` in library crates, native typed IDs, and no production `unwrap()`.
- Run `cargo fmt` before every commit containing Rust changes.

### Dependency placement note

The approved design describes the sink contracts conceptually as agent-loop ports. During this first compatible slice, place those contracts in `stratum-infra::agent_event_sink`, because `stratum-agent` still depends on `stratum-store` for the legacy session/resume implementation. Defining the contracts in `stratum-agent` now would force `stratum-store -> stratum-agent -> stratum-store`. The new `AgentLoop` still depends only on `DurableEventSink` and `TelemetryEventSink`, never on `EventStreamBus`.

## Task 1: Add strongly typed loop event payloads

**Files:**

- Create: `crates/stratum-core/src/agent_loop_event.rs`
- Modify: `crates/stratum-core/src/lib.rs`
- Test: `crates/stratum-core/src/agent_loop_event.rs`

**Step 1: Write failing serialization and classification tests**

Add tests at the end of the new module that construct one durable message event and one telemetry delta event. Assert their snake-case serialization and assert that the two enums cannot be interchanged by using distinct helper function parameter types.

The public shapes should start with:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DurableAgentEvent {
    LoopStarted,
    MessageAppended { message: ChatMessage },
    ToolApprovalRequested {
        approval_id: ApprovalId,
        call_id: CallId,
        tool_name: ToolName,
        arguments: Value,
        tool_kind: ToolKind,
        danger_level: DangerLevel,
    },
    ToolApprovalResolved {
        approval_id: ApprovalId,
        decision: ApprovalDecision,
    },
    ToolExecutionStarted {
        call_id: CallId,
        tool_name: ToolName,
    },
    IterationCompleted {
        iteration: u64,
        usage: TokenUsage,
    },
    LoopFinished {
        finish_reason: String,
        usage: TokenUsage,
    },
    LoopFailed {
        error_text: String,
        usage: TokenUsage,
    },
    LoopCancelled { usage: TokenUsage },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
#[non_exhaustive]
pub enum AgentTelemetryEvent {
    LlmStarted { llm_call_id: LlmCallId },
    TextDelta { llm_call_id: LlmCallId, delta: String },
    ReasoningDelta { llm_call_id: LlmCallId, delta: String },
    ToolCallDelta {
        llm_call_id: LlmCallId,
        call_id: CallId,
        name: Option<String>,
        arguments_delta: String,
    },
    LlmFinished {
        llm_call_id: LlmCallId,
        finish_reason: String,
        usage: Option<TokenUsage>,
    },
    ToolExecutionProgress { call_id: CallId, update: Value },
}
```

Do not add run/session IDs to these payloads. Scope is attached by the sink implementation.

**Step 2: Run the focused test and verify failure**

Run:

```bash
cargo test -p stratum-core agent_loop_event -- --nocapture
```

Expected: FAIL because `agent_loop_event` is not exported or the event types are not implemented.

**Step 3: Implement the event module and re-exports**

Add module-level `//!` documentation, the two enums, and small `event_type()` methods returning stable snake-case names. Re-export both enums from `stratum-core/src/lib.rs`.

Keep existing `AgentEvent` unchanged in this task; compatibility conversion belongs to Task 2.

**Step 4: Run tests and formatting**

Run:

```bash
cargo fmt --all
cargo test -p stratum-core agent_loop_event
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-core/src/agent_loop_event.rs crates/stratum-core/src/lib.rs
git commit -m "feat(core): add typed agent loop events"
```

## Task 2: Add durable and telemetry event ports

**Files:**

- Create: `crates/stratum-infra/src/agent_event_sink/mod.rs`
- Create: `crates/stratum-infra/src/agent_event_sink/definition.rs`
- Create: `crates/stratum-infra/src/agent_event_sink/error.rs`
- Create: `crates/stratum-infra/src/agent_event_sink/scoped.rs`
- Modify: `crates/stratum-infra/src/lib.rs`
- Modify: `crates/stratum-core/src/lib.rs`
- Test: `crates/stratum-infra/src/agent_event_sink/scoped.rs`

**Step 1: Write failing sink behavior tests**

Create a recording `EventStreamBus` and a failing `EventStreamBus` in the `scoped.rs` test module. Verify:

1. A durable event is converted to a scoped `StreamEnvelope` with the configured `AgentId`, `RunId`, and `TurnId` and returns the bus error.
2. A telemetry event is converted and published, but a bus error does not escape `emit()`.
3. A committed message maps to the existing external `AgentEvent::Message` shape.

Use a constructor shaped like:

```rust
ScopedAgentEventSink::new(
    agent_id,
    agent_name,
    run_id,
    turn_id,
    Arc::clone(&event_bus),
)
```

**Step 2: Run the focused test and verify failure**

Run:

```bash
cargo test -p stratum-infra agent_event_sink -- --nocapture
```

Expected: FAIL because the sink traits and scoped implementation do not exist.

**Step 3: Implement the two ports and scoped adapter**

In `definition.rs` define dyn-compatible async traits using the crate's existing `async_trait` dependency:

```rust
#[async_trait]
pub trait DurableEventSink: Send + Sync {
    async fn append(&self, event: DurableAgentEvent) -> Result<(), DurableEventSinkError>;
}

#[async_trait]
pub trait TelemetryEventSink: Send + Sync {
    async fn emit(&self, event: AgentTelemetryEvent);
}
```

Use `async_trait` here because these ports are intentionally stored as `Arc<dyn ...>`; native async trait methods are not dyn-compatible.

In `error.rs`, define a `thiserror` error preserving `EventStreamBusError` as its source. In `scoped.rs`, map local loop events to existing external `AgentEvent`/`LlmEvent` envelopes. Add only the compatibility `AgentEvent` variants required for:

- `ToolExecutionStarted`
- `IterationCompleted`

Update `AgentEvent::event_type()` exhaustively. Do not add a generic conversion that can accidentally treat telemetry as durable.

For telemetry failure, log one structured warning at the adapter boundary. Do not log payload content that may contain secrets.

**Step 4: Run focused tests**

Run:

```bash
cargo fmt --all
cargo test -p stratum-core agent_loop_event
cargo test -p stratum-infra agent_event_sink
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-core/src/lib.rs crates/stratum-infra/src/agent_event_sink crates/stratum-infra/src/lib.rs
git commit -m "feat(infra): add scoped agent event sinks"
```

## Task 3: Teach the store consumer about iteration commits

**Files:**

- Modify: `crates/stratum-store/src/decorator.rs`
- Modify: `crates/stratum-store/tests/decorator.rs`

**Step 1: Write failing consumer tests**

Add tests proving:

- `AgentEvent::IterationCompleted` calls `AgentStore::complete_iteration` before forwarding.
- A `complete_iteration` store failure is returned as `EventStreamBusError::Persistence` and the inner bus receives nothing.
- `AgentEvent::ToolExecutionStarted` is forwarded to the retained bus and its publish acknowledgement is required.

Extend the existing `RecordingStore` with recorded iteration arguments instead of creating another full mock.

**Step 2: Run focused tests and verify failure**

Run:

```bash
cargo test -p stratum-store --test decorator iteration_completed -- --nocapture
```

Expected: FAIL because `StoreEventStreamBus` does not project the new event.

**Step 3: Implement the projection**

In `StoreEventStreamBus::publish`, match the new iteration event and call:

```rust
self.store
    .complete_iteration(envelope.run_id, *turn_id, *iteration, *usage)
    .await
    .map_err(EventStreamBusError::persistence)?;
```

Include `turn_id` in the external compatibility event if required by the store projection. Keep tool-execution-started as a retained durable record rather than adding session recovery behavior.

**Step 4: Run store tests**

Run:

```bash
cargo fmt --all
cargo test -p stratum-store --test decorator
cargo test -p stratum-store --test recovery_composition
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-store/src/decorator.rs crates/stratum-store/tests/decorator.rs
git commit -m "feat(store): persist loop iteration events"
```

## Task 4: Propagate cancellation into tools

**Files:**

- Modify: `crates/stratum-tools/Cargo.toml`
- Modify: `crates/stratum-tools/src/definition.rs`
- Modify: `crates/stratum-tools/src/error.rs`
- Modify: `crates/stratum-tools/src/builtin/mod.rs`
- Modify: `crates/stratum-tools/src/builtin/apply_patch.rs`
- Modify: `crates/stratum-tools/src/builtin/file_metadata.rs`
- Modify: `crates/stratum-tools/src/builtin/list_dir.rs`
- Modify: `crates/stratum-tools/src/builtin/read_file_lines.rs`
- Modify: `crates/stratum-tools/src/builtin/search_text.rs`
- Modify: `crates/stratum-agent/src/loop.rs`
- Modify: `crates/stratum-agent/tests/streaming_loop.rs`
- Test: `crates/stratum-tools/src/definition.rs`

**Step 1: Write a failing cancellation-aware tool test**

Add a test tool whose `call` asserts that the supplied token is already cancelled and returns a deterministic output. Exercise it through `BuiltinToolRegistry::call`.

The target signatures are:

```rust
async fn call(
    &self,
    input: ToolInput,
    cancellation: &CancellationToken,
) -> Result<ToolOutput, ToolError>;
```

Apply the same parameter to `ToolRegistry::call`.

**Step 2: Run the focused test and verify compilation failure**

Run:

```bash
cargo test -p stratum-tools cancellation_token_reaches_tool
```

Expected: FAIL to compile because the trait has no cancellation parameter.

**Step 3: Update traits, builtins, and legacy callers**

Add `tokio-util.workspace = true` to `stratum-tools`. Thread `&CancellationToken` through the registry and every builtin. Builtins that cannot cancel an already-issued filesystem operation must at least check `is_cancelled()` before starting and return a typed `ToolError::Cancelled`; add that variant in `crates/stratum-tools/src/error.rs`.

Update the legacy agent loop and test implementations to pass their active token. Do not change legacy resume semantics in this task.

**Step 4: Run affected tests**

Run:

```bash
cargo fmt --all
cargo test -p stratum-tools
cargo test -p stratum-agent --test streaming_loop
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-tools crates/stratum-agent/src/loop.rs crates/stratum-agent/tests/streaming_loop.rs
git commit -m "feat(tools): propagate cancellation tokens"
```

## Task 5: Define the loop context, outcome, limits, and errors

**Files:**

- Create: `crates/stratum-agent/src/agent_loop/mod.rs`
- Create: `crates/stratum-agent/src/agent_loop/types.rs`
- Create: `crates/stratum-agent/src/agent_loop/error.rs`
- Modify: `crates/stratum-agent/src/lib.rs`
- Test: `crates/stratum-agent/src/agent_loop/types.rs`

**Step 1: Write failing public-type tests**

Test that:

- `LoopContext::new(system_prompt)` starts with no messages.
- `with_messages` moves an existing transcript in without cloning at the call site.
- `LoopLimits::default()` uses the current 16-turn and 16-tool-call limits.
- `LoopOutcome` distinguishes terminal completion from cancellation/error through `Result`, not a string field.

Use these minimal types:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct LoopContext {
    pub system_prompt: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoopLimits {
    pub max_iterations: usize,
    pub max_tool_calls_per_iteration: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopOutcome {
    pub new_messages: Vec<ChatMessage>,
    pub finish_reason: FinishReason,
    pub usage: TokenUsage,
}
```

Tool specs come from the injected `ToolExecutor`/registry, not a second independently mutable vector in `LoopContext`.

**Step 2: Run and verify failure**

Run:

```bash
cargo test -p stratum-agent agent_loop::types -- --nocapture
```

Expected: FAIL because the module does not exist.

**Step 3: Implement types and typed errors**

Define `AgentLoopError` in its own file with `thiserror` variants for durability, LLM, invalid protocol, cancellation, and limits. Preserve underlying errors with `#[source]`/`#[from]`. Error messages must start lowercase and omit trailing periods.

Do not merge these errors into the existing session-oriented `AgentError` yet. Re-export the new public types from `stratum-agent/src/lib.rs`.

**Step 4: Run focused tests**

Run:

```bash
cargo fmt --all
cargo test -p stratum-agent agent_loop::types
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-agent/src/agent_loop crates/stratum-agent/src/lib.rs
git commit -m "feat(agent): define loop kernel types"
```

## Task 6: Implement the concrete tool executor

**Files:**

- Create: `crates/stratum-agent/src/tool_executor/mod.rs`
- Create: `crates/stratum-agent/src/tool_executor/definition.rs`
- Create: `crates/stratum-agent/src/tool_executor/approval.rs`
- Create: `crates/stratum-agent/src/tool_executor/error.rs`
- Modify: `crates/stratum-agent/src/lib.rs`
- Test: `crates/stratum-agent/src/tool_executor/definition.rs`

**Step 1: Write failing execution-order tests**

Use a recording durable sink, a static approval handler, and a counting tool registry. Add separate tests for:

- missing tool produces an error `ChatMessage::tool` without `ToolExecutionStarted`;
- approval rejection persists requested then resolved and does not call the tool;
- approval acceptance persists resolved then `ToolExecutionStarted` before calling the tool;
- durable failure at any pre-execution boundary prevents the call;
- tool failure becomes an error tool-result message rather than `AgentLoopError`;
- a cancelled token is passed to the tool and its returned outcome is still represented.

Define a real approval port because interactive approval and non-interactive policy are distinct production modes:

```rust
#[async_trait]
pub trait ToolApproval: Send + Sync {
    async fn request(
        &self,
        request: ToolApprovalRequest,
        cancellation: &CancellationToken,
    ) -> Result<ApprovalDecision, ToolApprovalError>;
}
```

Provide concrete `AllowAllToolApproval` and `DenyAllToolApproval` implementations in `approval.rs`; interactive approval remains a higher-layer implementation.

**Step 2: Run and verify failure**

Run:

```bash
cargo test -p stratum-agent tool_executor -- --nocapture
```

Expected: FAIL because the executor does not exist.

**Step 3: Implement minimal sequential execution**

`ToolExecutor` owns:

```rust
pub struct ToolExecutor {
    registry: Arc<dyn ToolRegistry>,
    approval: Arc<dyn ToolApproval>,
    durable_events: Arc<dyn DurableEventSink>,
}
```

Expose `specs()` by delegating to the registry and one `execute(&ToolCall, &CancellationToken)` method. Keep lookup/authorization/approval/start/call/result conversion in this order. Return a `ToolExecutionOutcome` containing the tool message and whether execution reached the external tool.

Use a stable JSON error payload such as:

```rust
json!({ "error": error.to_string() })
```

Do not log tool arguments or results.

**Step 4: Run focused tests**

Run:

```bash
cargo fmt --all
cargo test -p stratum-agent tool_executor
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-agent/src/tool_executor crates/stratum-agent/src/lib.rs
git commit -m "feat(agent): add durable tool executor"
```

## Task 7: Implement the no-tool streaming loop

**Files:**

- Create: `crates/stratum-agent/src/agent_loop/runner.rs`
- Create: `crates/stratum-agent/src/agent_loop/stream.rs`
- Modify: `crates/stratum-agent/src/agent_loop/mod.rs`
- Create: `crates/stratum-agent/tests/agent_loop_kernel.rs`

**Step 1: Write failing happy-path and ordering tests**

Use a scripted `LlmProvider`, recording durable sink, and recording telemetry sink. Test:

- `LoopStarted` is acknowledged before the first prompt is committed;
- prompts are durably acknowledged before `chat_stream` is invoked;
- the request contains system prompt, committed history, prompts, model ID, and tool specs;
- text/reasoning deltas go only to telemetry;
- only the final assistant message enters committed context and `new_messages`;
- assistant message is acknowledged before `IterationCompleted` and `LoopFinished`;
- terminal usage is accumulated in the outcome.

The constructor should use a builder because it has four dependencies plus limits:

```rust
let agent_loop = AgentLoop::builder()
    .llm_provider(provider)
    .tool_executor(tool_executor)
    .durable_events(durable)
    .telemetry(telemetry)
    .limits(LoopLimits::default())
    .build()?;
```

Mark every builder setter `#[must_use]`.

**Step 2: Run and verify failure**

Run:

```bash
cargo test -p stratum-agent --test agent_loop_kernel no_tool -- --nocapture
```

Expected: FAIL because `AgentLoop::run` is not implemented.

**Step 3: Implement one streamed assistant turn**

Implement loop-start commit, prompt validation, durable prompt append, request construction, stream consumption, final assistant assembly, durable final append, iteration completion, and loop finish. Reuse the existing pending tool-call assembly rules by extracting only genuinely shared helpers; do not make the new kernel call methods on legacy `Agent`.

Telemetry failures are already contained by `TelemetryEventSink::emit`. Durable failures return immediately and must not mutate the committed context.

**Step 4: Run focused tests**

Run:

```bash
cargo fmt --all
cargo test -p stratum-agent --test agent_loop_kernel no_tool
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-agent/src/agent_loop crates/stratum-agent/tests/agent_loop_kernel.rs
git commit -m "feat(agent): add streaming loop kernel"
```

## Task 8: Add sequential tool turns and truncation safety

**Files:**

- Modify: `crates/stratum-agent/src/agent_loop/runner.rs`
- Modify: `crates/stratum-agent/tests/agent_loop_kernel.rs`

**Step 1: Write failing tool-cycle tests**

Add tests for:

- assistant tool call -> assistant ack -> tool executor -> tool-result ack -> iteration ack -> next LLM request;
- multiple tool calls execute strictly in assistant order;
- a missing/invalid/failed tool result is committed and shown to the next LLM request;
- exceeding the tool-call limit stops before any tool execution;
- reaching the iteration limit stops before another LLM request;
- `FinishReason::Length` with tool calls executes no tools and commits one error tool result per call;
- tool-call presence drives tool processing even when a provider reports an unexpected non-length finish reason.

Assert the exact recorded operation order, not only final messages.

**Step 2: Run and verify failure**

Run:

```bash
cargo test -p stratum-agent --test agent_loop_kernel tool -- --nocapture
```

Expected: FAIL because the first implementation terminates after one assistant response.

**Step 3: Implement the minimal inner loop**

For each iteration:

1. stream and commit assistant;
2. reject oversized tool batches before execution;
3. for `Length`, synthesize error tool messages without calling `ToolExecutor`;
4. otherwise call `ToolExecutor` sequentially and commit each returned tool message before the next call;
5. commit `IterationCompleted`;
6. continue only when tool calls produced results.

Preallocate per-batch message vectors with `Vec::with_capacity(tool_calls.len())`.

**Step 4: Run focused tests**

Run:

```bash
cargo fmt --all
cargo test -p stratum-agent --test agent_loop_kernel tool
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-agent/src/agent_loop/runner.rs crates/stratum-agent/tests/agent_loop_kernel.rs
git commit -m "feat(agent): execute sequential tool turns"
```

## Task 9: Enforce fail-closed errors and cancellation

**Files:**

- Modify: `crates/stratum-agent/src/agent_loop/runner.rs`
- Modify: `crates/stratum-agent/src/agent_loop/stream.rs`
- Modify: `crates/stratum-agent/src/agent_loop/error.rs`
- Modify: `crates/stratum-agent/tests/agent_loop_kernel.rs`

**Step 1: Write failing boundary-failure tests**

Build a table-driven recording sink that fails on append number N. Cover failures while committing:

- initial prompt;
- assistant final;
- tool result;
- iteration completion;
- loop finish.

For each case assert that no later LLM/tool external action occurs. Also add tests for:

- cancellation before first LLM call;
- cancellation during stream drops partial assistant content;
- cancellation while awaiting approval;
- cancellation after tool start still commits the tool's returned result;
- LLM stream failure attempts one durable `LoopFailed`;
- failure to persist `LoopFailed` returns durability as the primary error without a recursive write attempt.

**Step 2: Run and verify failure**

Run:

```bash
cargo test -p stratum-agent --test agent_loop_kernel fail_closed -- --nocapture
cargo test -p stratum-agent --test agent_loop_kernel cancellation -- --nocapture
```

Expected: FAIL until the runner checks cancellation and centralizes terminal error handling.

**Step 3: Implement cancellation and terminal error handling**

Use `tokio::select!` only around cancellation-safe LLM stream acquisition/next-event operations. Do not race and drop an already-started tool future; pass the token into the tool and await its reported outcome.

Centralize terminal recording in a helper that:

- records normal LLM/protocol/limit failures once;
- records cancellation once;
- never attempts another durable event after `DurableEventSink::append` fails;
- preserves the original error as context when terminal recording itself fails.

No mutex or rwlock guard may cross an `.await`.

**Step 4: Run all kernel tests**

Run:

```bash
cargo fmt --all
cargo test -p stratum-agent --test agent_loop_kernel
```

Expected: PASS.

**Step 5: Commit**

```bash
git add crates/stratum-agent/src/agent_loop crates/stratum-agent/tests/agent_loop_kernel.rs
git commit -m "feat(agent): enforce fail-closed loop boundaries"
```

## Task 10: Verify compatibility and archive crate conventions

**Files:**

- Modify: `crates/stratum-agent/AGENTS.md`
- Modify: `crates/stratum-store/AGENTS.md`
- Modify: `crates/stratum-tools/AGENTS.md`
- Modify if required by compiler only: existing crate tests and imports

**Step 1: Run the complete verification suite**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: formatting succeeds, all non-ignored tests pass, and clippy reports no warnings.

**Step 2: Fix only compatibility regressions**

Make minimal changes required by the compiler or existing tests. Do not migrate the API host to the new loop, delete resume behavior, or expand scope.

Re-run the exact failing command after each correction.

**Step 3: Update crate `AGENTS.md` files**

Archive these confirmed conventions:

- `stratum-agent`: the new kernel consumes preloaded context, uses separate durable/telemetry ports, commits before mutating context, and treats tools sequentially.
- `stratum-store`: durable loop events are projected before acknowledgement; committed-event forwarding failure does not undo store success.
- `stratum-tools`: cancellation token is propagated through registry and tool calls; tools check it before starting external work.
- Explicitly state that legacy session/resume integration remains temporary and is not the ownership model for the new kernel.

**Step 4: Re-run final verification and inspect the diff**

Run:

```bash
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
git diff --check
git status --short
```

Expected: all commands pass; only planned files are modified.

**Step 5: Commit**

```bash
git add crates/stratum-agent/AGENTS.md crates/stratum-store/AGENTS.md crates/stratum-tools/AGENTS.md
git add -u
git commit -m "docs: archive agent loop conventions"
```

## Completion checklist

- `AgentLoop` can be instantiated and tested without an `AgentStore`, session, or `EventStreamBus`.
- All prompts, complete assistant messages, tool results, iteration boundaries, and terminal states are fail-closed durable events.
- Telemetry failures never change control flow.
- Approval and `ToolExecutionStarted` are durable before real tool execution.
- A started tool whose result is not committed is observable as an unknown outcome and is never automatically retried by the kernel.
- Tool calls are sequential and cancellation-aware.
- Partial or length-truncated tool calls are never executed.
- The legacy `Agent` and workspace tests remain operational.
- Relevant crate `AGENTS.md` files contain the final conventions.
