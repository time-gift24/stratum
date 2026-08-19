# stratum-postgres 约定

## 范围

- `stratum-postgres` 是唯一执行存储后端：具体执行存储所有者，承载四张执行表
  `agents`（不可变 Agent 模板版本，`UNIQUE(name,version)`）、`agent_states`
  （每个 AgentRuntime 的精简状态：固定的 Agent、创建幂等键、持久状态、绑定的
  Session/当前 Turn、唯一可变的 `model_config`、`last_event_seq` 高水位）、
  `durable_events`（AgentRuntime 范围的仅追加账本）、
  `transcript_compactions`（压缩摘要伴随记录）。不建 Session 表、消息/审批
  投影表、发件箱或重建元数据；所有视图（AgentRuntimeView、历史、待处理
  审批、最新用量、最新 `assistant` 遥测下限）都从账本派生读取。单机 scheduler 另有两张控制表：
  `schedules`（Agent 名称 + cron 定义）与 `schedule_runs`（occurrence 到
  AgentRuntime/Session/Turn 的一向状态索引）；它们不是执行投影、Session 表、租约或分布式 claim。
- 本 crate 暴露窄的具体命令/查询接口（创建/准入/追加/查询/解析），
  采用单一实现、不引入 trait；存储 DTO、状态类型与错误都在 crate 内，错误集中在独立
  `error.rs`（`thiserror::Error`，保留来源链）。只有装配层 `stratum-api` 允许
  调用本 crate；组合层及以下不得依赖。
- 工作区中只有本 crate 依赖 sqlx，不引入 ORM。连接池、迁移、事务辅助函数是 crate
  内部实现细节，不泄漏成跨 crate 合同。

## 数据库模式与迁移纪律

- 执行基线保持四张核心表，scheduler 迁移追加两张控制表；枚举语义用 `TEXT + CHECK` 而非 Postgres 枚举；核心外键
  一律 `RESTRICT`，不提供核心资产的删除路径。
- `agents.id` 是不可变定义的 `AgentId`；`agent_states.id` 是长期运行聚合的
  `AgentRuntimeId`，`agent_states.agent_id` 以 RESTRICT 外键永久固定定义。模板
  `version` 是作者提供、大小写敏感且无排序语义的已校验字符串标签，不是数值或 ID。
- `durable_events` 主键为 `(agent_runtime_id, event_seq)`；载荷只存事件变体数据
  （仅变体 JSON），显式记录 `event_version`；运行时快照/版本两列同时为空或
  同时存在，且只允许、也必须出现在 `LoopStarted` 行。
- 唯一性由约束固定：每个 `(agent_runtime_id, turn_id)` 只有一个 `LoopStarted`，终止事件
  （`finished`/`failed`/`cancelled`）合计最多一个；审批 `Requested`/`Resolved` 经受约束的载荷
  表达式索引精确保证调用/审批身份唯一；`UNIQUE(session_id)
  WHERE status='running'` 实现当前 Agent 专属 Session 的单活。
- 数据库模式演进只通过新的迁移文件完成（`sqlx migrate`，文件内嵌于 crate）；已应用的迁移
  文件不得修改。破坏性的测试版切换不保留旧迁移历史，不做原地升级。
- `schedule_runs` 必须先以 `starting` 写入预分配 `SessionId` 与唯一 idempotency key，且只能一次性转为
  `accepted` 或 `failed`；`accepted` 必须同时固定 AgentRuntime/Agent/Turn，存储边界在同一事务中验证三者、
  Session 及已提交的首条用户消息属于同一执行；`failed` 不得伪造 Turn，携带 runtime 时也必须匹配其 Agent。
  当前数据库不提供 scheduler lease/fencing：同一执行数据库只允许一个调度进程。

## 事务纪律

- 所有持久写入器（内核接收器、审批请求器/解析器、仅有开始事件时的
  对账）共用同一条集中式追加事务模板：`SELECT agent_states ... FOR UPDATE`
  行锁既是无空洞 AgentRuntime 范围 `event_seq` 的分配器，也是同一运行时多个写入器的
  串行化点；`AppendEvent`/`ResolveApproval` 必须携带调用方已绑定的预期 `AgentId`，
  安装恢复时的重验也必须携带同一固定值；锁内校验精确的 AgentRuntime、固定的 Agent、Session/当前 Turn/状态与版本化载荷，插入持久行，只
  应用该事件拥有的状态变更，并在同一提交中推进 `last_event_seq`。
- `TranscriptCompacted` 判别符及其 `transcript_compactions` 伴随行在
  同一事务中原子写入；伴随行只保存单一类型化摘要、`upto`、`compacted_iteration`
  与非空 `retained_from_event_seq`，事件载荷固定为空对象，不复制伴随行字段。
- 接收器只在提交成功后向内核确认；提交后的 NATS 发布由装配层负责，
  失败不回滚 PG、不触发重复追加。

## 版本解码纪律

- `definition_schema_version`、`event_version`、`runtime_snapshot_version` 独立从 v1 起。
  未知的新版本表示数据合法但当前二进制程序不支持，返回 `runtime_incompatible`；已知版本
  无法解码或违反字段不变量，返回 `durable_state_corrupt`。不实现向上转换器，不通过字符串
  解析来分类错误。
- 所有专门化派生读取（待处理审批、最新用量、`read_latest_companion`、
  `read_approval`、`resolve_approval`、`read_open_hook_invocation`）必须选中并校验每一
  参与行的 `event_version` 及压缩伴随关系，禁止把未知版本载荷
  静默按 v1 解码，也禁止非 `TranscriptCompacted` 行关联伴随记录。用于
  `Resolved`/`Consumed`/`Completed` 匹配的 `NOT EXISTS` 行必须在语义排除前通过同一检查；
  未知版本或非法伴随记录必须返回类型化错误，不能静默改变派生真相。
- AgentRuntimeView 的 `telemetry_floor_event_seq` 在同一 MVCC 屏障内，从最新到最旧以
  服务端 `LIMIT` + 键集分页严格解码 `MessageAppended`，取第一个 `assistant` 行的
  AgentRuntime 范围序号；不存在时为 0。禁止用 JSON 角色谓词跳过未知/畸形的新行，
  也不得依赖 Rust 提前丢弃无 `LIMIT` 的 sqlx 流来伪装数据库资源边界；不新增投影。
- 账本 v1 解码必须把类型化事件重新编码为规范 v1，并与输入逐值相等；`core` 的 `serde`
  会忽略的未知字段、别名或非规范默认字段都属于 `durable_state_corrupt`。唯一例外是
  `ToolApprovalRequested` 的存储层自有 `hook_invocation_id`，由精确且拒绝未知字段的传输 DTO
  严格解码后再投影为内核事件。
- 缺少必需的压缩伴随记录/摘要或身份不一致，属于持久真相不完整：
  以 `durable_state_corrupt` 故障关闭，绝不修表或提供重建 API；只有加速指针
  （定位器/`retained_from_event_seq`）无效时才回退到内存完整重放。

## 测试基础设施

- 单元测试不需要容器：约束/校验/解码逻辑使用纯函数与模拟的 `sqlx::DatabaseError` 覆盖。
- 集成测试位于 `tests/`，全部默认使用 `#[ignore]`，经 crate 内 `Makefile` 运行：
  `make test-integration` 自动对 Compose 栈执行 `up -d --wait`（项目
  `stratum-postgres-test`，postgres:17-alpine，宿主机端口 45432），并在退出时执行 `down -v`。
  默认使用 `podman compose`，需要 Docker 时用 `COMPOSE="docker compose"` 覆盖；也可
  在执行 `make test-up` 后手动运行 `cargo test -p stratum-postgres -- --ignored --test-threads=1`
  （测试共用同一数据库并在入口处对六张表执行 `TRUNCATE`，必须单线程运行）。
- 数据库 URL 默认指向 Compose 栈，可用环境变量 `STRATUM_POSTGRES_TEST_URL` 覆盖。
- 竞态与崩溃窗口（并发写入器、序号无空洞、终止事件唯一、审批身份唯一、
  伴随记录原子性）必须在真实 Postgres 集成测试中验证。

## 公共 API 形态

- 所有能力都挂在具体的 `PostgresBackend` 上：`create_agent_runtime` / `begin_turn` /
  `append_event` / `resolve_approval` 四个命令，`read_agent_runtime_state` /
  `read_agent_runtime_view` / `read_history_page` / `read_loop_started` / `read_resume_slice` /
  `read_events_range` / `read_latest_companion` / `read_approval` / `turn_has_user_message`（scheduler 启动对账严格区分 started-only）/
  `find_agent_runtime_by_idempotency_key`（创建时基于键优先重放判定，先于任何模板读取）/
  `read_open_hook_invocation`（审批处理器按精确地址找到唯一开放日志调用）
  十一个查询，外加 `ping` 就绪性探针。
- scheduler 控制面同样挂在具体 `PostgresBackend`：`create_schedule` / `begin_schedule_run` /
  `finish_schedule_run` 三个命令，以及 `read_schedule` / `read_schedules` /
  `read_schedule_runs` / `read_starting_schedule_runs` 四个有界查询；调度循环另用
  `read_scheduler_definitions` 通过单条语句的稳定快照一次读取全部定义，避免 OFFSET 扫描在并发创建时重复或漏调度；不新增 trait 或 manager 层。
- 调用方（装配层）直接构造的命令/查询结构体（`CreateAgentRuntime`、`BeginTurn`、
  `AppendEvent`、`CompactionInput`、`ResolveApproval`、`HistoryQuery`、`ResumeSliceQuery`、
  `HookInvocationLookup`）
  不加 `#[non_exhaustive]`；存储层返回的视图/结果类型保留 `#[non_exhaustive]`。
- 运行时快照复用 `stratum_core::TurnRuntimeSnapshot`（以 `AgentId` 固定不可变
  定义，且不含 `AgentRuntimeId`），
  只在 `LoopStarted` 行信封中持久化，版本列恒为 1。
- `ToolApprovalRequested` 的持久载荷在 `core` 类型化事件之外注入
  `hook_invocation_id`（`core` 事件不携带它，但持久合同要求）；解码回类型化事件时，该字段
  被 `core` 反序列化器忽略，账本查询经 crate 内传输结构体读取。
- 对外事件序号一律使用十进制字符串：`encode_event_seq` / `parse_event_seq`；
  存储内保持 `u64`/`i64`。

## 未来边界（明确不做，记录在案）

- 不做投影表与双写；用户可见历史永远直接读取持久账本，压缩不改写原始
  消息，原始历史永久保留。
- 不做存量文件系统/测试版数据迁移：从空库起步，库内演进由显式版本列与严格解码纪律承担。
- scheduler 定义与 occurrence 索引永久保留、无自动 TTL 或单项删除 API；执行对话按账本“原始历史永久保留”策略处理。需要清理时由管理员对整个测试/部署数据库执行显式生命周期操作。
- 分布式租约/围栏、多实例所有权、暂停/编辑/删除、错过触发补跑与持久取消仍不实现；出现明确需求前不引入跨进程占用抽象。
