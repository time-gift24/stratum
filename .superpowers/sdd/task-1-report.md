status: DONE
files changed:
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/crates/wyse-core/src/lib.rs
commits:
- pending
tests run:
- `cargo test -p wyse-core --all-targets` — passed
self-review notes:
- Replaced string-backed `AgentId` with a UUIDv7-backed newtype and kept the existing `RunId` style consistent.
- Added shared `ToolCall`, `ToolCallDelta`, `ChatRole`, `ChatContent`, `ChatMessage`, and `AgentEvent` definitions in core.
- Added `RuntimeEvent::Agent` plus `event_type()` support and the requested core tests.
concerns:
- The brief's sample command `cargo test -p wyse-core agent_id_new_uses_uuid_v7 chat_message_user_constructor_sets_role_and_text tool_message_records_answered_call_id runtime_agent_event_type_is_agent` is not valid Cargo syntax; I used the crate-wide `--all-targets` test run instead.
- Unrelated workspace edits already existed in `TODO.md` and `docs/superpowers/`; I left them untouched.

---
Task 1 follow-up:
status: DONE
commit: 75a87a6
files changed:
- /Users/wanyaozhong/projects/wyse-agent-os/.worktrees/wyse-agent-design/crates/wyse-core/src/lib.rs
tests run:
- `cargo test -p wyse-core --all-targets`
test summary:
- 18 tests
- 18 passed
- 0 failed
- 0 ignored
- 0 filtered
fix:
- Added `ChatMessage::with_tool_calls(mut self, tool_calls: Vec<ToolCall>) -> Self` as a documented public builder method adjacent to `with_reasoning_content`.
- Added unit test `chat_message_with_tool_calls_sets_tool_calls` that verifies tool calls are assigned from the provided vector.
concerns:
- none
