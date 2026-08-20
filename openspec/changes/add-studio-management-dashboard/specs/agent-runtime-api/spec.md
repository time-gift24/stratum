## REMOVED Requirements

### Requirement: Template Catalog 是只读热目录
**Reason**: Agent authoring 已迁入独立 Studio PostgreSQL；继续热读 `[agent].templates_root` 会让同一 definition 同时受数据库和文件控制，违反 DB-only 合同。
**Migration**: 删除 `[agent]` 配置与 template mount。先备份旧 TOML，再通过 loopback Studio 管理 API 按 Provider → Model → Agent definition 显式建立 catalog；系统不自动导入。

## ADDED Requirements

### Requirement: Template Catalog 是 Studio definition 的兼容投影
系统必须（SHALL）通过 `GET /v1/agent-templates` 投影 Studio PostgreSQL 中当前 Agent definitions。Host 启动必须（SHALL）连接并验证 Studio database；空 catalog 合法并返回空列表。catalog 与 idempotency key 未命中的 runtime create 不得（SHALL NOT）读取 boot config、环境变量或 template 文件，也不得自动创建或导入 definition。

每份 Studio definition 必须（SHALL）包含作者提供的 version 字符串 tag。tag 必须（SHALL）通过 UTF-8 长度 `1..=128` bytes、无控制字符且无首尾空白的校验；它大小写敏感、没有排序或 SemVer 语义。兼容 DTO 必须（SHALL）只返回创建界面需要的公开 name、version 与安全模型信息，不得（SHALL NOT）返回 prompt、tools JSON、credential 或其他完整 definition 内容。

#### Scenario: 空 Studio Catalog
- **WHEN** Studio database 已迁移但没有 Agent definition
- **THEN** 服务正常启动，`GET /v1/agent-templates` 返回 `200 OK` 与空列表

#### Scenario: 数据库 Definition 更新
- **WHEN** 管理员以新 version tag 成功更新 Studio Agent definition
- **THEN** 后续 catalog 读取与新 runtime create 使用新 definition，更新前创建的 runtime 仍 pin 原 immutable AgentId

#### Scenario: 配置或文件不能覆盖数据库
- **WHEN** 主机上存在旧 `[agent]` 配置或同名 template TOML
- **THEN** 配置边界拒绝旧字段，runtime 不读取文件，也不导入或覆盖 Studio definition

#### Scenario: Studio Database 无效
- **WHEN** Studio database URL 缺失、连接或 migration 失败，或 definition 持久值无法严格解码
- **THEN** 服务启动 fail closed，不回退到内置、配置或 filesystem definition

## MODIFIED Requirements

### Requirement: AgentRuntime 创建是 Key-only 幂等的纯持久化操作
系统必须（SHALL）通过 `POST /v1/agent-runtimes` 创建长期运行聚合。请求体必须（SHALL）且只能包含 `agent_name` 与可选完整 `model_config`，请求必须（SHALL）携带由客户端生成的 UUID `Idempotency-Key`。请求不得（SHALL NOT）包含 version、`AgentId`、`AgentRuntimeId`、user message、`SessionId` 或 `TurnId`；创建不得（SHALL NOT）调用模型、启动 `AgentLoop` 或生成 Turn event。

在完成请求大小、JSON 语法、strict DTO shape 与 key 格式等边界校验后，系统必须（SHALL）先按 idempotency key 查询 `agent_states`，再执行 create 业务语义校验或读取 Studio definition。key 命中时必须（SHALL）无条件返回首次创建的同一 runtime，不比较此次 `agent_name` 或 model override，也不重新校验当前 definition/model；key 是 command identity，不是请求指纹。系统必须（SHALL）从 `agent_states + agents` 重构语义相同的 `201 Created` body 与同一 `Location`。

key 未命中时，系统必须（SHALL）从 Studio database 读取并校验当前 definition name 与作者 tag，完成 definition/model/tool preflight，并构造不含 create override 的 canonical `resolved_definition`。创建事务必须（SHALL）按 exact `(name, version)` 获取 transaction-scoped advisory lock，再次检查 key，并执行以下唯一规则：

- pair 不存在时插入新的 immutable `agents` row 与新 `AgentId`；
- pair 存在且 `definition_schema_version + resolved_definition` 严格相等时复用原 `AgentId`；
- pair 存在但定义不同时返回 `409 agent_version_conflict` 并回滚；
- tag 不同即使定义相同也插入新的 `agents` row。

事务必须（SHALL）生成新 `AgentRuntimeId`，原子提交可能的新 definition row 与引用它的 idle `agent_states` row，并以完整 create override 或 Studio definition 默认值初始化唯一 `model_config`。失败不得（SHALL NOT）消费 key 或留下孤立版本；并发相同 key 必须（SHALL）由 unique constraint 收敛后回滚输家并按 key-only 规则重读 winner。

成功响应必须（SHALL）使用固定 `AgentRuntimeCreated` DTO，且只能包含 `agent_runtime_id`、pinned `agent_id`、`agent_name`、`agent_version` 与 runtime `created_at`。响应必须（SHALL）为 `201 Created` 并携带 `Location: /v1/agent-runtimes/{agent_runtime_id}`；不得（SHALL NOT）包含随后可变的 model、status、Session、Turn、usage、approval 或 barrier。

#### Scenario: 纯创建 AgentRuntime
- **WHEN** 客户端用有效 UUID key 和有效 Studio definition 调用 `POST /v1/agent-runtimes`
- **THEN** API 返回固定 `AgentRuntimeCreated`、201 与 runtime Location；state 为 idle、Session/current Turn 为空且没有模型调用或 durable Turn event

#### Scenario: 相同 Key 与不同 Body 重试
- **WHEN** 第一次 create 已提交但响应丢失，客户端用同一 key 和不同 `agent_name` 或 model override 重试
- **THEN** API 不重读 Studio definition 或比较请求，返回原 runtime 相同语义的 201 body 与 Location

#### Scenario: Studio Definition 已变化但 Create 重试命中 Key
- **WHEN** 原 create 成功后 Studio definition 被修改或删除，客户端以同一 key 重试
- **THEN** API 在读取 Studio database 当前 definition 前命中原 state 并返回原 runtime 与 pinned definition metadata

#### Scenario: Exact Tag 相同定义被不同 Key 复用
- **WHEN** 不同 key 基于 same name、same tag 与相同 canonical definition 创建 runtime
- **THEN** API 创建不同 `AgentRuntimeId` 但复用同一 `AgentId`

#### Scenario: Exact Tag 不同定义冲突
- **WHEN** key 未命中且当前 Studio definition 复用已存在 name/tag 却改变 canonical definition
- **THEN** API 返回 `409 agent_version_conflict`，不覆盖历史 definition、不创建 runtime 且不消费 key

#### Scenario: 不同 Tag 相同定义创建新版本
- **WHEN** 管理员为相同 canonical definition 提供不同 tag 后创建 runtime
- **THEN** API 插入新 `agents` row 与新 `AgentId`，新 runtime pin 它

#### Scenario: Create Key 缺失或无效
- **WHEN** create 请求缺少 `Idempotency-Key` 或其值不是合法 UUID
- **THEN** API 返回 `400 invalid_request`，不读取 Studio definition 且不产生 durable mutation

#### Scenario: Web 保留未决 Create Key
- **WHEN** Web 使用 `crypto.randomUUID()` 发起 create 但无法确定请求是否成功
- **THEN** Web 为该 pending create 保留同一 key 并重试；只有形成新的 create intent 时才生成新 key
