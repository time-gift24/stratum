status: DONE

files changed:
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/Cargo.lock
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/crates/wyse-agent/Cargo.toml
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/crates/wyse-agent/src/definition.rs
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/crates/wyse-agent/src/loop.rs
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/crates/wyse-agent/tests/streaming_loop.rs

commits:
- eb18781 feat: implement streaming agent loop

tests run with results:
- `cargo test -p wyse-agent stream_runs_tool_and_continues_with_tool_result` -> FAIL before implementation (`timed out waiting for streamed agent events`)
- `cargo test -p wyse-agent stream_runs_tool_and_continues_with_tool_result` -> PASS after implementation
- `cargo test -p wyse-agent --all-targets` -> FAIL once due test harness timing in `stream_rejects_second_run_while_background_loop_is_active`
- `cargo test -p wyse-agent --all-targets` -> PASS after fixing the timing harness
- `cargo fmt` -> PASS
- `cargo clippy --workspace --all-targets` -> PASS
- `cargo test -p wyse-agent --all-targets` -> PASS (3 unit tests, 2 integration tests)

self-review notes:
- `Agent::stream` now clones history before spawning the internal loop and only writes history back on loop success, so the history mutex is not held across await points.
- Active-run protection now lasts for the spawned loop lifetime instead of the returned stream lifetime, and coverage includes rejecting a second run while the first loop is still active.
- The internal loop stays `pub(crate)`, publishes `RuntimeEvent::Agent` envelopes with the required metadata, accumulates usage, and executes tool calls sequentially.
- Tool execution failures publish `LlmEvent::ToolCallFailed` and append model-visible tool messages instead of terminating the run with `AgentError`.

concerns:
- none

---

fix review findings:
- raced `LlmProvider::chat_stream(request)` against the run cancellation token so a hung stream start now publishes `AgentEvent::Cancelled` and exits promptly
- raced `ToolRegistry::call(...)` against the run cancellation token so a hung tool call now publishes `AgentEvent::Cancelled` and exits promptly
- added streaming coverage for provider-start cancellation, tool-call cancellation, and non-terminal tool failure follow-up messaging

files changed:
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/crates/wyse-agent/src/loop.rs
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/crates/wyse-agent/tests/streaming_loop.rs

tests run with results:
- `cargo test -p wyse-agent --test streaming_loop stream_publishes_cancelled_when_provider_stream_creation_hangs` -> FAIL before loop fix (`timed out waiting for cancelled event`)
- `cargo test -p wyse-agent --test streaming_loop stream_publishes_cancelled_when_tool_call_hangs` -> FAIL before loop fix (`timed out waiting for cancelled event`)
- `cargo test -p wyse-agent --test streaming_loop stream_publishes_tool_failure_and_retries_with_tool_error_message` -> PASS
- `cargo test -p wyse-agent --test streaming_loop` -> PASS (5 tests)
- `cargo fmt` -> PASS
- `cargo test -p wyse-agent --all-targets` -> PASS (3 unit tests, 5 integration tests)
- `cargo clippy -p wyse-agent --all-targets` -> PASS

output summary:
- both cancellation regressions reproduced before the code change and emitted the expected timeout symptom
- after the fix, the streaming loop test suite passed with 5/5 tests
- full crate verification passed with 8 total tests and clean clippy output

concerns:
- reminder: before merging the PR, archive the final crate-level design/implementation notes in the relevant `AGENTS.md`
