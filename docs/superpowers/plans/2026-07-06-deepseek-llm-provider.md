# DeepSeek LLM Provider Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a minimal DeepSeek provider to `wyse-llm`, with assistant reasoning support and shared SSE framing.

**Architecture:** Keep DeepSeek as its own provider mapping because it owns `thinking`, `reasoning_effort`, and `reasoning_content` semantics. Move the existing SSE parser out of `openai_compatible` into `protocol::sse`, then reuse it from both providers. Do not add default base URL, price/cost metadata, old-model rejection, registry, factory, or request-level provider options.

**Tech Stack:** Rust 2024, Tokio, reqwest, serde/serde_json, futures-core/futures-util, thiserror, existing `wyse-core` IDs.

---

## File Structure

- Modify `crates/wyse-llm/src/message.rs`: add assistant `reasoning_content` to `ChatMessage`.
- Modify `crates/wyse-llm/src/definition.rs`: add `ChatStreamEvent::ReasoningDelta`.
- Create `crates/wyse-llm/src/protocol/sse.rs`: shared `SseParser`, `SseEvent`, and `stream_eof_error`.
- Modify `crates/wyse-llm/src/protocol/mod.rs`: export `sse` and `deepseek`.
- Modify `crates/wyse-llm/src/protocol/openai_compatible.rs`: delete local SSE parser and import shared parser.
- Create `crates/wyse-llm/src/protocol/deepseek.rs`: DeepSeek provider, request mapping, response mapping, stream mapping.
- Modify `crates/wyse-llm/src/lib.rs`: re-export DeepSeek public types.
- Modify `crates/wyse-llm/tests/openai_compatible_provider.rs`: keep existing stream coverage passing after SSE extraction.
- Create `crates/wyse-llm/tests/deepseek_provider.rs`: local HTTP tests for thinking, reasoning, and stream mapping.
- Modify `crates/wyse-llm/tests/openai_compatible_smoke.rs` or create `crates/wyse-llm/tests/deepseek_smoke.rs`: ignored network smoke test.
- Modify `crates/wyse-llm/AGENTS.md`: archive DeepSeek provider rules.

## Task 1: Add Reasoning To Public Chat Types

**Files:**
- Modify: `crates/wyse-llm/src/message.rs`
- Modify: `crates/wyse-llm/src/definition.rs`

- [ ] **Step 1: Write the failing tests**

Add these tests at the end of the existing `#[cfg(test)] mod tests` in `crates/wyse-llm/src/message.rs`:

```rust
#[test]
fn assistant_message_can_carry_reasoning_content() {
    let message = ChatMessage::assistant("answer").with_reasoning_content("thinking");

    assert_eq!(message.reasoning_content.as_deref(), Some("thinking"));
}

#[test]
fn reasoning_content_is_skipped_when_absent() {
    let value = serde_json::to_value(ChatMessage::assistant("answer"))
        .expect("message should serialize");

    assert!(value.get("reasoning_content").is_none());
}
```

Add this assertion to `crates/wyse-llm/src/definition.rs` tests:

```rust
#[test]
fn stream_event_can_represent_reasoning_delta() {
    let event = crate::ChatStreamEvent::ReasoningDelta {
        delta: "thinking".to_owned(),
    };

    assert_eq!(
        serde_json::to_value(event).expect("event should serialize"),
        serde_json::json!({
            "type": "reasoning_delta",
            "data": { "delta": "thinking" }
        })
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p wyse-llm assistant_message_can_carry_reasoning_content stream_event_can_represent_reasoning_delta
```

Expected: FAIL because `reasoning_content`, `with_reasoning_content`, and `ReasoningDelta` do not exist.

- [ ] **Step 3: Implement minimal public type changes**

In `ChatMessage`, add:

```rust
/// Reasoning content produced by an assistant message.
#[serde(default, skip_serializing_if = "Option::is_none")]
pub reasoning_content: Option<String>,
```

Update `ChatMessage::text`:

```rust
Self {
    role,
    content: ChatContent::Text(content.into()),
    tool_calls: Vec::new(),
    tool_call_id: None,
    reasoning_content: None,
}
```

Add builder method:

```rust
/// Sets assistant reasoning content.
#[must_use]
pub fn with_reasoning_content(mut self, content: impl Into<String>) -> Self {
    self.reasoning_content = Some(content.into());
    self
}
```

In `ChatStreamEvent`, add this variant before `ToolCallDelta`:

```rust
/// Reasoning text emitted by the model.
ReasoningDelta {
    /// Reasoning text fragment.
    delta: String,
},
```

- [ ] **Step 4: Run tests**

Run:

```bash
cargo test -p wyse-llm assistant_message_can_carry_reasoning_content stream_event_can_represent_reasoning_delta
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wyse-llm/src/message.rs crates/wyse-llm/src/definition.rs
git commit -m "feat: add llm reasoning message types"
```

## Task 2: Extract Shared SSE Parser

**Files:**
- Create: `crates/wyse-llm/src/protocol/sse.rs`
- Modify: `crates/wyse-llm/src/protocol/mod.rs`
- Modify: `crates/wyse-llm/src/protocol/openai_compatible.rs`
- Test: existing `crates/wyse-llm/tests/openai_compatible_provider.rs`

- [ ] **Step 1: Move parser code into a shared module**

Create `crates/wyse-llm/src/protocol/sse.rs` with the existing parser logic from `openai_compatible.rs`, made crate-visible:

```rust
//! Server-sent event framing shared by provider protocol adapters.

use std::io;

use crate::LlmError;

#[derive(Debug, Default)]
pub(crate) struct SseParser {
    buffer: Vec<u8>,
}

impl SseParser {
    pub(crate) fn push(&mut self, chunk: &[u8]) -> Vec<Result<SseEvent, LlmError>> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some((event_end, delimiter_len)) = event_delimiter(&self.buffer) {
            let event = self.buffer[..event_end].to_vec();
            self.buffer.drain(..event_end + delimiter_len);

            match parse_sse_event(event) {
                Ok(Some(event)) => events.push(Ok(event)),
                Ok(None) => {}
                Err(error) => {
                    events.push(Err(error));
                    break;
                }
            }
        }

        events
    }

    pub(crate) fn has_pending(&self) -> bool {
        !self.buffer.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SseEvent {
    Data(String),
    Done,
}

pub(crate) fn stream_eof_error(message: &'static str) -> LlmError {
    LlmError::stream(io::Error::new(io::ErrorKind::UnexpectedEof, message))
}

fn event_delimiter(buffer: &[u8]) -> Option<(usize, usize)> {
    let lf = buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|position| (position, 2));
    let crlf = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| (position, 4));

    match (lf, crlf) {
        (Some(lf), Some(crlf)) => Some(lf.min(crlf)),
        (Some(lf), None) => Some(lf),
        (None, Some(crlf)) => Some(crlf),
        (None, None) => None,
    }
}

fn parse_sse_event(event: Vec<u8>) -> Result<Option<SseEvent>, LlmError> {
    let text = String::from_utf8(event).map_err(LlmError::stream)?;
    let mut data_lines = Vec::new();

    for line in text.lines() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() || line.starts_with(':') {
            continue;
        }

        if let Some(data) = line.strip_prefix("data:") {
            data_lines.push(data.strip_prefix(' ').unwrap_or(data).to_owned());
        }
    }

    if data_lines.is_empty() {
        return Ok(None);
    }

    let data = data_lines.join("\n");
    if data == "[DONE]" {
        return Ok(Some(SseEvent::Done));
    }

    Ok(Some(SseEvent::Data(data)))
}
```

Add to `crates/wyse-llm/src/protocol/mod.rs`:

```rust
pub(crate) mod sse;
```

- [ ] **Step 2: Rewire OpenAI-compatible to use shared SSE**

In `openai_compatible.rs`, replace local imports:

```rust
use std::{collections::VecDeque, pin::Pin};
```

Add:

```rust
use crate::protocol::sse::{SseEvent, SseParser, stream_eof_error};
```

Delete local `SseParser`, `SseEvent`, `event_delimiter`, `parse_sse_event`, and `stream_eof_error` from `openai_compatible.rs`.

- [ ] **Step 3: Run OpenAI stream regression tests**

Run:

```bash
cargo test -p wyse-llm --test openai_compatible_provider chat_stream
```

Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add crates/wyse-llm/src/protocol/sse.rs crates/wyse-llm/src/protocol/mod.rs crates/wyse-llm/src/protocol/openai_compatible.rs
git commit -m "refactor: share llm sse parser"
```

## Task 3: Add DeepSeek Request Mapping

**Files:**
- Create: `crates/wyse-llm/src/protocol/deepseek.rs`
- Modify: `crates/wyse-llm/src/protocol/mod.rs`
- Modify: `crates/wyse-llm/src/lib.rs`
- Test: `crates/wyse-llm/tests/deepseek_provider.rs`

- [ ] **Step 1: Write failing request mapping test**

Create `crates/wyse-llm/tests/deepseek_provider.rs` by copying the `TestServer` helper from `openai_compatible_provider.rs`. Add this test:

```rust
use serde_json::{Value, json};
use wyse_llm::{
    ApiKey, ChatMessage, ChatRequest, DeepSeekModel, DeepSeekProvider, DeepSeekReasoningEffort,
    DeepSeekThinking, LlmProvider,
};

#[tokio::test]
async fn chat_posts_thinking_and_reasoning_content() {
    let server = TestServer::spawn(TestResponse::ok(json!({
        "choices": [{
            "message": {"role": "assistant", "content": "done"},
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    })));
    let provider = DeepSeekProvider::new(
        server.base_url("v1"),
        ApiKey::new("sk-test"),
        DeepSeekModel::V4Pro,
        DeepSeekThinking::Enabled {
            effort: Some(DeepSeekReasoningEffort::Max),
        },
    );

    let model = DeepSeekModel::V4Pro.model_id();
    provider
        .chat(
            ChatRequest::new(model)
                .with_message(ChatMessage::user("solve"))
                .with_message(ChatMessage::assistant("tool answer").with_reasoning_content("why")),
        )
        .await
        .expect("chat should succeed");

    let request = server.request();
    let body: Value = serde_json::from_slice(&request.body).expect("request body should be json");

    assert_eq!(request.path, "/v1/chat/completions");
    assert_eq!(body["model"], "deepseek-v4-pro");
    assert_eq!(body["thinking"], json!({"type": "enabled"}));
    assert_eq!(body["reasoning_effort"], "max");
    assert_eq!(body["messages"][1]["reasoning_content"], "why");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p wyse-llm --test deepseek_provider chat_posts_thinking_and_reasoning_content
```

Expected: FAIL because DeepSeek types do not exist.

- [ ] **Step 3: Implement minimal DeepSeek provider shell and request mapping**

Add `pub mod deepseek;` to `crates/wyse-llm/src/protocol/mod.rs`.

Add re-exports to `crates/wyse-llm/src/lib.rs`:

```rust
pub use protocol::deepseek::{
    DeepSeekModel, DeepSeekProvider, DeepSeekReasoningEffort, DeepSeekThinking,
};
```

In `crates/wyse-llm/src/protocol/deepseek.rs`, implement provider shape, URL/header helpers, model enum, thinking enum, `chat`, `chat_stream` placeholder returning `UnsupportedCapability` for now, and `to_chat_payload`. Reuse the OpenAI-compatible message/tool/structured mapping by making those helper functions `pub(crate)` in `openai_compatible.rs` if needed:

```rust
#[derive(Debug, Clone)]
pub struct DeepSeekProvider {
    client: reqwest::Client,
    base_url: String,
    api_key: ApiKey,
    model: DeepSeekModel,
    thinking: DeepSeekThinking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeepSeekModel {
    V4Flash,
    V4Pro,
}

impl DeepSeekModel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V4Flash => "deepseek-v4-flash",
            Self::V4Pro => "deepseek-v4-pro",
        }
    }

    #[must_use]
    pub fn model_id(self) -> ModelId {
        ModelId::from(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeepSeekThinking {
    Enabled { effort: Option<DeepSeekReasoningEffort> },
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeepSeekReasoningEffort {
    High,
    Max,
}
```

In `chat`, enforce:

```rust
if request.model != self.model.model_id() {
    return Err(LlmError::InvalidRequest(
        "request model does not match provider model",
    ));
}
```

Add `thinking` mapping:

```rust
match self.thinking {
    DeepSeekThinking::Enabled { effort } => {
        payload["thinking"] = json!({"type": "enabled"});
        if let Some(effort) = effort {
            payload["reasoning_effort"] = Value::String(match effort {
                DeepSeekReasoningEffort::High => "high",
                DeepSeekReasoningEffort::Max => "max",
            }.to_owned());
        }
    }
    DeepSeekThinking::Disabled => {
        payload["thinking"] = json!({"type": "disabled"});
    }
}
```

- [ ] **Step 4: Run test**

Run:

```bash
cargo test -p wyse-llm --test deepseek_provider chat_posts_thinking_and_reasoning_content
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wyse-llm/src/protocol/deepseek.rs crates/wyse-llm/src/protocol/mod.rs crates/wyse-llm/src/lib.rs crates/wyse-llm/tests/deepseek_provider.rs
git commit -m "feat: add deepseek chat request mapping"
```

## Task 4: Map DeepSeek Reasoning Responses

**Files:**
- Modify: `crates/wyse-llm/src/protocol/deepseek.rs`
- Test: `crates/wyse-llm/tests/deepseek_provider.rs`

- [ ] **Step 1: Write failing non-streaming response test**

Add:

```rust
#[tokio::test]
async fn chat_maps_reasoning_content_to_assistant_message() {
    let server = TestServer::spawn(TestResponse::ok(json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "reasoning_content": "first think",
                "content": "final answer"
            },
            "finish_reason": "stop"
        }],
        "usage": {"prompt_tokens": 2, "completion_tokens": 3, "total_tokens": 5}
    })));
    let provider = DeepSeekProvider::new(
        server.base_url("v1"),
        ApiKey::new("sk-test"),
        DeepSeekModel::V4Flash,
        DeepSeekThinking::Disabled,
    );

    let response = provider
        .chat(ChatRequest::new(DeepSeekModel::V4Flash.model_id()))
        .await
        .expect("chat should succeed");

    assert_eq!(response.message, ChatMessage::assistant("final answer").with_reasoning_content("first think"));
    assert_eq!(response.usage.expect("usage").total_tokens, 5);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p wyse-llm --test deepseek_provider chat_maps_reasoning_content_to_assistant_message
```

Expected: FAIL because DeepSeek response mapping does not read `reasoning_content`.

- [ ] **Step 3: Implement response mapping**

In `deepseek.rs`, implement `chat_response_from_value` like OpenAI-compatible mapping, plus:

```rust
let mut chat_message = ChatMessage::assistant(content);
if let Some(reasoning) = message["reasoning_content"].as_str()
    && !reasoning.is_empty()
{
    chat_message = chat_message.with_reasoning_content(reasoning);
}
chat_message.tool_calls = tool_calls_from_message(message)?;
```

If OpenAI-compatible helper functions are private, make only these helpers `pub(crate)` and reuse them:

```rust
pub(crate) fn tool_calls_from_message(message: &Value) -> Result<Vec<ToolCall>, LlmError>
pub(crate) fn finish_reason(reason: Option<&str>) -> FinishReason
pub(crate) fn usage_from_value(value: Option<&Value>) -> Option<TokenUsage>
```

- [ ] **Step 4: Run test**

Run:

```bash
cargo test -p wyse-llm --test deepseek_provider chat_maps_reasoning_content_to_assistant_message
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wyse-llm/src/protocol/deepseek.rs crates/wyse-llm/src/protocol/openai_compatible.rs crates/wyse-llm/tests/deepseek_provider.rs
git commit -m "feat: map deepseek reasoning responses"
```

## Task 5: Map DeepSeek Streaming Reasoning

**Files:**
- Modify: `crates/wyse-llm/src/protocol/deepseek.rs`
- Test: `crates/wyse-llm/tests/deepseek_provider.rs`

- [ ] **Step 1: Write failing stream test**

Add:

```rust
use futures_util::StreamExt;
use wyse_llm::{ChatStreamEvent, FinishReason};

#[tokio::test]
async fn chat_stream_maps_reasoning_and_text_delta() {
    let server = TestServer::spawn(TestResponse::stream(
        "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"think\"}}]}\n\n\
         data: {\"choices\":[{\"delta\":{\"content\":\"answer\"}}]}\n\n\
         data: {\"choices\":[{\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\n",
    ));
    let provider = DeepSeekProvider::new(
        server.base_url("v1"),
        ApiKey::new("sk-test"),
        DeepSeekModel::V4Pro,
        DeepSeekThinking::Enabled {
            effort: Some(DeepSeekReasoningEffort::High),
        },
    );

    let mut stream = provider
        .chat_stream(ChatRequest::new(DeepSeekModel::V4Pro.model_id()))
        .await
        .expect("stream should open");

    assert_eq!(
        stream.next().await.expect("event").expect("reasoning maps"),
        ChatStreamEvent::ReasoningDelta {
            delta: "think".to_owned()
        }
    );
    assert_eq!(
        stream.next().await.expect("event").expect("text maps"),
        ChatStreamEvent::TextDelta {
            delta: "answer".to_owned()
        }
    );
    assert_eq!(
        stream.next().await.expect("event").expect("finish maps"),
        ChatStreamEvent::Finished {
            finish_reason: FinishReason::Stop,
            usage: Some(wyse_core::TokenUsage {
                input_tokens: 2,
                output_tokens: 3,
                total_tokens: 5,
            })
        }
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cargo test -p wyse-llm --test deepseek_provider chat_stream_maps_reasoning_and_text_delta
```

Expected: FAIL because `chat_stream` is not implemented or does not map reasoning deltas.

- [ ] **Step 3: Implement stream mapping**

In `deepseek.rs`, implement `chat_stream` using the same `VecDeque`/`stream::unfold` pattern as OpenAI-compatible, but import:

```rust
use crate::protocol::sse::{SseEvent, SseParser, stream_eof_error};
```

Map chunks with:

```rust
fn stream_events_from_sse_data(data: &str) -> Result<Vec<ChatStreamEvent>, LlmError> {
    let value = serde_json::from_str::<Value>(data).map_err(LlmError::stream)?;
    let choice = value["choices"]
        .as_array()
        .and_then(|choices| choices.first())
        .ok_or(LlmError::InvalidProviderPayload("missing choice"))?;
    let mut events = Vec::new();

    if let Some(delta) = choice["delta"]["reasoning_content"].as_str()
        && !delta.is_empty()
    {
        events.push(ChatStreamEvent::ReasoningDelta {
            delta: delta.to_owned(),
        });
    }

    if let Some(delta) = choice["delta"]["content"].as_str()
        && !delta.is_empty()
    {
        events.push(ChatStreamEvent::TextDelta {
            delta: delta.to_owned(),
        });
    }

    if let Some(tool_calls) = choice["delta"]["tool_calls"].as_array() {
        for call in tool_calls {
            events.push(ChatStreamEvent::ToolCallDelta(tool_call_delta_from_value(call)?));
        }
    }

    if let Some(reason) = choice["finish_reason"].as_str() {
        events.push(ChatStreamEvent::Finished {
            finish_reason: finish_reason(Some(reason)),
            usage: usage_from_value(value.get("usage")),
        });
    }

    Ok(events)
}
```

Reuse OpenAI-compatible `tool_call_delta_from_value`, `finish_reason`, and `usage_from_value` as `pub(crate)` helpers.

- [ ] **Step 4: Run test**

Run:

```bash
cargo test -p wyse-llm --test deepseek_provider chat_stream_maps_reasoning_and_text_delta
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/wyse-llm/src/protocol/deepseek.rs crates/wyse-llm/src/protocol/openai_compatible.rs crates/wyse-llm/tests/deepseek_provider.rs
git commit -m "feat: stream deepseek reasoning deltas"
```

## Task 6: Add DeepSeek Smoke Test And AGENTS Notes

**Files:**
- Create: `crates/wyse-llm/tests/deepseek_smoke.rs`
- Modify: `crates/wyse-llm/AGENTS.md`

- [ ] **Step 1: Add ignored smoke test**

Create `crates/wyse-llm/tests/deepseek_smoke.rs`:

```rust
use std::error::Error;

use wyse_llm::{
    ApiKey, ChatMessage, ChatRequest, ChatRole, DeepSeekModel, DeepSeekProvider,
    DeepSeekThinking, LlmProvider,
};

#[tokio::test]
#[ignore = "requires WYSE_LLM_TEST_BASE_URL, WYSE_LLM_TEST_API_KEY, and WYSE_LLM_TEST_MODEL"]
async fn deepseek_provider_returns_chat_response() -> Result<(), Box<dyn Error>> {
    let base_url = std::env::var("WYSE_LLM_TEST_BASE_URL")?;
    let api_key = ApiKey::new(std::env::var("WYSE_LLM_TEST_API_KEY")?);
    let model = match std::env::var("WYSE_LLM_TEST_MODEL")?.as_str() {
        "deepseek-v4-flash" => DeepSeekModel::V4Flash,
        "deepseek-v4-pro" => DeepSeekModel::V4Pro,
        _ => return Err("WYSE_LLM_TEST_MODEL must be deepseek-v4-flash or deepseek-v4-pro".into()),
    };
    let provider = DeepSeekProvider::new(base_url, api_key, model, DeepSeekThinking::Disabled);

    let response = provider
        .chat(ChatRequest::new(model.model_id()).with_message(ChatMessage::user("Say ok.")))
        .await?;

    assert_eq!(response.message.role, ChatRole::Assistant);

    Ok(())
}
```

- [ ] **Step 2: Update crate AGENTS.md**

Append to `crates/wyse-llm/AGENTS.md`:

```markdown
- DeepSeek provider owns DeepSeek-specific request/response mapping, including `thinking`, `reasoning_effort`, and assistant `reasoning_content`.
- Do not add a default DeepSeek base URL; callers must pass the endpoint explicitly.
- Keep SSE framing in `protocol::sse`; provider modules should only map provider JSON into Wyse events.
- Do not add DeepSeek pricing, concurrency, cache-hit usage, or old-model rejection code until a caller needs it.
```

- [ ] **Step 3: Run smoke test compile check**

Run:

```bash
cargo test -p wyse-llm --test deepseek_smoke -- --ignored
```

Expected without env vars: FAIL early with missing env var if executed. If only compilation is needed, run:

```bash
cargo test -p wyse-llm --test deepseek_smoke --no-run
```

Expected: PASS compilation.

- [ ] **Step 4: Commit**

```bash
git add crates/wyse-llm/tests/deepseek_smoke.rs crates/wyse-llm/AGENTS.md
git commit -m "test: add deepseek provider smoke test"
```

## Task 7: Final Verification

**Files:**
- All changed Rust and doc files.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: no output or only formatted files.

- [ ] **Step 2: Run tests**

Run:

```bash
cargo test --workspace --all-targets
```

Expected: PASS. Ignored smoke tests should not run.

- [ ] **Step 3: Run clippy**

Run:

```bash
cargo clippy --workspace --all-targets
```

Expected: PASS with no warnings promoted to errors.

- [ ] **Step 4: Inspect git diff**

Run:

```bash
git status --short
git diff --stat
```

Expected: only intended `wyse-llm` files are modified. The pre-existing untracked `docs/superpowers/specs/2026-07-05-wyse-llm-design.md` may remain untracked and should not be committed unless explicitly requested.

- [ ] **Step 5: Commit verification cleanup**

If formatting changed files after the previous commits:

```bash
git add crates/wyse-llm
git commit -m "chore: format deepseek provider"
```

If no formatting changes remain, skip this commit.

---

## Self-Review

Spec coverage:

- DeepSeek provider: Task 3.
- No default base URL: Task 3 constructor requires base URL.
- Shared SSE parser: Task 2.
- Assistant `reasoning_content`: Task 1 and Task 4.
- Streaming `ReasoningDelta`: Task 1 and Task 5.
- Ignored smoke test: Task 6.
- AGENTS archive reminder: Task 6.
- No price/concurrency/old-model rejection code: File structure and Task 6 AGENTS notes.

Placeholder scan:

- No `TBD`, `TODO`, or open-ended "add tests" steps.
- Each implementation task includes exact files, commands, and expected result.

Type consistency:

- Uses `DeepSeekProvider`, `DeepSeekModel`, `DeepSeekThinking`, and `DeepSeekReasoningEffort` consistently.
- Uses existing `ApiKey`, `ChatRequest`, `ChatMessage`, `ChatStreamEvent`, `FinishReason`, and `LlmProvider`.
