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

#### Scenario: 部署配置包含 Provider resource
- **WHEN** boot 配置尝试声明 Provider credential、models、base URL 或 timeout
- **THEN** 配置边界必须拒绝这些已移除字段，且不得用它们构造、seed 或覆盖 runtime Provider

#### Scenario: 替换 credential
- **WHEN** 客户端 PUT Provider 且提供新的 API key
- **THEN** API 必须保存新 secret 且响应不得回显 secret；之后开始的 LLM work 必须从 Studio DB 读取新 credential 组装 snapshot，不得依赖进程级 manager 热替换

#### Scenario: 保留 credential
- **WHEN** 客户端 PUT Provider 且省略 API key
- **THEN** API 必须保留现有 secret，不得将省略解释为空值或删除

### Requirement: Provider Model 子资源
Stratum API SHALL 在 Provider 下以 immutable identity 管理 Model 名称，并继续通过 `ModelId` 提供 provider-scoped canonical identity 与 adapter parameter schema。Model surface 必须只提供 list/create/read/delete；不得暴露无独立语义的 PUT，改变 identity 必须先删除旧 Model 再显式创建新 Model。

#### Scenario: 列出 Models
- **WHEN** 客户端 GET `/v1/providers/{provider}/models`
- **THEN** API 必须返回分页 Model resources，每项包含 canonical `provider:model`、provider-local name、parameter schema 与 updated_at

#### Scenario: 创建 OpenAI Model
- **WHEN** 客户端在已配置 OpenAI Provider 下创建合法且未重复的 model name
- **THEN** API 必须在 Studio database 原子加入 catalog，并让 `/v1/models` 与之后开始的 LLM work / Turn 立即看见该 Model

#### Scenario: 创建 DeepSeek Model
- **WHEN** 客户端请求当前 DeepSeek adapter 不支持的 model name
- **THEN** API 必须返回 422，不得假装通用 OpenAI-compatible model 可用

#### Scenario: 删除 Model
- **WHEN** 客户端 DELETE 未被任何 Agent definition 引用的 Model 且 `If-Match` 匹配
- **THEN** API 必须原子删除并让后续 catalog 读取不再返回它

#### Scenario: 尝试原地更新 Model identity
- **WHEN** 客户端尝试 PUT 或在详情页原地修改 Model provider/name
- **THEN** API 与 UI 必须保持 Model identity 只读；调用方只能在通过 Agent definition 引用检查后删除旧 Model，再创建具有新 identity 的 Model

### Requirement: Provider 与 Model 引用完整性
LLM resource 写入 MUST 保持 Provider、Model 与 Agent definitions 之间的引用完整性。系统不得（SHALL NOT）维护独立的全局 default Model；每个 Agent definition 必须显式选择 Model。

#### Scenario: 删除被 Agent definition 引用的 Model
- **WHEN** Model 被一个或多个 Agent definitions 引用
- **THEN** API 必须返回 409 `studio_conflict` 和结构化 blocker names，不得删除 Model

#### Scenario: 删除 Provider
- **WHEN** Provider 包含被 Agent definition 引用的 Model
- **THEN** API 必须返回 409 Agent definition blocker 列表，不得 cascade 删除、迁移或改写 Agent definitions，也不得部分删除 Provider-owned 资源

#### Scenario: 删除无引用 Provider
- **WHEN** Provider 的 Models 均未被任何 Agent definition 引用且 `If-Match` 匹配
- **THEN** API 必须在同一 Studio transaction 中删除 Provider-owned Models、credential 与 Provider；这些 owned rows 不是 blocker

### Requirement: Provider connection test
Stratum API SHALL 通过 `POST /v1/providers/{provider}/test` 执行一次低副作用且脱敏的连接探测，并且不持久化健康状态。探测 MUST 复用 workspace `reqwest 0.12`，OpenAI endpoint 固定为 `https://api.openai.com/v1/models`、DeepSeek endpoint 固定为 `https://api.deepseek.com/models`，connect 与 overall timeout 均固定 10s，禁止 redirect，且只检查 response status、不得读取 response body。

#### Scenario: 测试成功
- **WHEN** 受支持 Provider 接受凭据并成功响应探测
- **THEN** API 必须返回本次测试成功与完成时间，不得写入持续 online/ready 状态

#### Scenario: 测试失败
- **WHEN** 探测因认证、超时、传输或 provider response 失败
- **THEN** API 必须返回稳定的 sanitized error code/message，不得包含 API key、Authorization header、完整 provider body 或内部堆栈

#### Scenario: Provider 返回 redirect 或大响应体
- **WHEN** 固定 probe endpoint 返回 redirect、非成功 status 或任意大小的 response body
- **THEN** Host 不得跟随 redirect，也不得读取或记录 body；只能依据原始 response status 返回成功或统一脱敏失败

#### Scenario: 并发更新 credential
- **WHEN** 测试进行期间 Provider credential 被成功更新
- **THEN** 本次测试可以完成其捕获的快照，但之后的测试和新 LLM work / Turn 必须从 Studio DB 使用新 credential

### Requirement: Secret 保护
Provider credentials MUST 在内存中使用 secret 类型、只持久化到独立 Studio PostgreSQL database，并在所有读取、OpenAPI example、日志和错误中保持不可见。

#### Scenario: 读取 Provider
- **WHEN** 任意 Provider GET、list 或 error response 被序列化
- **THEN** 响应最多返回 `credential_configured: bool`，不得返回 secret、掩码 secret、长度、hash 或首尾字符

#### Scenario: 记录管理操作
- **WHEN** Provider 创建、更新、测试或失败被 tracing 记录
- **THEN** structured fields 只能包含 provider kind、model id 与安全状态码，不得记录请求 DTO 或 credential

#### Scenario: 持久化 credential
- **WHEN** Provider credential 被创建或替换
- **THEN** secret 必须在同一 Studio transaction 内写入 `studio_provider_credentials`，不得写入 config、template、execution ledger、NATS 或本地 catalog 文件

### Requirement: Studio database 启动与 per-work snapshot
Host SHALL 始终以 Studio PostgreSQL catalog 装配 Provider runtime。生产路径不得保留需要热替换的进程级 Provider catalog/manager cache；每次新的 LLM work / Turn 必须从 Studio DB 读取一致的 Provider、Model 与 credential snapshot 并组装本次 work 的 manager，只有 in-flight Turn pin 住捕获的 Provider `Arc`。

#### Scenario: 空 database 启动
- **WHEN** Studio database 已迁移但没有 Provider、Model 或 Agent definition
- **THEN** Host 必须以空 catalog 启动，管理列表与 `/v1/models` 返回空投影，不得从 boot config、环境 API key 或 template 文件隐式导入资源

#### Scenario: 后续启动
- **WHEN** Studio database 已存在 Provider、Model、credential 与 Agent definition
- **THEN** Host 必须验证并使用数据库内容，不得读取 boot `[llm]` 或环境 API key 来覆盖或补齐资源

#### Scenario: 过渡 catalog 省略 DeepSeek 参数
- **WHEN** 既有 Studio Agent definition 为 DeepSeek Model 持久化空 `model_parameters` 对象
- **THEN** adapter 必须应用其 schema 声明的 disabled-thinking 默认值且不改写数据库；任意非空但无效的参数对象仍必须 fail closed

#### Scenario: database 写入失败
- **WHEN** Provider、Model 或 credential 的 Studio transaction 提交失败
- **THEN** Host 必须保留旧 database state 并返回类型化错误；不得存在需要额外回滚或同步的进程级 runtime catalog

#### Scenario: catalog 更新后的下一次 work
- **WHEN** catalog 更新成功且之后开始一次新的 LLM work / Turn
- **THEN** 新 work 必须从 Studio DB 组装包含已提交变更的 snapshot；更新时已经执行中的 Turn 继续使用其捕获的 Provider `Arc`，但 Turn 结束后不得把该 manager 当作后续 work 的权威 cache

#### Scenario: 管理面关闭
- **WHEN** `management_enabled = false`
- **THEN** Host 仍必须连接并验证 Studio database、把它纳入 readiness、并在每次新 work 从中装配 Provider runtime，但不得注册 Provider、Model、Agent definition、Provider test 或 tools catalog 管理路由，OpenAPI 也不得包含这些 paths

#### Scenario: 缺少 Studio database
- **WHEN** API host 缺少 Studio database URL、连接失败或 migration 失败
- **THEN** 启动必须 fail closed；运行期间 Studio readiness 失败时 `/health/ready` 必须失败，不得回退到 boot Provider 配置

### Requirement: 管理面网络边界
Provider、Model、Agent definition、Provider test 与只读 tools catalog 的 management API MUST 仅在管理面显式启用且 API 绑定 loopback 地址时注册；同一条件必须控制对应 OpenAPI fragment。

#### Scenario: 默认配置
- **WHEN** `management_enabled` 未设置或为 false
- **THEN** 全部 management routes 及其 OpenAPI paths 必须不可用，但既有对话和只读 `/v1/models`、`/v1/agent-templates` 必须继续投影 Studio database，Studio 仍是 runtime 与 readiness 的必需依赖

#### Scenario: 非 loopback 启用管理面
- **WHEN** `management_enabled = true` 且 API bind 不是 loopback
- **THEN** 配置校验必须 fail closed 并阻止服务启动，不得仅依赖 CORS 保护 secret 管理
