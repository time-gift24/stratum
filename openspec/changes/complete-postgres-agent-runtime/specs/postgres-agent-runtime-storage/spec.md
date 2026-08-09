## ADDED Requirements

### Requirement: Agent 定义与薄状态由四表模型承载
系统必须（SHALL）只使用 `agents`、`agent_state`、`durable_events` 与 `transcript_compactions` 四张核心表承载 Agent execution truth。`agents` 必须（SHALL）保存 `agent_id`、一对一唯一的 `agent_version_id`、永久唯一的客户端 `idempotency_key`、`source_template_name`、nullable `creation_model_override`、正数 `definition_schema_version`、immutable `resolved_definition` 与 `created_at`。`resolved_definition` 必须（SHALL）包含创建时已解析和校验的 prompt、按序 tools、creation-time effective model config及运行所需的非敏感定义身份；不得（SHALL NOT）保存原始 TOML、host path、template digest、credential、token 或 secret。

`agent_state` 必须（SHALL）只保存 `agent_id`、`status`、nullable `session_id`、nullable `current_turn_id`、`default_model_config`、`last_event_seq` 与 `updated_at`。不得（SHALL NOT）增加 outcome、runtime snapshot、usage、approval、session claim、hosting、lease 或 `resume_required` 字段。所有核心外键必须（SHALL）使用 `ON DELETE RESTRICT`，不得（SHALL NOT）cascade 删除执行资产。

#### Scenario: 创建不可变 Agent 与 Idle State
- **WHEN** create transaction 使用有效模板与 model override 创建 Agent
- **THEN** 系统原子插入一条 immutable `agents` row 与一条 `idle` state，Session/current Turn 为空且 `last_event_seq=0`

#### Scenario: 模板变化不改写历史 Agent
- **WHEN** Agent 创建后同名 filesystem 模板被修改或删除
- **THEN** 该 Agent 的新 Turn 与 resume 继续使用自己的 `resolved_definition`，数据库不重新解析模板

#### Scenario: State 不复制 Ledger Truth
- **WHEN** Agent 产生 usage、approval、terminal outcome 或 runtime snapshot
- **THEN** 这些事实只存在于对应 durable row或查询派生结果中，`agent_state` schema 不增加副本

### Requirement: Create Idempotency 永久绑定同一 Agent
Postgres create command 必须（SHALL）以客户端提供的 UUID `Idempotency-Key` 作为永久唯一键，并以保存的 `source_template_name + nullable creation_model_override` 定义请求等价性。命中 key 时必须（SHALL）先比较已保存请求，不得（SHALL NOT）重新读取模板：相同请求返回原 `AgentId`，不同请求返回 typed `idempotency_key_conflict`。未命中时才可（SHALL）解析最新模板，并在一个事务中写 Agent 与 state；失败事务不得（SHALL NOT）消费 key。并发相同 key 必须（SHALL）通过唯一约束收敛后重读并执行相同等价判断。

#### Scenario: 相同 Key 与请求重放
- **WHEN** create response 丢失后调用方以相同 key、模板名和 model override 重试
- **THEN** store 返回第一次创建的同一 Agent，不产生第二个 Agent，也不重新读取模板

#### Scenario: 相同 Key 被不同请求复用
- **WHEN** 已使用的 key 携带不同模板名或不同 nullable model override
- **THEN** store 返回 `idempotency_key_conflict`，原 Agent 与 state 保持不变

#### Scenario: 创建失败不占 Key
- **WHEN** 模板、模型或事务校验失败且 create 未提交
- **THEN** 后续调用可继续使用该 key，因为数据库中没有已提交的 key owner

### Requirement: Agent State 编码 Current 或 Recent Turn
`agent_state.status` 必须（SHALL）只允许 `idle | running | finished | failed | cancelled`，使用 `TEXT + CHECK` 而不是 Postgres enum。`idle` 必须（SHALL）同时具有空 Session/current Turn 与 `last_event_seq=0`；`running` 和三个 terminal status 必须（SHALL）同时具有非空 Session/current Turn。terminal status 只描述最近 Turn，不表示 Agent 永久关闭；其 `current_turn_id` 必须（SHALL）保留到后续合法 admission 原子替换。

当前版本必须（SHALL）建立 `UNIQUE(session_id) WHERE status='running'` partial index，保证 Agent runtime rows 中同一 Session 至多一个 running Agent。该约束不得（SHALL NOT）扩展成 Session 表、operation claim、Workflow owner 或 scheduler lease。

#### Scenario: Terminal 后仍可开始新 Turn
- **WHEN** Agent 的 current Turn 已提交 finished、failed 或 cancelled，且新 message 携带 exact recent Turn CAS
- **THEN** admission 可创建新 Turn并把状态原子变为 running，AgentId 与绑定 Session 不变

#### Scenario: 同 Session 两个 Agent 并发 Admission
- **WHEN** 两个 Agent 同时尝试把相同 Session 置为 running
- **THEN** partial unique index最多允许一个事务提交，另一个返回 `session_busy` 且不留下 durable row

#### Scenario: Unhosted 不改变 Durable Status
- **WHEN** 进程退出而 Postgres 中 current Turn 仍为 running
- **THEN** state 继续为 running；系统不写 hosted、lease 或 `resume_required`

### Requirement: Durable Ledger 使用 Agent-wide 连续 Event Sequence
`durable_events` 必须（SHALL）以 `(agent_id,event_seq)` 为主键，并保存 non-null Session/Turn identity、受约束的 `event_type`、正数 `event_version`、variant-only `payload`、nullable runtime snapshot version/snapshot 与 `created_at`。每个 Agent 的 sequence 必须（SHALL）跨 Turn 从 1 连续到 `agent_state.last_event_seq`，不得（SHALL NOT）使用 Postgres sequence、sink-local counter、per-Turn seq、`message_seq` 或其他第二前沿。

所有 durable writer，包括 kernel sink、approval resolver 与 started-only reconciliation，必须（SHALL）在事务中 `FOR UPDATE` 锁定 exact Agent state，校验 Session/current Turn/status，分配 `last_event_seq + 1`、插入 row、执行该 event 拥有的 state side effect，并更新 high-water。`DurableEventSink::append` 只能（SHALL）在 commit 成功后确认；NATS publish/notify只能（SHALL）发生在 commit 后，且失败不得（SHALL NOT）回滚或使 kernel 重复 append。

#### Scenario: Kernel 与 Approval Resolver 并发写入
- **WHEN** kernel 与 HTTP approval resolver 同时向同一 Agent/current Turn追加 durable fact
- **THEN** state row lock将两者线性化并分配相邻且唯一的 event_seq

#### Scenario: 跨 Turn Sequence 无空洞
- **WHEN** Turn A terminal 后 Turn B 开始
- **THEN** Turn B 的 `LoopStarted.event_seq` 等于此前 high-water 加一，完整 truth range不存在缺行

#### Scenario: 事务回滚不消耗 Sequence
- **WHEN** event insert、compaction companion insert或state update任一步失败
- **THEN** 整个事务回滚，`last_event_seq` 不前进，后续成功 append复用该下一个值

#### Scenario: Product View 可以有数值空洞
- **WHEN** Hook journal event占用了 durable event_seq但不属于公开 history
- **THEN**公开 product sequence可以跳号；恢复读取完整 truth range时仍要求无缺行

### Requirement: Event 与 Runtime Snapshot 显式版本化
`definition_schema_version`、`event_version` 与 `runtime_snapshot_version` 必须（SHALL）彼此独立并从 v1 开始；当前版本不得（SHALL NOT）实现 upcaster。未知但结构可识别的更高版本必须（SHALL）映射 `runtime_incompatible`；已知版本无法解码、含typed shape之外的未知字段、使用非canonical默认/别名或违反 identity/ordering invariant 必须（SHALL）映射 `durable_state_corrupt`。普通event必须（SHALL）经typed decode后与canonical v1重新编码逐值相等；`ToolApprovalRequested`仅允许store-owned `hook_invocation_id`这一精确扩展，并由deny-unknown wire shape解码。

runtime snapshot必须（SHALL）只附着于 `LoopStarted` durable row，且严格包含 `agent_version_id`、`effective_model_config`、`tool_set_fingerprint`、`skill_set_version_id`、`extension_set_version_id` 与 ordered `hook_handler_versions`。snapshot不得（SHALL NOT）包含 prompt、provider重建配置、secret或base；`base_event_seq` 必须（SHALL）由 `LoopStarted.event_seq - 1` 推导。snapshot version和snapshot必须（SHALL）同时存在或同时为空，并且每个 `(agent_id,turn_id)` 必须（SHALL）恰有一个 `LoopStarted`、至多一个 terminal event。

#### Scenario: LoopStarted 固定 Runtime Identity
- **WHEN** 新 Turn 的 LoopStarted事务提交
- **THEN** row保存v1 snapshot六项内容，state不保存副本，后续resume按该snapshot组合相同runtime

#### Scenario: 未知 Snapshot Version
- **WHEN** 当前binary读取结构完整但版本高于支持范围的runtime snapshot
- **THEN** command返回 `runtime_incompatible`，不得猜测字段或开始外部动作

#### Scenario: 已知 Event Version 内容损坏
- **WHEN** v1 durable row缺少必需variant字段或identity不合法
- **THEN**读取返回 `durable_state_corrupt` 并保留底层source chain

#### Scenario: 已知 Event Version 含未知字段
- **WHEN** v1 durable row在variant、嵌套message/hook decision或approval wire shape中增加当前合同未声明的字段
- **THEN**读取返回`durable_state_corrupt`，不得由serde静默忽略后继续resume、发布或执行外部动作

### Requirement: TranscriptCompacted 使用单一 Companion Summary
每个 `TranscriptCompacted` 必须（SHALL）在 durable append 的同一事务中写一条 `durable_events` discriminator row 与一条同 `(agent_id,event_seq)` 的 `transcript_compactions` companion row。companion必须（SHALL）保存 `turn_id`、`compacted_iteration`、`upto`、non-null `retained_from_event_seq`、单一 typed system-marker `summary` 与 `created_at`；companion shape由对应durable row的`event_version`治理，不增加独立summary version。该event的durable `payload`必须（SHALL）固定为空对象，不得（SHALL NOT）复制summary、iteration、upto或pointer；读取typed `TranscriptCompacted`时必须（SHALL）通过一对一companion物化。不得（SHALL NOT）保存完整messages、suffix snapshot、`summary_digest`、filesystem line或byte offset。

companion是永久 durable fact的一部分而不是可丢失projection。存在 discriminator却缺 companion、summary无法按已知版本解码，或 companion Agent/Turn/event identity不一致，必须（SHALL）视为 `durable_state_corrupt`。若summary与identity完整但 `retained_from_event_seq` 不能作为加速起点校验，恢复可以（SHALL）忽略该pointer并利用永久event+summary从ledger起点内存full replay；不得（SHALL NOT）在线修表或提供通用rebuild命令。

#### Scenario: Compaction 原子提交
- **WHEN** kernel append合法 TranscriptCompacted
- **THEN** discriminator、单一summary companion与state high-water同时可见，kernel随后才收到acknowledgement

#### Scenario: Summary 只存一份
- **WHEN** 查询同一 compaction 的 durable row 与 companion
- **THEN**durable payload为空，summary/iteration/upto/pointer只存在于companion，typed event由join物化而不是从两份JSON比较

#### Scenario: Companion 缺失是 Truth Corruption
- **WHEN** durable ledger存在 TranscriptCompacted discriminator但缺少对应companion或summary损坏
- **THEN** recovery返回 `durable_state_corrupt`，不得把核心事实缺失当作普通checkpoint miss

#### Scenario: Pointer 无效时 Full Replay
- **WHEN** companion summary有效但retained pointer无法校验
- **THEN** recovery忽略加速pointer并从ledger起点内存重放，不写repair row

### Requirement: History 与 Derived View 直接查询 Durable Ledger
Postgres query API必须（SHALL）从固定 high-water 的 `durable_events` 读取AgentView派生事实与product history，不得（SHALL NOT）维护 `agent_messages`、`tool_approvals`、usage、telemetry floor、outcome或resume projection。AgentView 的 telemetry floor 必须（SHALL）通过严格版本/companion 解码 barrier 内的 `MessageAppended` rows 派生为最后一个 assistant row 的 event sequence；history必须（SHALL）使用只覆盖 `MessageAppended`、`TranscriptCompacted`和安全 `LoopFailed/LoopCancelled` marker的partial index；Tool最终结果必须（SHALL）作为 `MessageAppended(role=tool,tool_call_id=CallId)`读取。原始durable rows与compaction companions必须（SHALL）永久保留，当前change不得（SHALL NOT）提供retention delete或projection rebuild API。

#### Scenario: Pending Approval 从同一 Snapshot 派生
- **WHEN** AgentView在PG MVCC snapshot中捕获barrier
- **THEN** status、usage、latest assistant telemetry floor与Requested减Resolved的pending approvals均按该barrier查询，不读取state副本

#### Scenario: History 不依赖 Message Projection
- **WHEN** 调用方分页读取公开history
- **THEN** query直接映射barrier内可见durable rows，系统中不存在需要同步的`agent_messages`表

#### Scenario: Compaction 不删除原文
- **WHEN** transcript已发生一次或多次压缩
- **THEN**较旧原始MessageAppended仍能分页读取，所有summary companion也永久存在

### Requirement: Filesystem Execution 与旧 Store 彻底退出
生产workspace必须（SHALL）删除整个 `stratum-store` 与 `stratum-agent-builtin` crate、`AgentStore`、FilesystemAgentStore、filesystem durable/history/state/checkpoint、旧backend selector/fallback、旧beta migration以及 `session_operation_claims`、`agent_messages`、`tool_approvals`。`stratum-filesystem` 必须（SHALL）删除`cas.rs`、`record.rs`、get/put、record version、CAS errors与LocalFilesystem内存version state，但保留`VirtualPath`、sandboxed read/list/write/create/remove/apply-patch等真实业务文件能力和只读template读取。

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
