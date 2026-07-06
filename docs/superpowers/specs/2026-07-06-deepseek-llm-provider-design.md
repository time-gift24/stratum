# DeepSeek LLM Provider 设计

日期：2026-07-06

## 目标

为 `wyse-llm` 接入 DeepSeek provider，支持 DeepSeek V4 的 OpenAI Chat Completions 形态、思考模式、reasoning 输出和现有 tool/structured-output 能力。

本轮做：

- 独立的 DeepSeek provider 请求/响应映射
- 共享 SSE framing/parser，供 DeepSeek 和 OpenAI-compatible provider 复用
- assistant message 级别的 `reasoning_content`
- streaming `ReasoningDelta`
- 本地 mock HTTP 测试和 ignored smoke test

本轮不做 provider registry、factory、manager、Anthropic API、FIM、前缀续写、上下文硬盘缓存控制、自动重试、限流器或内置默认 base URL。

## 官方数值

DeepSeek 官方文档当前列出的 OpenAI 格式 base URL 是 `https://api.deepseek.com`，但 provider 不内置默认值，调用方必须显式传入。

支持模型：

- `deepseek-v4-flash`
- `deepseek-v4-pro`

模型能力与数值：

- 上下文长度：`1M`，配置示例中为 `1048576` tokens
- 官方最大输出长度：`384K`，即 `393216` tokens
- Agent 配置示例默认输出长度：`32768` tokens
- Flash 并发限制：`2500`
- Pro 并发限制：`500`
- Flash 价格：缓存命中输入 `0.02` 元 / 百万 tokens，缓存未命中输入 `1` 元 / 百万 tokens，输出 `2` 元 / 百万 tokens
- Pro 价格：缓存命中输入 `0.025` 元 / 百万 tokens，缓存未命中输入 `3` 元 / 百万 tokens，输出 `6` 元 / 百万 tokens

旧模型名 `deepseek-chat` 和 `deepseek-reasoner` 将于北京时间 `2026-07-24 23:59` 弃用。DeepSeek provider 不把它们作为推荐或枚举模型。本轮不额外实现旧模型名拒绝逻辑。

参考来源：

- DeepSeek 模型与价格：https://api-docs.deepseek.com/zh-cn/quick_start/pricing
- DeepSeek 限速与隔离：https://api-docs.deepseek.com/zh-cn/quick_start/rate_limit
- DeepSeek 对话补全：https://api-docs.deepseek.com/zh-cn/api/create-chat-completion
- DeepSeek 思考模式：https://api-docs.deepseek.com/zh-cn/guides/thinking_mode
- DeepSeek Agent/Crush 配置示例：https://api-docs.deepseek.com/zh-cn/quick_start/agent_integrations/crush

## 架构

新增共享 `protocol::sse` 模块。该模块只负责 SSE framing：

- 从 byte stream 中识别 `\n\n` 和 `\r\n\r\n`
- 忽略注释行和空行
- 合并多行 `data:`
- 识别 `data: [DONE]`
- 处理 TCP chunk 切分
- 对 partial EOF 返回 stream error

`protocol::sse` 不理解 OpenAI 或 DeepSeek JSON shape。它只暴露共享 `SseParser` 和 `SseEvent`，OpenAI-compatible 与 DeepSeek 各自保留现有 `unfold` stream 映射逻辑。

`protocol::openai_compatible` 移除私有 SSE parser，改为复用 `protocol::sse`。OpenAI-compatible 的 JSON chunk 到 `ChatStreamEvent` 的映射继续留在该模块内。

新增 DeepSeek provider 模块，使用自己的 request/response 映射和 HTTP 错误映射，但底层 streaming framing 复用 `protocol::sse`。DeepSeek provider 不复用 `OpenAICompatibleProvider` 的完整协议映射，因为 DeepSeek 的 thinking、reasoning 和多轮 reasoning 回传需要明确的 provider 语义；但两者共享 SSE 底座，避免复制流处理。

## Public API

`ChatMessage` 增加：

```rust
pub reasoning_content: Option<String>
```

该字段只在 assistant message 上有语义。序列化时使用 `#[serde(default, skip_serializing_if = "Option::is_none")]`。新增 builder：

```rust
pub fn with_reasoning_content(mut self, content: impl Into<String>) -> Self
```

`ChatStreamEvent` 增加：

```rust
ReasoningDelta { delta: String }
```

DeepSeek 类型：

```rust
pub struct DeepSeekProvider { ... }

pub enum DeepSeekModel {
    V4Flash,
    V4Pro,
}

pub enum DeepSeekThinking {
    Enabled { effort: Option<DeepSeekReasoningEffort> },
    Disabled,
}

pub enum DeepSeekReasoningEffort {
    High,
    Max,
}
```

构造 API 保持直接：

```rust
impl DeepSeekProvider {
    pub fn new(
        base_url: impl Into<String>,
        api_key: ApiKey,
        model: DeepSeekModel,
        thinking: DeepSeekThinking,
    ) -> Self;
}
```

`DeepSeekModel` 只提供模型名转换：

- `model_id() -> ModelId`
- `as_str() -> &'static str`

官网价格、并发、上下文和输出长度只保留在设计文档中，不进入本轮代码。需要 cost 或限流时再加真实调用点需要的类型。

## 请求映射

DeepSeek provider 构造 payload：

- `model`
- `messages`
- `stream`
- `tools`
- `response_format`
- `thinking`
- `reasoning_effort`

`thinking` 由 provider 级 `DeepSeekThinking` 决定：

- `Enabled` 映射为 `{"thinking": {"type": "enabled"}}`
- `Disabled` 映射为 `{"thinking": {"type": "disabled"}}`
- `Enabled { effort: Some(High) }` 映射 `reasoning_effort: "high"`
- `Enabled { effort: Some(Max) }` 映射 `reasoning_effort: "max"`

本轮不向 `ChatRequest` 增加 `max_tokens`、temperature、top_p 或 provider option。DeepSeek 的 max output 数值只保留在设计文档中。

消息映射沿用 OpenAI Chat Completions 形态。assistant message 如果带 `reasoning_content`，DeepSeek provider 在请求中输出同名字段，支持思考模式工具调用后的多轮上下文拼接。

## 响应映射

非流式响应：

- `message.content` 映射到 assistant `ChatMessage.content`
- `message.reasoning_content` 映射到 assistant `ChatMessage.reasoning_content`
- `message.tool_calls` 映射到现有 `ToolCall`
- `finish_reason` 映射到现有 `FinishReason`
- `usage.prompt_tokens` / `completion_tokens` / `total_tokens` 映射到 `TokenUsage`

DeepSeek 的 `prompt_cache_hit_tokens`、`prompt_cache_miss_tokens`、`completion_tokens_details.reasoning_tokens` 本轮不进入 `TokenUsage`，避免扩大 usage 公共 API。后续如果 cost 或 observability 需要，可以单独设计扩展 usage 类型。

流式响应：

- `delta.reasoning_content` 映射为 `ChatStreamEvent::ReasoningDelta`
- `delta.content` 映射为 `ChatStreamEvent::TextDelta`
- `delta.tool_calls` 映射为 `ChatStreamEvent::ToolCallDelta`
- terminal chunk 映射为 `ChatStreamEvent::Finished`

如果同一个 chunk 同时包含 reasoning 和 content，按 provider payload 顺序稳定产出 reasoning delta 再产出 text delta。

## 错误处理

DeepSeek provider 复用 `LlmError`，不新增 provider 专属错误 enum。

- 模型不匹配和请求侧不支持能力返回 `LlmError::InvalidRequest`
- 请求 URL/header/body 构造失败返回 `RequestBuild`
- reqwest 失败返回 `Transport`
- 非 2xx status 返回 `ProviderStatus`
- provider error JSON 解析失败返回 `ProviderPayloadDecode`
- 成功响应 shape 不符合预期返回 `InvalidProviderPayload`
- stream chunk 或 SSE 失败返回 `Stream`

`ProviderStatusError` 继续只保留 status、provider code、短 message 和 request id。错误路径必须 redact API key，不记录 prompt、completion、tool arguments、reasoning_content 或完整 raw payload。

DeepSeek 官方错误码 `400/401/402/422/429/500/503` 只作为测试和文档参考，不预先建强类型枚举。

## 可观测性

library 只允许通过 `tracing` 发安全字段，不安装 subscriber。

允许字段：

- provider：`deepseek`
- model
- HTTP status
- request id
- token usage

禁止字段：

- API key
- prompt
- completion
- reasoning_content
- tool arguments
- raw provider payload

## 测试

普通测试覆盖：

- `protocol::sse` 解析 LF / CRLF delimiter
- `protocol::sse` 忽略 comment 和空行
- `protocol::sse` 合并多行 data
- `protocol::sse` 识别 `[DONE]`
- `protocol::sse` 处理 split TCP chunk
- `protocol::sse` 对 partial EOF 返回错误
- OpenAI-compatible provider 抽出 SSE 后 stream 行为不变
- DeepSeek request body 包含 `thinking` 和 `reasoning_effort`
- DeepSeek request body 能回传 assistant `reasoning_content`
- DeepSeek 非流式响应把 `reasoning_content` 放进 assistant message
- DeepSeek stream 映射 `ReasoningDelta`、`TextDelta`、`ToolCallDelta` 和 `Finished`
- API key debug/error redaction

真实网络测试使用 `#[ignore]`，需要环境变量：

- `WYSE_LLM_TEST_BASE_URL`
- `WYSE_LLM_TEST_API_KEY`
- `WYSE_LLM_TEST_MODEL`

本地验证：

- `cargo fmt`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets`

## 非目标

本轮不做：

- 默认 DeepSeek base URL
- provider registry / factory / manager
- Anthropic-compatible protocol
- DeepSeek FIM 补全
- DeepSeek 对话前缀续写
- DeepSeek 上下文硬盘缓存控制
- `max_tokens` 请求字段
- temperature / top_p / penalty 参数
- usage cache hit/miss 公共 API
- reasoning token 计费自动计算
- 自动限流或重试
- tool schema validation

## 验收标准

- `wyse-llm` 暴露 `DeepSeekProvider`
- `DeepSeekProvider` 构造时必须显式传 `base_url`
- `DeepSeekProvider` 支持 `deepseek-v4-flash` 和 `deepseek-v4-pro`
- 旧 DeepSeek 模型名不会成为推荐 API，也没有专门的旧名拒绝分支
- `ChatMessage` 可以承载 assistant `reasoning_content`
- DeepSeek 非流式响应能保留 reasoning 内容
- DeepSeek 流式响应能发出 `ReasoningDelta`
- OpenAI-compatible 和 DeepSeek provider 共享 SSE framing/parser
- 普通测试不需要真实 DeepSeek 凭据
- secret、prompt、completion、tool arguments 和 reasoning 内容不会进入错误文本或日志
- 实现完成后，将 DeepSeek provider 约定归档到 `crates/wyse-llm/AGENTS.md`
