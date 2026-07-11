# Stratum REPL Echo Approval-Flow Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let `stratum-repl` register one Echo tool and let its terminal operator approve or reject every Echo call.

**Architecture:** `build_default_agent` receives a caller-owned tool registry rather than constructing an empty one. The REPL composes a `RequireApproval` builtin registry containing only Echo, then resolves `ToolApprovalRequested` inline while consuming the same event stream for the active turn.

**Tech Stack:** Rust 2024/MSRV 1.88, Tokio, `wyse-agent`, `wyse-tools`, `StoreEventStreamBus`, mock LLM streams, temporary `LocalFilesystem` stores.

## Global Constraints

- Keep existing `wyse-*` package names unchanged; `stratum` remains user-facing only.
- Register exactly one `EchoTool` in `stratum-repl`; do not register filesystem tools.
- Use `BuiltinToolRegistry::new(ToolPermissionMode::RequireApproval)`; every Echo call requires a human decision.
- Preserve the default builder as injection-only: no configuration, filesystem composition, or hard-coded tool selection in library code.
- Subscribe with `ReplayStart::New` before a normal run or crash resume, and render events only from that subscription.
- Default output is assistant text plus operational prompts/diagnostics; `--debug` emits every received `StreamEnvelope` as NDJSON.
- Approval input is exactly `approve` or `reject`; invalid input repeats the prompt and EOF means reject.
- Rejection must use `Agent::resolve_tool_approval` so the runtime returns its structured `approval_rejected` result; do not emulate tool results in the REPL.
- Production Rust uses typed errors, no `unwrap()`, and public fallible APIs document `# Errors`.
- Do not modify the user's ignored `.stratum/` session data.

---

## File Structure

- `crates/wyse-agent-builtin/src/default_agent.rs` — make the registry an explicit dependency of the default-agent constructor.
- `crates/wyse-agent-builtin/src/lib.rs` — no API export change beyond the existing re-exported function signature.
- `crates/wyse-agent-builtin/src/bin/stratum_repl.rs` — register Echo, prompt/resolve approvals inside turn event consumption, and add mock-provider tests.
- `crates/wyse-agent-builtin/AGENTS.md` — archive the Echo-only approval contract.

### Task 1: Inject the tool registry and compose approval-only Echo

**Files:**
- Modify: `crates/wyse-agent-builtin/src/default_agent.rs:1-48`
- Modify: `crates/wyse-agent-builtin/src/bin/stratum_repl.rs:1-180, 320-385`

**Interfaces:**
- Produces: `build_default_agent(agent_id: AgentId, store: Arc<dyn AgentStore>, event_bus: Arc<dyn EventStreamBus>, llm_provider: Arc<dyn LlmProvider>, tool_registry: Arc<dyn ToolRegistry>) -> Result<Agent, AgentError>`.
- Produces: `fn approval_registry() -> Result<Arc<dyn ToolRegistry>, ReplError>`.
- Consumes: `BuiltinToolRegistry::new(ToolPermissionMode::RequireApproval)`, `EchoTool::new()`, and `ToolRegistry::register`.

- [ ] **Step 1: Write failing constructor/composition tests**

  Update the `DefaultAgentBuilder` function-pointer test in `default_agent.rs` to require the fifth `Arc<dyn ToolRegistry>` parameter. In the binary test module, add a test that calls `approval_registry()`, obtains `specs()`, and asserts the sole spec name is `"echo"`; assert `authorization(&ToolName::from("echo"))` returns `Some((ToolKind::Read, DangerLevel::Low))`.

- [ ] **Step 2: Run the tests and verify failure**

  Run:

  ```bash
  cargo test -p wyse-agent-builtin --bin stratum_repl approval_registry -- --nocapture
  ```

  Expected: compilation failure because `approval_registry` does not exist and the builder still has four inputs.

- [ ] **Step 3: Implement the explicit registry boundary**

  Change the default builder to accept and forward an injected registry:

  ```rust
  pub fn build_default_agent(
      agent_id: AgentId,
      store: Arc<dyn AgentStore>,
      event_bus: Arc<dyn EventStreamBus>,
      llm_provider: Arc<dyn LlmProvider>,
      tool_registry: Arc<dyn ToolRegistry>,
  ) -> Result<Agent, AgentError> {
      Agent::builder()
          .id(agent_id)
          .name("default-agent")
          .system_prompt(DEFAULT_SYSTEM_PROMPT)
          .llm_provider(llm_provider)
          .tool_registry(tool_registry)
          .event_bus(event_bus)
          .store(store)
          .build()
  }
  ```

  In `stratum_repl.rs`, import `BuiltinToolRegistry`, `EchoTool`, `ToolRegistry`, `ToolPermissionMode`, `ToolKind`, `DangerLevel`, and `ToolError`. Add `ReplError::Tool(#[from] ToolError)`. Implement:

  ```rust
  fn approval_registry() -> Result<Arc<dyn ToolRegistry>, ReplError> {
      let mut registry = BuiltinToolRegistry::new(ToolPermissionMode::RequireApproval);
      registry.register(Arc::new(EchoTool::new()), ToolKind::Read, DangerLevel::Low)?;
      Ok(Arc::new(registry))
  }
  ```

  Pass `approval_registry()?` at both production and test-session calls to `build_default_agent`. Do not alter provider, filesystem, or store composition.

- [ ] **Step 4: Run focused tests and the builtin crate suite**

  Run:

  ```bash
  cargo fmt --check
  cargo test -p wyse-agent-builtin --bin stratum_repl
  ```

  Expected: all existing REPL tests and the new registry test pass without provider network access.

- [ ] **Step 5: Commit the registry injection task**

  ```bash
  git add crates/wyse-agent-builtin/src/default_agent.rs crates/wyse-agent-builtin/src/bin/stratum_repl.rs
  git commit -m "feat(builtin): register echo approval tool in repl"
  ```

### Task 2: Resolve approval events in the REPL turn consumer

**Files:**
- Modify: `crates/wyse-agent-builtin/src/bin/stratum_repl.rs:107-262, 390-650`
- Modify: `crates/wyse-agent-builtin/AGENTS.md:1-11`

**Interfaces:**
- Produces: `async fn resolve_approval<R: BufRead, W: Write>(agent: &Agent, approval_id: ApprovalId, input: &mut R, output: &mut W) -> Result<(), ReplError>`.
- Produces: `async fn consume_turn_events<R: BufRead, W: Write>(session: &Session, events: &mut EventStream, input: &mut R, debug: bool, output: &mut W) -> Result<(), ReplError>`.
- Changes: `drive_turn` and `restore_session` accept the caller's `&mut impl BufRead` and forward it to `consume_turn_events`.

- [ ] **Step 1: Write failing approve, reject, and EOF tests**

  Add an `approval_provider()` helper in the binary test module with two queued streams: first emits a `ToolCallDelta` with `call_id = "echo-1"`, `name = "echo"`, `arguments_delta = r#"{"message":"hello"}"#`, then finishes with `FinishReason::ToolCalls`; second emits text `"done"` and `FinishReason::Stop`.

  Add three tests using a `Cursor<Vec<u8>>` as REPL input:

  ```rust
  drive_turn(&session, "use echo", &mut Cursor::new(b"approve\n"), true, &mut output).await?;
  drive_turn(&session, "use echo", &mut Cursor::new(b"reject\n"), false, &mut output).await?;
  drive_turn(&session, "use echo", &mut Cursor::new(Vec::<u8>::new()), false, &mut output).await?;
  ```

  For approve, assert output contains the approval prompt and `done`, debug envelopes contain `ToolApprovalRequested` then `ToolApprovalResolved { decision: Approve }`, and persisted message 3 is a tool message whose JSON content is `{ "message": "hello" }`. For reject and EOF, assert persisted message 3 is the exact runtime payload:

  ```json
  {"error":{"type":"approval_rejected","message":"user rejected tool call"}}
  ```

  and message 4 is assistant `done`.

- [ ] **Step 2: Run tests and verify failure**

  Run:

  ```bash
  cargo test -p wyse-agent-builtin --bin stratum_repl approval -- --nocapture
  ```

  Expected: compilation failure because `drive_turn` does not accept approval input and approval events are ignored.

- [ ] **Step 3: Implement inline terminal resolution**

  Change `main` to pass its locked stdin to `restore_session` and each `drive_turn`. When `consume_turn_events` receives `AgentEvent::ToolApprovalRequested`, serialize the arguments to the output and call `resolve_approval`.

  `resolve_approval` must print a prompt containing the approval ID, tool name, kind, danger level, and arguments. It repeatedly calls `input.read_line(&mut line)` and maps trimmed exact input as follows:

  ```rust
  let decision = match line.trim() {
      "approve" => ApprovalDecision::Approve,
      "reject" => ApprovalDecision::Reject,
      _ if bytes_read == 0 => ApprovalDecision::Reject,
      _ => { writeln!(output, "enter approve or reject")?; continue; }
  };
  agent.resolve_tool_approval(approval_id, decision).await?;
  return Ok(());
  ```

  Preserve existing debug serialization before event matching. Treat `ToolApprovalResolved` as a normal forwarded event. Keep `Finished`, `Failed`, `Cancelled`, and closed-stream behavior unchanged. Do not fabricate a tool result or execute Echo from the REPL.

  Update `AGENTS.md` to state that `stratum_repl` registers only `EchoTool` in `RequireApproval` mode for approval-flow validation and that it prompts for `approve` or `reject` for every call.

- [ ] **Step 4: Run focused and workspace verification**

  Run:

  ```bash
  cargo fmt --check
  cargo test -p wyse-agent-builtin --bin stratum_repl approval -- --nocapture
  cargo test --workspace --all-targets
  cargo clippy --workspace --all-targets
  ```

  Expected: approve, reject, and EOF paths pass; all workspace tests and Clippy exit 0. Existing external NATS/provider/Docker tests remain ignored by their established annotations.

- [ ] **Step 5: Commit the interactive approval flow**

  ```bash
  git add crates/wyse-agent-builtin/src/bin/stratum_repl.rs crates/wyse-agent-builtin/AGENTS.md
  git commit -m "feat(repl): resolve echo tool approvals interactively"
  ```
