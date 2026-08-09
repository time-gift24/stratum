# stratum-llm 约定

## 职责范围

`stratum-llm` 负责 Stratum LLM 领域类型、模拟提供方和底层提供方协议适配器。

## 设计规则

- 在另一种 trait 拆分确实拥有多个实现之前，只保留一个公开的提供方 trait。
- 将 `protocol::openai_compatible` 视为底层协议适配器，而不是长期的提供方模型。
- 公开的模型标识使用 `stratum_core::ModelId`。
- `LlmProvider` 暴露其绑定的 `ModelId`；Agent/运行时调用方不应再携带重复的模型设置。
- `OpenAICompatibleProvider` 绑定到单个模型；如果请求的 `ChatRequest.model` 与提供方模型不同，
  必须拒绝该请求。
- 工具名称是 LLM 边界上的标识；在有真实调用方需要之前，不得添加内部工具 ID 映射或提供方层级的
  工具选择提示。
- 将未来 OpenAI 和 Anthropic 的流式输出映射为 `LlmEvent`（内核侧由 `AgentTelemetryEvent` 按
  `llm_call_id` 承载）；不得向遥测事件添加 `model_id`、`message_id` 或消息生命周期事件。
- `LlmEvent::TextDelta.role` 只用于普通的 `system`、`user`、`assistant` 和 `tool` 文本；推理内容
  必须继续使用 `LlmEvent::ReasoningDelta`。
- 在有具体调用方之前，不得添加提供方注册表、工厂、管理器、嵌入、重排序或 Anthropic 兼容协议。
- 在真实兼容性差异需要代码处理之前，不得添加 DeepSeek、zhipu 或其他提供方专用分支。
- 不得记录提示词、补全内容、工具参数、API 密钥或提供方原始载荷。
- 工具模式校验属于 `stratum-tools` 的职责，不属于本 crate。
- DeepSeek 提供方负责 DeepSeek 专用的请求/响应映射，包括 `thinking`、`reasoning_effort` 和助手的
  `reasoning_content`。
- 不得添加默认的 DeepSeek 基础 URL；调用方必须显式传入端点。
- SSE 分帧保留在 `protocol::sse` 中；提供方模块只应将提供方 JSON 映射为 Stratum 事件。
- 在调用方需要之前，不得添加 DeepSeek 定价、并发、缓存命中用量或旧模型拒绝代码。
- 提供方负责校验自己的参数对象；调用方接收 `LlmError::InvalidModelParameters`，无需重复提供方
  模式或校验规则。
- LLM 出站的连接建立、非流式请求总时长、流式首响应、流式数据块空闲四个超时，由
  `stratum-config::ProviderConfig` 显式传入 `LlmTimeouts`；不得新增不生效的配置，也不得在适配器
  内硬编码。长时间流式响应不设总时长，但任一数据块的静默时间不得超过空闲上限。
- OpenAI 兼容协议与 DeepSeek 共用有界响应体读取器：非流式成功响应体和提供方错误响应体分别使用
  固定的安全字节上限，并逐数据块监测空闲超时；流式请求的非成功错误响应体还受请求总时长约束，
  禁止通过低速滴流长期占住 Turn。发生超时、传输错误或超限时立即丢弃部分响应体，错误和日志不得
  携带提供方正文。SSE 解析器只限制单个未分隔事件/缓冲区，不限制合法长流的累计长度。
- `ApiKey` 包装 `secrecy::SecretString`，`Debug` 恒为 `[redacted]`；凭据只在协议层构造请求时经
  `expose_secret()` 使用。
