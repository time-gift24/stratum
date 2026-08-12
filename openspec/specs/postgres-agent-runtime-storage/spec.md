# postgres-agent-runtime-storage Specification

## Purpose
定义 Postgres 四表模型、不可变 Agent template 版本、AgentRuntime state、连续 durable ledger 与严格恢复查询合同。
## Requirements
### Requirement: Agent 定义与 Runtime 状态由四表模型承载
系统必须（SHALL）只使用 `agents`、`agent_states`、`durable_events` 与 `transcript_compactions` 四张核心表承载 Agent template 版本、runtime state 与 execution truth。

`agents` 必须（SHALL）只包含 `id UUID PRIMARY KEY`、`name TEXT NOT NULL`、`version TEXT COLLATE "C" NOT NULL`、正数 `definition_schema_version`、immutable canonical `resolved_definition JSONB NOT NULL` 与 `created_at`，其中 `id` 是服务端生成的 UUIDv7 `AgentId`，并且 `(name, version)` 必须（SHALL）唯一。`agents` 每行只表示一个可复用、不可变的 template 版本。

`agent_states` 必须（SHALL）只包含 `id UUID PRIMARY KEY`、`agent_id UUID NOT NULL REFERENCES agents(id) ON DELETE RESTRICT`、永久唯一的 UUID `idempotency_key`、`status`、nullable `session_id`、nullable `current_turn_id`、唯一可变的 `model_config JSONB NOT NULL`、非负 `last_event_seq`、`created_at` 与 `updated_at`，其中 `id` 是服务端生成的 UUIDv7 `AgentRuntimeId`。同一 `AgentId` 可以（SHALL）被多个相互隔离的 `AgentRuntimeId` 引用；runtime 生命周期内不得（SHALL NOT）修改其 `agent_id` pin。

`resolved_definition` 必须（SHALL）包含创建时从 template 解析并校验的 system prompt、按序 tools、template 默认模型与运行所需的非敏感定义身份。它不得（SHALL NOT）包含 name、version、任一 runtime ID、create model override、effective runtime model、原始 TOML、host path、时间戳、内容指纹、credential、token 或 secret。所有核心外键必须（SHALL）使用 `ON DELETE RESTRICT`，不得（SHALL NOT）cascade 删除这些永久资产。

#### Scenario: 新 Template 版本创建 Idle Runtime
- **WHEN** runtime create 解析到尚不存在的有效 `(name, version)` 并提交事务
- **THEN** 系统原子插入一条 immutable `agents` row 与一条引用它的 `idle` `agent_states` row，Session/current Turn 为空且 `last_event_seq=0`

#### Scenario: 多个 Runtime 复用同一 Template 版本
- **WHEN** 两个不同 idempotency key 创建基于同一 exact template 版本的 runtime
- **THEN** 系统创建两个不同 `AgentRuntimeId`，二者的 `agent_id` 引用同一 `AgentId`，并拥有互不共享的 state、Session、Turn 与 ledger

#### Scenario: Template 变化不改写既有 Runtime
- **WHEN** runtime 创建后 filesystem 中的同名 template 被修改、改 tag 或删除
- **THEN** 该 runtime 的新 Turn 与 resume 继续通过 pinned `AgentId` 使用数据库中的 `resolved_definition`，不得重读 template 或自动升级

#### Scenario: State 不复制 Ledger Truth
- **WHEN** runtime 产生 usage、approval、terminal outcome 或 runtime snapshot
- **THEN** 这些事实只存在于 durable row 或查询派生结果中，`agent_states` 不增加 outcome、snapshot、usage、approval、hosting、lease 或 `resume_required` 字段

### Requirement: Template Version Tag 由作者命名并原子物化
每份 template TOML 必须（SHALL）在顶层提供 `version` 字符串 tag。tag 的 UTF-8 编码长度必须（SHALL）为 `1..=128` bytes，不得（SHALL NOT）包含控制字符或首尾空白；比较必须（SHALL）使用原始值并区分大小写，不做 trim、case folding、Unicode normalization、SemVer 解析或排序。application必须（SHALL）使用validated string newtype，数据库必须（SHALL）以等价`CHECK`作最终backstop。`version` 与 `definition_schema_version` 必须（SHALL）保持独立，前者表示作者命名的版本身份，后者表示 canonical definition codec。

runtime create 请求不得（SHALL NOT）接收、选择或覆盖 version tag。key 未命中后，storage 必须（SHALL）从当时读取的 template 取得 name 与 tag，并在创建事务内对 exact `(name, version)` 获取 transaction-scoped advisory lock。若 pair 不存在，系统必须（SHALL）生成新 `AgentId` 并插入 row；若已存在且 `definition_schema_version + canonical resolved_definition` 严格相等，必须（SHALL）复用已有 `AgentId`；若已存在但任一值不同，必须（SHALL）返回 typed `AgentVersionConflict` 并回滚，绝不覆盖已有 row。`UNIQUE(name, version)` 必须（SHALL）作为最终并发 backstop。

不同 name 或不同 tag 即使 canonical definition 相同也必须（SHALL）创建独立 `agents` row。系统不得（SHALL NOT）计算、分配或比较 latest、max、next 或数值版本，也不得（SHALL NOT）使用派生内容摘要代替严格 canonical equality。

#### Scenario: Exact Tag 与相同定义复用
- **WHEN** exact `(name, version)` 已存在且 schema version 与 canonical definition 均严格相等
- **THEN** create 复用原 `AgentId`，不新增或修改 `agents` row

#### Scenario: Exact Tag 被复用于不同定义
- **WHEN** template 作者保留 exact name/tag 却修改 prompt、ordered tools、template 默认模型或其他 canonical 定义内容
- **THEN** create 返回 `AgentVersionConflict`，既有 definition 与所有 pinned runtime 保持不变

#### Scenario: 不同 Tag 的相同定义
- **WHEN** template 作者为与历史定义完全相同的内容提供不同 tag
- **THEN** storage 创建新的 `agents` row 与新 `AgentId`，不得因内容相同而跨 tag 合并

#### Scenario: Tag 没有排序语义
- **WHEN** 作者依次使用 `"release-10"`、`"release-2"` 或回到历史 tag
- **THEN** storage 只按 exact `(name, version)` 判断，不推断先后、升级或回退

### Requirement: Create Idempotency 永久绑定同一 AgentRuntime
Postgres runtime create 必须（SHALL）以客户端提供的 UUID `Idempotency-Key` 作为 `agent_states.idempotency_key` 的永久唯一值。完成 transport 与 key 格式校验后，系统必须（SHALL）先按 key 查询 `agent_states`；命中时必须（SHALL）无条件返回第一次创建的同一 `AgentRuntimeId` 及其 pinned definition metadata，不得（SHALL NOT）重读 template、重新校验新的 model override或比较请求等价性。

key 未命中时，系统才可（SHALL）读取并校验当前 template、definition 与 model。在一个 Postgres 事务中，系统必须（SHALL）再次检查 key、按 exact `(name, version)` 物化或复用 `agents` row，并插入引用它的 idle `agent_states` row。版本 row 与 runtime state 必须（SHALL）原子提交；任何失败不得（SHALL NOT）占用 key或留下无 runtime 引用的孤立新版本。并发相同 key 必须（SHALL）由 unique constraint 收敛，失败方回滚自己的全部 mutation后重读 winner，并遵循相同 key-only replay。

#### Scenario: 相同 Key 重放
- **WHEN** create response 丢失后调用方以相同 key 重试，无论请求 body 是否与第一次相同
- **THEN** store 在重读 template 或重新解释 create intent 前返回第一次创建的同一 runtime，不创建第二个 runtime

#### Scenario: Template 已变化但 Key 命中
- **WHEN** 首次创建后 template 被修改、删除或 tag 改变，而调用方使用相同 key 重试
- **THEN** store 仍从 `agent_states + agents` 返回原 runtime 与原 definition metadata，不受 filesystem 当前状态影响

#### Scenario: 创建失败不占 Key或留下孤立版本
- **WHEN** template、tag、模型、版本冲突或事务校验失败且 create 未提交
- **THEN** 数据库不存在该 key owner，也不存在该失败事务新插入的无引用 `agents` row

#### Scenario: 并发相同 Key 收敛
- **WHEN** 两个事务以相同 key 并发创建并各自完成 template preflight
- **THEN** 最多一个 `agent_states` row提交，另一个事务回滚其全部 mutation后返回 winner 的同一 `AgentRuntimeId`

### Requirement: AgentRuntime State 编码 Current 或 Recent Turn
`agent_states.status` 必须（SHALL）只允许 `idle | running | finished | failed | cancelled`，使用 `TEXT + CHECK` 而不是 Postgres enum。`idle` 必须（SHALL）同时具有空 Session/current Turn 与 `last_event_seq=0`；`running` 和三个 terminal status 必须（SHALL）同时具有非空 Session/current Turn。terminal status 只描述最近 Turn，不表示 runtime 永久关闭；其 `current_turn_id` 必须（SHALL）保留到后续合法 admission 原子替换。

当前版本必须（SHALL）建立 `UNIQUE(session_id) WHERE status='running'` partial index，保证 `agent_states` 中同一 Session 至多一个 running AgentRuntime。该约束不得（SHALL NOT）扩展成 Session 表、operation claim、Workflow owner、multi-instance lease 或 scheduler 语义。

#### Scenario: Terminal 后仍可开始新 Turn
- **WHEN** AgentRuntime 的 current Turn 已提交 finished、failed 或 cancelled，且新 message 携带 exact recent Turn CAS
- **THEN** admission 可创建新 Turn并把状态原子变为 running，`AgentRuntimeId`、pinned `AgentId` 与绑定 Session 不变

#### Scenario: 同 Session 两个 Runtime 并发 Admission
- **WHEN** 两个 AgentRuntime 同时尝试把相同 Session 置为 running
- **THEN** partial unique index最多允许一个事务提交，另一个返回 `session_busy` 且不留下 durable row

#### Scenario: Unhosted 不改变 Durable Status
- **WHEN** 进程退出而 Postgres 中 current Turn 仍为 running
- **THEN** state 继续为 running；系统不写 hosted、lease 或 `resume_required`

### Requirement: Durable Ledger 使用 AgentRuntime-wide 连续 Event Sequence
`durable_events` 必须（SHALL）以 `(agent_runtime_id, event_seq)` 为主键，其中 `agent_runtime_id` 引用 `agent_states.id ON DELETE RESTRICT`；row 必须（SHALL）保存 non-null Session/Turn identity、受约束的 `event_type`、正数 `event_version`、variant-only `payload`、nullable runtime snapshot version/snapshot 与 `created_at`。每个 AgentRuntime 的 sequence 必须（SHALL）跨 Turn 从 1 连续到其 `agent_states.last_event_seq`，不得（SHALL NOT）使用 Postgres sequence、sink-local counter、per-Turn seq、`message_seq` 或其他第二前沿。共享同一 `AgentId` 的不同 runtime 必须（SHALL）各自从 1 开始且不得互相锁定或混流。

所有 durable writer，包括 kernel sink、approval resolver 与 started-only reconciliation，必须（SHALL）在事务中 `FOR UPDATE` 锁定 exact `AgentRuntimeId` state，校验 pinned Agent、Session/current Turn/status，分配 `last_event_seq + 1`、插入 row、执行该 event 拥有的 state side effect，并更新 high-water。`DurableEventSink::append` 只能（SHALL）在 commit 成功后确认；NATS publish/notify只能（SHALL）发生在 commit 后，且失败不得（SHALL NOT）回滚或使 kernel 重复 append。

#### Scenario: Kernel 与 Approval Resolver 并发写入
- **WHEN** kernel 与 HTTP approval resolver 同时向同一 AgentRuntime/current Turn追加 durable fact
- **THEN** state row lock将两者线性化并分配相邻且唯一的 event_seq

#### Scenario: 共享 Definition 的 Runtime 独立分配
- **WHEN** 两个 runtime pin 同一 `AgentId` 并并发追加事件
- **THEN** 两者锁定不同 `agent_states` row并独立分配各自 sequence，不产生跨 runtime 排序

#### Scenario: 跨 Turn Sequence 无空洞
- **WHEN** Turn A terminal 后 Turn B 开始
- **THEN** Turn B 的 `LoopStarted.event_seq` 等于该 AgentRuntime 此前 high-water 加一，完整 truth range不存在缺行

#### Scenario: 事务回滚不消耗 Sequence
- **WHEN** event insert、compaction companion insert或state update任一步失败
- **THEN** 整个事务回滚，`last_event_seq` 不前进，后续成功 append复用该下一个值

#### Scenario: Product View 可以有数值空洞
- **WHEN** Hook journal event占用了 durable event_seq但不属于公开 history
- **THEN** 公开 product sequence可以跳号；恢复读取完整 truth range时仍要求无缺行

### Requirement: Event 与 Runtime Snapshot 显式版本化
`definition_schema_version`、`event_version` 与 `runtime_snapshot_version` 必须（SHALL）彼此独立并从 v1 开始；当前版本不得（SHALL NOT）实现 upcaster。未知但结构可识别的更高版本必须（SHALL）映射 `runtime_incompatible`；已知版本无法解码、含 typed shape 之外的未知字段、使用非canonical默认/别名或违反 identity/ordering invariant 必须（SHALL）映射 `durable_state_corrupt`。普通 event 必须（SHALL）经 typed decode 后与 canonical v1 重新编码逐值相等；`ToolApprovalRequested` 仅允许 store-owned `hook_invocation_id` 这一精确扩展，并由 deny-unknown wire shape 解码。

runtime snapshot 必须（SHALL）只附着于 `LoopStarted` durable row，且严格包含 `agent_id: AgentId`、`effective_model_config`、`tool_set_fingerprint`、`skill_set_version_id`、`extension_set_version_id` 与 ordered `hook_handler_versions`。`agent_states.agent_id`、snapshot `agent_id` 与加载的 `agents.id` 必须（SHALL）三者一致；identity 不一致必须（SHALL）在 Handler、Tool 或 provider 外部动作前返回 `durable_state_corrupt`，不得重读 filesystem 修复。现有 Hook journal payload不得（SHALL NOT）为此复制Agent或runtime identity；storage必须（SHALL）通过外层durable row校验exact AgentRuntime/Session/Turn归属。

snapshot 不得（SHALL NOT）包含 `AgentRuntimeId`、prompt、provider重建配置、secret或base；`AgentRuntimeId` 必须（SHALL）只由 API-owned sink与恢复编排在 kernel 外层绑定。`base_event_seq` 必须（SHALL）由 `LoopStarted.event_seq - 1` 推导。snapshot version和snapshot必须（SHALL）同时存在或同时为空，并且每个 `(agent_runtime_id, turn_id)` 必须（SHALL）恰有一个 `LoopStarted`、至多一个 terminal event。

#### Scenario: LoopStarted 固定 Definition 与 Runtime 配置
- **WHEN** 新 Turn 的 `LoopStarted` 事务提交
- **THEN** row保存 v1 snapshot 六项内容，state不保存副本，后续 resume按 pinned `AgentId` 与该 snapshot组合相同 runtime

#### Scenario: Snapshot Pin 与 State 不一致
- **WHEN** snapshot中的 `agent_id` 不等于 exact `agent_states.agent_id` 或加载的 `agents.id`
- **THEN** command返回 `durable_state_corrupt`，不得开始模型、Tool、Hook 或其他外部动作

#### Scenario: 未知 Snapshot Version
- **WHEN** 当前 binary 读取结构完整但版本高于支持范围的 runtime snapshot
- **THEN** command返回 `runtime_incompatible`，不得猜测字段或开始外部动作

#### Scenario: 已知 Event Version 内容损坏
- **WHEN** v1 durable row缺少必需variant字段、含未知字段或identity不合法
- **THEN** 读取返回 `durable_state_corrupt` 并保留底层 source chain

### Requirement: TranscriptCompacted 使用单一 Companion Summary
每个 `TranscriptCompacted` 必须（SHALL）在 durable append 的同一事务中写一条 `durable_events` discriminator row 与一条同 `(agent_runtime_id, event_seq)` 的 `transcript_compactions` companion row。companion 必须（SHALL）保存 `turn_id`、`compacted_iteration`、`upto`、non-null `retained_from_event_seq`、单一 typed system-marker `summary` 与 `created_at`；companion shape 由对应 durable row 的 `event_version` 治理，不增加独立 summary version。该 event 的 durable `payload` 必须（SHALL）固定为空对象，不得（SHALL NOT）复制 summary、iteration、upto或pointer；读取 typed `TranscriptCompacted` 时必须（SHALL）通过一对一 companion 物化。不得（SHALL NOT）保存完整 messages、suffix snapshot、summary digest、filesystem line或byte offset。

companion 是永久 durable fact的一部分而不是可丢失 projection。存在 discriminator却缺 companion、summary无法按已知版本解码，或 companion runtime/Turn/event identity不一致，必须（SHALL）视为 `durable_state_corrupt`。若 summary与identity完整但 `retained_from_event_seq` 不能作为加速起点校验，恢复可以（SHALL）忽略该 pointer并利用永久 event + summary从 ledger起点内存 full replay；不得（SHALL NOT）在线修表或提供通用 rebuild命令。

#### Scenario: Compaction 原子提交
- **WHEN** kernel append合法 `TranscriptCompacted`
- **THEN** discriminator、单一 summary companion与state high-water同时可见，kernel随后才收到 acknowledgement

#### Scenario: Summary 只存一份
- **WHEN** 查询同一 compaction 的 durable row 与 companion
- **THEN** durable payload为空，summary/iteration/upto/pointer只存在于 companion，typed event由join物化而不是从两份JSON比较

#### Scenario: Companion 缺失是 Truth Corruption
- **WHEN** durable ledger存在 `TranscriptCompacted` discriminator但缺少对应 companion或summary损坏
- **THEN** recovery返回 `durable_state_corrupt`，不得把核心事实缺失当作普通checkpoint miss

#### Scenario: Pointer 无效时 Full Replay
- **WHEN** companion summary有效但retained pointer无法校验
- **THEN** recovery忽略加速pointer并从ledger起点内存重放，不写repair row

### Requirement: History 与 Derived View 直接查询 Durable Ledger
Postgres query API必须（SHALL）从固定 high-water 的 `durable_events` 读取 `AgentRuntimeView` 派生事实与完整public product history，不得（SHALL NOT）维护 `agent_messages`、`tool_approvals`、usage、telemetry floor、outcome或resume projection。`AgentRuntimeView` 的 telemetry floor 必须（SHALL）通过严格版本/companion 解码 barrier 内的 `MessageAppended` rows 派生为最后一个 assistant row 的 event sequence；history partial index必须（SHALL）覆盖全部安全public product types：`LoopStarted`、`MessageAppended`、`ToolApprovalRequested`、`ToolApprovalResolved`、`TranscriptCompacted`、`IterationCompleted`、`LoopFinished`、`LoopFailed`与`LoopCancelled`，并排除`ToolExecutionStarted`、Hook journal与其他internal facts。Tool最终结果必须（SHALL）作为 `MessageAppended(role=tool, tool_call_id=CallId)`读取。原始 durable rows与compaction companions必须（SHALL）永久保留，当前change不得（SHALL NOT）提供retention delete或projection rebuild API。

#### Scenario: Pending Approval 从同一 Snapshot 派生
- **WHEN** `AgentRuntimeView` 在PG MVCC snapshot中捕获barrier
- **THEN** status、usage、latest assistant telemetry floor与Requested减Resolved的pending approvals均按该barrier查询，不读取state副本

#### Scenario: History 按 AgentRuntime 隔离
- **WHEN** 两个 AgentRuntime pin同一 `AgentId`
- **THEN** 每个查询只读取自己的 `agent_runtime_id` 分区，不共享history、barrier、approval或usage

#### Scenario: History 不依赖 Message Projection
- **WHEN** 调用方分页读取公开history
- **THEN** query严格映射barrier内完整public product rows供reconcile与pagination共用，系统中不存在需要同步的`agent_messages`表

#### Scenario: Compaction 不删除原文
- **WHEN** transcript已发生一次或多次压缩
- **THEN** 较旧原始`MessageAppended`仍能分页读取，所有summary companion也永久存在

### Requirement: Filesystem Execution 与旧 Store 彻底退出
生产workspace必须（SHALL）删除整个 `stratum-store` 与 `stratum-agent-builtin` crate、`AgentStore`、`FilesystemAgentStore`、filesystem durable/history/state/checkpoint、旧backend selector/fallback、旧beta migration以及 `session_operation_claims`、`agent_messages`、`tool_approvals`。`stratum-filesystem` 必须（SHALL）删除`cas.rs`、`record.rs`、get/put、record version、CAS errors与`LocalFilesystem`内存version state，但保留`VirtualPath`、sandboxed read/list/write/create/remove/apply-patch等真实业务文件能力和只读template读取。

配置必须（SHALL）使用直接的 `[postgres]` 与 `[agent].templates_root`，不得（SHALL NOT）保留`storage_root` alias、自动execution目录或writable agent-data volume。部署必须（SHALL）删除旧beta migration并建立单一最终baseline；既有数据库连同sqlx migration history必须重建，旧物理filesystem数据不迁移、不读取，也不由程序自动删除。

#### Scenario: Postgres 不可用时启动失败
- **WHEN** execution store无法连接、迁移或通过core readiness
- **THEN** API启动/ready失败，不创建filesystem execution目录且不降级为旧backend

#### Scenario: 旧执行符号无生产残留
- **WHEN** 对workspace搜索旧store、filesystem sink、compact.jsonl、CAS record、backend selector和三张已删除表
- **THEN** 生产代码、配置、migration与测试fixture中不存在可执行旧路径，仅迁移说明可以提及名称

#### Scenario: 旧物理文件保留给操作者处理
- **WHEN** cutover机器仍有历史filesystem execution files
- **THEN** 新binary忽略它们且不自动删除；操作者按部署说明另行备份或清理
