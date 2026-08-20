## ADDED Requirements

### Requirement: Agent definition REST 资源
Stratum API SHALL 通过 `/v1/agent-definitions` 提供与 runtime `/v1/agent-runtimes` 分离的 Agent definition CRUD，并将所有 DTO 纳入 utoipa OpenAPI。

#### Scenario: 分页列出 definitions
- **WHEN** 客户端 GET `/v1/agent-definitions?page=1&per_page=20&sort=-updated_at`
- **THEN** API 必须返回 `data` 与统一 pagination envelope，且每项包含 agent name、author-supplied `agent_version`、model configuration、tools、prompt 和 updated_at 的真实持久化投影

#### Scenario: 创建 definition
- **WHEN** 客户端 POST 一个带非空 author-supplied `agent_version`、有效且名称未占用的 definition
- **THEN** API 必须原子持久化 definition，返回 201、canonical representation、ETag 与 Location

#### Scenario: 读取 definition
- **WHEN** 客户端 GET `/v1/agent-definitions/{agent_name}`
- **THEN** API 必须返回 canonical representation 与强 ETag；不存在时返回 404

#### Scenario: 更新 definition
- **WHEN** 客户端 PUT 带有不同于当前值的新 `agent_version` 的完整 definition，且 `If-Match` 匹配当前 ETag
- **THEN** API 必须原子替换资源并返回更新后的 representation 与新 ETag

#### Scenario: 更新时复用版本标签
- **WHEN** 客户端 PUT 完整 definition 但 `agent_version` 与当前值相同
- **THEN** API 必须返回 409 `studio_conflict` 并保持原 definition 不变；Studio UI 必须在提交前明确提示作者每次保存都要分配新标签

#### Scenario: 删除 definition
- **WHEN** 客户端 DELETE definition 且 `If-Match` 匹配
- **THEN** API 必须返回 204，仅删除 template definition，不得删除已存在的 runtime Agent、Session、history 或 event

### Requirement: Agent definition 校验
Agent definition 边界 MUST 校验名称、author-supplied `agent_version`、Model、parameters、tools 和 prompt，并拒绝未知字段。

#### Scenario: 名称无效
- **WHEN** agent name 不匹配既有 `AgentName` 规则
- **THEN** API 必须返回 400，且不得创建或覆盖数据库记录

#### Scenario: Model 不存在
- **WHEN** definition 引用当前 managed catalog 中不存在的 Model
- **THEN** API 必须返回 422 和定位到 model 字段的稳定错误，不得保存 definition

#### Scenario: Model parameters 无效
- **WHEN** parameters 不符合所选 Provider adapter 的 parameter schema
- **THEN** API 必须返回 422 和字段级 violation，不得猜测默认值或删除未知参数

#### Scenario: Tool 不可用或重复
- **WHEN** definition 请求不存在或重复的 tool
- **THEN** API 必须返回 422，并保持原 definition 不变

#### Scenario: Prompt 为空
- **WHEN** system prompt trim 后为空
- **THEN** API 必须返回 422，并不得保存空 prompt

### Requirement: Host tools wire catalog
Stratum API SHALL 通过 management-gated `GET /v1/tools` 返回当前 host binary 真实可注册的只读工具目录，供 Agent definition 表单选择；该端点不得把 Tool 变成可创建、更新或删除的 Studio 资源。

#### Scenario: 列出可用 tools
- **WHEN** 已启用管理面的客户端 GET `/v1/tools`
- **THEN** API 必须返回每个可注册 tool 的稳定 name、安全 description、read/write kind 与 danger level，并将 DTO 与 path 纳入 management OpenAPI fragment

#### Scenario: 管理面关闭时读取 tools
- **WHEN** `management_enabled = false`
- **THEN** `/v1/tools` 与对应 OpenAPI path 必须不可用，但 runtime 已持久化 definition 的工具解析语义不得改变

### Requirement: Agent definition 并发与重复保护
Agent definition 写入 MUST 使用名称唯一性与 ETag 前置条件避免丢失更新。

#### Scenario: 创建重复名称
- **WHEN** POST 的 agent name 已存在
- **THEN** API 必须返回 409 `studio_conflict` 且不得覆盖现有 definition

#### Scenario: 更新 revision 过期
- **WHEN** PUT 的 `If-Match` 不匹配当前 representation
- **THEN** API 必须返回 412 `studio_precondition_failed` 且不得修改持久化状态

#### Scenario: 删除 revision 过期
- **WHEN** DELETE 的 `If-Match` 不匹配当前 representation
- **THEN** API 必须返回 412 并保留 definition

### Requirement: Definition 变更的 snapshot 语义
Agent definition 更新 SHALL 只影响之后创建的 runtime Agent，现存 runtime Agent 必须继续使用其已持久化 definition 与 runtime snapshot。

#### Scenario: 更新正在被使用的 definition
- **WHEN** 管理员在某个 runtime Agent 正在执行时更新同名 definition
- **THEN** 当前 Turn 和该 runtime Agent 不得被热重配、中断或改变；之后新建的 Agent 必须使用新 definition

#### Scenario: 删除被历史 Agent 使用的 definition
- **WHEN** 管理员删除一个已有历史 runtime Agent 使用过的 definition
- **THEN** 删除可以成功且历史保持可恢复，但以后以该 definition 名称创建 Agent 必须返回 404

### Requirement: Definition 持久化安全
Agent definition MUST 只持久化到独立 Studio PostgreSQL database，并与 execution ledger、template 文件和 Provider credential table 保持明确边界。

#### Scenario: 写入过程中失败
- **WHEN** definition transaction 的校验、写入或提交任一步失败
- **THEN** API 必须返回类型化 5xx，旧 definition 仍保持完整可读，且不得留下部分更新的数据库记录

#### Scenario: 启动恢复
- **WHEN** Host 启动并发现数据库中的 definition 无法解析为严格领域类型或违反引用不变量
- **THEN** 恢复必须 fail closed 并报告安全错误，不得从 template 文件补齐、静默忽略或猜测修复

#### Scenario: template 文件与 database 不同
- **WHEN** `/templates` 中存在同名或不同内容的 definition
- **THEN** Studio 管理读取、`/v1/agent-templates` 与新 AgentRuntime 创建必须只使用数据库记录，不得导入或覆盖它
