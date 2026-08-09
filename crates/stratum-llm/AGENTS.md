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
- LLM 出站的 connect、非流式 request 总时长、stream 首响应、stream chunk idle 四个 timeout
  由 `stratum-config::ProviderConfig` 显式传入 `LlmTimeouts`，不得新增不生效的配置或在 adapter
  内硬编码。长 streaming response 不设总时长，但任一 chunk 静默不得超过 idle bound。
- OpenAI-compatible 与 DeepSeek 共用 bounded response-body reader：非流成功体、provider error
  体分别使用固定安全 byte cap，逐 chunk 观察 idle timeout；streaming 请求的非成功 error body
  还受 request 总时长约束，禁止通过低速滴流长期占住 Turn。超时、transport 或超限立即丢弃
  partial body，错误和日志不得携带 provider 正文。SSE parser 只限制单个未分隔 event/buffer，
  不限制合法长流的累计长度。
- `ApiKey` 包装 `secrecy::SecretString`，`Debug` 恒为 `[redacted]`；凭据只在协议层构造请求时经 `expose_secret()` 使用。
