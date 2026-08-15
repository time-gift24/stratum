## ADDED Requirements

### Requirement: 受支持 Provider 资源
Stratum API SHALL 管理当前真实支持的 `openai` 与 `deepseek` Provider，且不得把任意 URL 或未实现 adapter 暴露为可配置能力。

#### Scenario: 列出 Provider
- **WHEN** 客户端 GET `/v1/providers`
- **THEN** API 必须返回已配置 Provider 的分页投影，包括 provider enum、`credential_configured`、model 数量与 updated_at，且不得返回 credential 内容或可推断信息

#### Scenario: 创建 Provider
- **WHEN** 客户端 POST 未配置的受支持 Provider 与非空 API key
- **THEN** API 必须验证候选配置、原子持久化并返回 201、Location、canonical representation 与 ETag

#### Scenario: 创建不受支持 Provider
- **WHEN** 请求包含 `openai`、`deepseek` 之外的 provider kind 或自定义 base URL
- **THEN** API 必须拒绝请求且不得保存或发起出站连接

#### Scenario: 替换 credential
- **WHEN** 客户端 PUT Provider 且提供新的 API key
- **THEN** API 必须保存新 secret 并重建后续请求使用的 provider manager，但响应不得回显 secret

#### Scenario: 保留 credential
- **WHEN** 客户端 PUT Provider 且省略 API key
- **THEN** API 必须保留现有 secret，不得将省略解释为空值或删除

### Requirement: Provider Model 子资源
Stratum API SHALL 在 Provider 下管理 Model 名称，并继续通过 `ModelId` 提供 provider-scoped canonical identity 与 adapter parameter schema。

#### Scenario: 列出 Models
- **WHEN** 客户端 GET `/v1/providers/{provider}/models`
- **THEN** API 必须返回分页 Model resources，每项包含 canonical `provider:model`、provider-local name、parameter schema 与 updated_at

#### Scenario: 创建 OpenAI Model
- **WHEN** 客户端在已配置 OpenAI Provider 下创建合法且未重复的 model name
- **THEN** API 必须原子加入 catalog，并让 `/v1/models` 与之后新建 Agent 的模型选择立即看见该 Model

#### Scenario: 创建 DeepSeek Model
- **WHEN** 客户端请求当前 DeepSeek adapter 不支持的 model name
- **THEN** API 必须返回 422，不得假装通用 OpenAI-compatible model 可用

#### Scenario: 删除 Model
- **WHEN** 客户端 DELETE 未被 default model 或任何 Agent definition 引用的 Model 且 `If-Match` 匹配
- **THEN** API 必须原子删除并让后续 catalog 读取不再返回它

### Requirement: Provider 与 Model 引用完整性
LLM resource 写入 MUST 保持 default model、Provider、Model 与 Agent definitions 之间的引用完整性。

#### Scenario: 删除被 Agent definition 引用的 Model
- **WHEN** Model 被一个或多个 Agent definitions 引用
- **THEN** API 必须返回 409 `resource_conflict` 和结构化 blocker names，不得删除 Model

#### Scenario: 删除 default Model
- **WHEN** Model 是当前 default model
- **THEN** API 必须返回 409 并标识 default 引用，直到管理员先选择其他 default

#### Scenario: 删除 Provider
- **WHEN** Provider 包含被引用或 default 的 Model
- **THEN** API 必须返回 409 blocker 列表，不得 cascade 删除或改写 Agent definitions

#### Scenario: 删除无引用 Provider
- **WHEN** Provider 的 Models 均无外部引用且 `If-Match` 匹配
- **THEN** API 可以在一次原子 catalog 更新中删除 Provider 及其 Models

### Requirement: Provider connection test
Stratum API SHALL 通过 `POST /v1/providers/{provider}/test` 执行一次有固定超时、低副作用且脱敏的连接探测，并且不持久化健康状态。

#### Scenario: 测试成功
- **WHEN** 受支持 Provider 接受凭据并成功响应探测
- **THEN** API 必须返回本次测试成功与完成时间，不得写入持续 online/ready 状态

#### Scenario: 测试失败
- **WHEN** 探测因认证、超时、传输或 provider response 失败
- **THEN** API 必须返回稳定的 sanitized error code/message，不得包含 API key、Authorization header、完整 provider body 或内部堆栈

#### Scenario: 并发更新 credential
- **WHEN** 测试进行期间 Provider credential 被成功更新
- **THEN** 本次测试可以完成其捕获的快照，但之后的测试和新 Agent 必须使用新 credential

### Requirement: Secret 保护
Provider credentials MUST 在内存中使用 secret 类型、在持久化文件中限制权限，并在所有读取、OpenAPI example、日志和错误中保持不可见。

#### Scenario: 读取 Provider
- **WHEN** 任意 Provider GET、list 或 error response 被序列化
- **THEN** 响应最多返回 `credential_configured: bool`，不得返回 secret、掩码 secret、长度、hash 或首尾字符

#### Scenario: 记录管理操作
- **WHEN** Provider 创建、更新、测试或失败被 tracing 记录
- **THEN** structured fields 只能包含 provider kind、model id 与安全状态码，不得记录请求 DTO 或 credential

#### Scenario: 持久化 catalog
- **WHEN** managed catalog 被写入 Studio PostgreSQL database
- **THEN** credential 只能被运行进程的 database identity 读取，且不得写入执行 durable events、NATS、OpenAPI、日志或错误响应

### Requirement: Managed catalog 启动与热替换
Host SHALL 以 boot config 与只读 template catalog 首次 seed managed catalog，并在后续启动使用 Studio database；成功管理写入必须作用于后续 provider selection，而不改变已经开始的 Turn。

#### Scenario: 首次启动
- **WHEN** Studio database 的 catalog 为空且 boot `[llm]` 配置与只读 template catalog 有效
- **THEN** Host 必须在一个事务中 seed managed catalog 并装配等价 Provider/Models 和 Agent definitions

#### Scenario: 后续启动
- **WHEN** Studio catalog 已存在
- **THEN** Host 必须验证并使用它，不得用 boot config 或 template files 静默覆盖管理变更

#### Scenario: catalog 写入失败
- **WHEN** candidate manager 可构造但持久化失败
- **THEN** Host 必须保留旧 catalog 与旧 provider manager，并返回类型化错误

#### Scenario: 热替换成功
- **WHEN** catalog 更新完成
- **THEN** 之后启动的 Turn 必须使用新 catalog，已经开始的 Turn 必须继续使用其捕获的 Provider `Arc` snapshot

### Requirement: 管理面网络边界
Provider、Model 与 Agent definition 的写 API MUST 仅在 Studio 管理面显式启用、Studio database 配置有效且 API 绑定 loopback 地址时注册。

#### Scenario: 默认配置
- **WHEN** `management_enabled` 未设置或为 false
- **THEN** 管理写路由必须不可用，既有对话和只读模型/template API 行为保持不变

#### Scenario: 非 loopback 启用管理面
- **WHEN** `management_enabled = true` 且 API bind 不是 loopback
- **THEN** 配置校验必须 fail closed 并阻止服务启动，不得仅依赖 CORS 保护 secret 管理
