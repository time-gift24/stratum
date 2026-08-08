# stratum-llm AGENTS.md

## Scope

`stratum-llm` owns Stratum LLM domain types, the mock provider, and low-level provider protocol adapters.

## Design Rules

- Keep one public provider trait until another real trait split has multiple implementations.
- Treat `protocol::openai_compatible` as a low-level protocol adapter, not the long-term provider model.
- Use `stratum_core::ModelId` for public model identity.
- `LlmProvider` exposes its bound `ModelId`; agent/runtime callers should not carry a duplicate model setting.
- `OpenAICompatibleProvider` is bound to one model; reject requests whose `ChatRequest.model` differs from the provider model.
- Tool names are the LLM boundary identity; do not add internal tool-id mapping or provider-level tool selection hints until a real caller needs them.
- Map future OpenAI and Anthropic streaming output to `LlmEvent`（kernel 侧由 `AgentTelemetryEvent` 按 `llm_call_id` 承载）；do not add `model_id`, `message_id`, or message lifecycle events to telemetry events.
- Use `LlmEvent::TextDelta.role` only for normal `system`, `user`, `assistant`, and `tool` text; keep reasoning as `LlmEvent::ReasoningDelta`.
- Do not add provider registry, factory, manager, embedding, rerank, or Anthropic-compatible protocol without a concrete caller.
- Do not add DeepSeek, zhipu, or other provider-specific forks until a real compatibility difference needs code.
- Do not log prompts, completions, tool arguments, API keys, or raw provider payloads.
- Tool schema validation belongs in `stratum-tools`, not here.
- DeepSeek provider owns DeepSeek-specific request/response mapping, including `thinking`, `reasoning_effort`, and assistant `reasoning_content`.
- Do not add a default DeepSeek base URL; callers must pass the endpoint explicitly.
- Keep SSE framing in `protocol::sse`; provider modules should only map provider JSON into Stratum events.
- Do not add DeepSeek pricing, concurrency, cache-hit usage, or old-model rejection code until a caller needs it.
- Providers own validation of their parameter object; callers receive `LlmError::InvalidModelParameters`
  without duplicating provider schemas or validation rules.
- LLM 出站必须显式限时（`protocol::openai_compatible` 的共享常量）：connect 10s；非流式 `chat` 总限时 120s；`chat_stream` 只对首响应等待限时 30s，流体本身不设总限时（由 Turn 的 CancellationToken 负责取消）。新增 provider 必须复用 `default_client()` 与同一组常量。
