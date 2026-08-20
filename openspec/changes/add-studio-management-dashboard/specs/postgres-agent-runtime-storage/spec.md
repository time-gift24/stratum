## MODIFIED Requirements

### Requirement: Agent 定义与 Runtime 状态由四表模型承载
系统必须（SHALL）只使用 `agents`、`agent_states`、`durable_events` 与 `transcript_compactions` 四张核心 execution 表承载不可变 Agent 版本、runtime state 与 execution truth；可变 authoring definition 只存在于隔离的 Studio database，不进入 execution schema。

`agents` 必须（SHALL）只包含 `id UUID PRIMARY KEY`、`name TEXT NOT NULL`、`version TEXT COLLATE "C" NOT NULL`、正数 `definition_schema_version`、immutable canonical `resolved_definition JSONB NOT NULL` 与 `created_at`，其中 `id` 是服务端生成的 UUIDv7 `AgentId`，并且 `(name, version)` 必须（SHALL）唯一。`agents` 每行只表示一个可复用、不可变的 definition 版本。

`agent_states` 必须（SHALL）只包含 `id UUID PRIMARY KEY`、`agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE RESTRICT`、永久唯一的 UUID `idempotency_key`、`status`、nullable `session_id`、nullable `current_turn_id`、唯一可变的 `model_config JSONB NOT NULL`、非负 `last_event_seq`、`created_at` 与 `updated_at`，其中 `id` 是服务端生成的 UUIDv7 `AgentRuntimeId`。同一 `AgentId` 可以（SHALL）被多个相互隔离的 `AgentRuntimeId` 引用；runtime 生命周期内不得（SHALL NOT）修改其 `agent_id` pin。

`resolved_definition` 必须（SHALL）包含创建时从 Studio definition 解析并校验的 system prompt、按序 tools、默认模型与运行所需的非敏感定义身份。它不得（SHALL NOT）包含 name、version、任一 runtime ID、create model override、effective runtime model、authoring row revision、时间戳、credential、token 或 secret。所有核心外键必须（SHALL）使用 `ON DELETE RESTRICT`，不得（SHALL NOT）cascade 删除这些永久资产。

#### Scenario: 新 Definition 版本创建 Idle Runtime
- **WHEN** runtime create 解析到尚不存在的有效 `(name, version)` 并提交事务
- **THEN** 系统原子插入一条 immutable `agents` row 与一条引用它的 `idle` `agent_states` row，Session/current Turn 为空且 `last_event_seq=0`

#### Scenario: 多个 Runtime 复用同一 Definition 版本
- **WHEN** 两个不同 idempotency key 创建基于同一 exact definition 版本的 runtime
- **THEN** 系统创建两个不同 `AgentRuntimeId`，二者的 `agent_id` 引用同一 `AgentId`，并拥有互不共享的 state、Session、Turn 与 ledger

#### Scenario: Studio Definition 变化不改写既有 Runtime
- **WHEN** runtime 创建后 Studio 中同名 authoring definition 被修改或删除
- **THEN** 该 runtime 的新 Turn 与 resume 继续通过 pinned `AgentId` 使用 execution database 中的 `resolved_definition`，不得重读当前 authoring row 或自动升级

#### Scenario: State 不复制 Ledger Truth
- **WHEN** runtime 产生 usage、approval、terminal outcome 或 runtime snapshot
- **THEN** 这些事实只存在于 durable row 或查询派生结果中，`agent_states` 不增加 outcome、snapshot、usage、approval、hosting、lease 或 `resume_required` 字段

### Requirement: Template Version Tag 由作者命名并原子物化
每份 Studio Agent definition 必须（SHALL）提供 `version` 字符串 tag。tag 的 UTF-8 编码长度必须（SHALL）为 `1..=128` bytes，不得（SHALL NOT）包含控制字符或首尾空白；比较必须（SHALL）使用原始值并区分大小写，不做 trim、case folding、Unicode normalization、SemVer 解析或排序。application 必须（SHALL）使用 validated string newtype，数据库必须（SHALL）以等价 `CHECK` 作最终 backstop。`version` 与 `definition_schema_version` 必须（SHALL）保持独立，前者表示作者命名的版本身份，后者表示 canonical definition codec。

runtime create 请求不得（SHALL NOT）接收、选择或覆盖 version tag。key 未命中后，storage 必须（SHALL）从当时读取的 Studio definition 取得 name 与 tag，并在创建事务内对 exact `(name, version)` 获取 transaction-scoped advisory lock。若 pair 不存在，系统必须（SHALL）生成新 `AgentId` 并插入 row；若已存在且 `definition_schema_version + canonical resolved_definition` 严格相等，必须（SHALL）复用已有 `AgentId`；若已存在但任一值不同，必须（SHALL）返回 typed `AgentVersionConflict` 并回滚，绝不覆盖已有 row。`UNIQUE(name, version)` 必须（SHALL）作为最终并发 backstop。

不同 name 或不同 tag 即使 canonical definition 相同也必须（SHALL）创建独立 `agents` row。系统不得（SHALL NOT）计算、分配或比较 latest、max、next 或数值版本，也不得（SHALL NOT）使用派生内容摘要代替严格 canonical equality。

#### Scenario: Exact Tag 与相同定义复用
- **WHEN** exact `(name, version)` 已存在且 schema version 与 canonical definition 均严格相等
- **THEN** create 复用原 `AgentId`，不新增或修改 `agents` row

#### Scenario: Exact Tag 被复用于不同定义
- **WHEN** 管理员保留 exact name/tag 却修改 prompt、ordered tools、默认模型或其他 canonical 定义内容
- **THEN** create 返回 `AgentVersionConflict`，既有 definition 与所有 pinned runtime 保持不变

#### Scenario: 不同 Tag 的相同定义
- **WHEN** 管理员为与历史定义完全相同的内容提供不同 tag
- **THEN** storage 创建新的 `agents` row 与新 `AgentId`，不得因内容相同而跨 tag 合并

#### Scenario: Tag 没有排序语义
- **WHEN** 管理员依次使用 `"release-10"`、`"release-2"` 或回到历史 tag
- **THEN** storage 只按 exact `(name, version)` 判断，不推断先后、升级或回退

### Requirement: Create Idempotency 永久绑定同一 AgentRuntime
Postgres runtime create 必须（SHALL）以客户端提供的 UUID `Idempotency-Key` 作为 `agent_states.idempotency_key` 的永久唯一值。完成 transport 与 key 格式校验后，系统必须（SHALL）先按 key 查询 `agent_states`；命中时必须（SHALL）无条件返回第一次创建的同一 `AgentRuntimeId` 及其 pinned definition metadata，不得（SHALL NOT）读取 Studio 当前 definition、重新校验新的 model override 或比较请求等价性。

key 未命中时，系统才可（SHALL）读取并校验当前 Studio definition 与 model。在一个 Postgres 事务中，系统必须（SHALL）再次检查 key、按 exact `(name, version)` 物化或复用 `agents` row，并插入引用它的 idle `agent_states` row。版本 row 与 runtime state 必须（SHALL）原子提交；任何失败不得（SHALL NOT）占用 key 或留下无 runtime 引用的孤立新版本。并发相同 key 必须（SHALL）由 unique constraint 收敛，失败方回滚自己的全部 mutation 后重读 winner，并遵循相同 key-only replay。

#### Scenario: 相同 Key 重放
- **WHEN** create response 丢失后调用方以相同 key 重试，无论请求 body 是否与第一次相同
- **THEN** store 在读取 Studio 当前 definition 或重新解释 create intent 前返回第一次创建的同一 runtime，不创建第二个 runtime

#### Scenario: Studio Definition 已变化但 Key 命中
- **WHEN** 首次创建后 Studio definition 被修改或删除，而调用方使用相同 key 重试
- **THEN** store 仍从 `agent_states + agents` 返回原 runtime 与原 definition metadata，不受当前 authoring state 影响

#### Scenario: 创建失败不占 Key 或留下孤立版本
- **WHEN** definition、tag、模型、版本冲突或事务校验失败且 create 未提交
- **THEN** 数据库不存在该 key owner，也不存在该失败事务新插入的无引用 `agents` row

#### Scenario: 并发相同 Key 收敛
- **WHEN** 两个事务以相同 key 并发创建并各自完成 definition preflight
- **THEN** 只有一个事务提交 runtime，失败事务回滚其全部写入并重读 winner

### Requirement: Filesystem Execution 与旧 Store 彻底退出
生产 workspace 必须（SHALL）删除整个 `stratum-store` 与 `stratum-agent-builtin` crate、`AgentStore`、`FilesystemAgentStore`、filesystem durable/history/state/checkpoint、旧 backend selector/fallback、旧 beta migration 以及 `session_operation_claims`、`agent_messages`、`tool_approvals`。`stratum-filesystem` 可以（SHALL）保留 sandboxed 业务文件能力，但 API host 不得（SHALL NOT）再把它用作 Agent definition catalog。

配置必须（SHALL）使用直接的 `[postgres]` execution store 与必需的 `[studio].database_url` authoring store，不得（SHALL NOT）保留 `[agent]`、`[llm]` provider resources、`storage_root` alias、自动 execution 目录或 writable agent-data volume。部署必须（SHALL）删除旧 beta migration 并建立单一最终 baseline；既有数据库连同 sqlx migration history 必须重建，旧物理 filesystem 数据不迁移、不读取，也不由程序自动删除。

#### Scenario: Postgres 不可用时启动失败
- **WHEN** execution 或 Studio store 无法连接、迁移或通过 core readiness
- **THEN** API 启动/ready 失败，不创建 filesystem execution/catalog 目录且不降级为旧 backend 或 boot config

#### Scenario: 旧执行与 Catalog 符号无生产残留
- **WHEN** 对 workspace 搜索旧 store、filesystem sink、compact.jsonl、template catalog、`[agent]`、`[llm]` 和三张已删除表
- **THEN** 生产代码、配置、migration 与测试 fixture 中不存在可执行旧路径，仅迁移说明可以提及名称

#### Scenario: 旧物理文件保留给操作者处理
- **WHEN** cutover 机器仍有历史 filesystem execution 或 template files
- **THEN** 新 binary 忽略它们且不自动导入或删除；操作者按部署说明另行备份或清理
