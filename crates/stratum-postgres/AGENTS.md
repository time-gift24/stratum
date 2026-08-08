# stratum-postgres 约定

## Scope

- `stratum-postgres` 是唯一执行存储后端：concrete execution storage owner，承载四张表
  `agents`（immutable Agent identity 与 resolved definition snapshot）、`agent_state`
  （薄状态：durable status、绑定 Session/current Turn、mutable default model、
  `last_event_seq` high-water）、`durable_events`（append-only ledger）、
  `transcript_compactions`（压缩 summary companion）。不建 Session 表、message/approval
  projection 表、outbox 或 rebuild metadata；所有视图（AgentView、history、pending
  approvals、latest usage）都从 ledger 派生读取。
- 本 crate 暴露窄的 concrete command/query 接口（create/admission/append/query/resolve），
  单实现、不引入 trait；storage DTO、状态类型与错误都在 crate 内，错误集中在独立
  `error.rs`（`thiserror::Error`，保留 source chain）。只有装配层 `stratum-api` 允许
  调用本 crate；组合层及以下不得依赖。
- workspace 中只有本 crate 依赖 sqlx，不引入 ORM。连接池、迁移、事务 helper 是 crate
  内部实现细节，不泄漏成跨 crate 合同。

## Schema 与迁移纪律

- 最终 baseline 只有四张核心表；枚举语义用 `TEXT + CHECK` 而非 Postgres enum；核心外键
  一律 `RESTRICT`，不提供核心资产的 delete 路径。
- `durable_events` 主键 `(agent_id, event_seq)`；payload 只存 event variant 数据
  （variant-only JSON），显式 `event_version`；runtime snapshot/version 两列同时为空或
  同时存在，且只允许、也必须出现在 `LoopStarted` row。
- 唯一性由约束固定：每 `(agent_id, turn_id)` 只有一个 `LoopStarted`，terminal
  （finished/failed/cancelled）合计最多一个；approval Requested/Resolved 经受约束 payload
  expression index 唯一到 exact invocation/approval identity；`UNIQUE(session_id)
  WHERE status='running'` 实现当前 Agent-only Session 单活。
- schema 演进只走新 migration 文件（sqlx migrate，文件内嵌于 crate）；已应用的 migration
  文件不得修改。破坏性 beta cutover 不保留旧 migration history，不做原地升级。

## 事务纪律

- 所有 durable writer（kernel sink、approval requester/resolver、started-only
  reconciliation）共用同一条集中 append 事务模板：`SELECT agent_state ... FOR UPDATE`
  行锁既是无空洞 agent-wide `event_seq` 的分配器，也是同 Agent 多 writer 的串行化点；
  校验 exact Agent/Session/current Turn/status 与版本化 payload，插入 durable row，只
  应用该 event 拥有的 state 变更，同一 commit 推进 `last_event_seq`。
- `TranscriptCompacted` discriminator 与其 `transcript_compactions` companion row 在
  同一事务原子写入；companion 只保存单一 typed summary、`upto`、`compacted_iteration`
  与非空 `retained_from_event_seq`，event payload 固定为空对象，不复制 companion 字段。
- sink 只在 commit 成功后向 kernel acknowledgement；commit 后的 NATS 发布由装配层负责，
  失败不回滚 PG、不触发重复 append。

## 版本解码纪律

- `definition_schema_version`、`event_version`、`runtime_snapshot_version` 独立从 v1 起。
  未知的新版本表示数据合法但当前 binary 不支持，返回 `runtime_incompatible`；已知版本
  无法解码或违反字段不变量，返回 `durable_state_corrupt`。不实现 upcaster，不通过字符串
  解析分类错误。
- 缺少必需 compaction companion/summary 或 identity 不一致属于 durable truth 不完整：
  `durable_state_corrupt` fail closed，绝不修表或提供 rebuild API；只有加速指针
  （locator/`retained_from_event_seq`）无效时才回退内存 full replay。

## 测试基建

- 单元测试不需要容器：约束/校验/解码逻辑用纯函数与 mock `sqlx::DatabaseError` 覆盖。
- 集成测试在 `tests/`，全部默认 `#[ignore]`，经 crate 内 `Makefile` 跑：
  `make test-integration` 自动 `up -d --wait` compose 栈（project
  `stratum-postgres-test`，postgres:17-alpine，宿主机端口 45432）并在退出时 `down -v`。
  默认 `podman compose`，需 Docker 时用 `COMPOSE="docker compose"` 覆盖；也可
  `make test-up` 后手动 `cargo test -p stratum-postgres -- --ignored --test-threads=1`
  （测试共用同一数据库并在入口处 TRUNCATE 四张表，必须单线程运行）。
- 数据库 URL 默认指向 compose 栈，可用环境变量 `STRATUM_POSTGRES_TEST_URL` 覆盖。
- 竞态与崩溃窗口（并发 writer、sequence 无空洞、terminal 唯一、approval identity 唯一、
  companion 原子性）必须在真实 Postgres 集成测试中验证。

## 公共 API 形态

- 所有能力都挂在 concrete `PostgresBackend` 上：`create_agent` / `begin_turn` /
  `append_event` / `resolve_approval` 四个 command，`read_agent_state` /
  `read_agent_view` / `read_history_page` / `read_loop_started` / `read_resume_slice` /
  `read_events_range` / `read_latest_companion` / `read_approval` /
  `find_agent_by_idempotency_key`（create 的 key-first 重放判定，先于任何模板读取）/
  `read_open_hook_invocation`（审批 Handler 按 exact 地址找到唯一 open journal invocation）
  十个 query，外加 `ping` readiness 探针。
- 调用方（装配层）直接构造的 command/query struct（`CreateAgent`、`BeginTurn`、
  `AppendEvent`、`CompactionInput`、`ResolveApproval`、`HistoryQuery`、`ResumeSliceQuery`、
  `HookInvocationLookup`）
  不加 `#[non_exhaustive]`；store 返回的 view/outcome 类型保留 `#[non_exhaustive]`。
- runtime snapshot 复用 `stratum_core::TurnRuntimeSnapshot`（恰好是 design 固定的六字段），
  只在 `LoopStarted` row envelope 持久化，版本列恒为 1。
- `ToolApprovalRequested` 的 durable payload 在 core typed event 之外注入
  `hook_invocation_id`（core 事件不携带它，durable 合同要求）；解码回 typed event 时该字段
  被 core 反序列化器忽略，ledger 查询经 crate 内 wire struct 读取。
- 对外 event sequence 一律十进制字符串：`encode_event_seq` / `parse_event_seq`；
  存储内保持 `u64`/`i64`。

## 未来边界（明确不做，记录在案）

- 不做 projection 表与双写；用户可见历史永远直接读 durable ledger，压缩不改写原始
  messages，原始历史永久保留。
- 不做存量 filesystem/beta 数据迁移：空库起跑，库内演进由显式版本列与严格解码纪律承担。
- durable scheduling、lease/fencing、多实例 ownership、durable cancel 延期为独立
  scheduler change；本 crate 不提前引入跨进程 claim 抽象。
