## Why

H3b 原计划用 sqlite per-session 承载 Hook 记录与事件存储。重新评估后否决：N 个 session 对应 N 个库文件，schema 迁移要逐文件执行；跨 session 查询（观测、调试）基本无解；server 语境下的 WAL/锁/并发调优是纯负担；filesystem + sqlite + NATS 三套存储引擎让备份与运维故事碎片化。

统一 Postgres 足够覆盖全部诉求，且为后续阶段提供更好的原语：H4 的 Tool 幂等键直接是 unique constraint + 事务，W2 的持久化队列直接是 `FOR UPDATE SKIP LOCKED`，保留策略是 `DELETE` 或未来的分区 `DROP PARTITION`。P1 存储合同（关系记录 vs Blob、保留策略、租户分区）也随之获得真正的关系型载体。

数据归属按读写模式切分：filesystem 保留**人写的声明式定义**（agent 定义/配置，对齐主流 agent 配置惯例，可 git 化、可 diff）；Postgres 接管**系统跑出来的执行事实**（agent state、消息历史、journal 事件流）。NATS 观测路径不动，与耐久性无关。

## What Changes

- 新建 `stratum-postgres` crate：`PostgresDurableEventSink`（journal）、`PostgresAgentStore`（state + 消息历史）、`sqlx migrate` migrations、共享连接池与事务 helper。不引入 ORM。
- 三张表：`durable_events`（宽表 + JSONB payload + 物化索引列）、`agent_state`（含 `next_message_seq` 计数器与 `state_version`）、`agent_messages`（消息历史主表，append-only，序号同事务分配）。
- `append_message` 收进单个 PG 事务：序号分配 + 消息行一次 WAL flush，消除部分提交。journal 与消息历史的统一投影待新 kernel 组合进 API 时实现（两条 loop 尚未统一组合，本 change 不建投影器）。
- `stratum-store` 重定位为纯合同层（trait + `AgentState` + `StoreError`）；`FilesystemAgentStore` 与 `StoreEventStreamBus` 迁往 `stratum-infra`（与 filesystem event sink 团聚），依赖方向理顺为 `core ← store ← infra ← postgres ← api`。搬迁为独立 commit。
- `stratum-api` 组合根显式选择后端：`[storage] backend = "postgres" | "filesystem"`，无静默回退，配置缺失或无法连接即启动失败（fail closed）。生产路径只支持 postgres。
- `docker-compose.yml`、`config.example.toml`、`config.docker.toml` 增加 postgres 服务与配置段。
- `stratum-postgres` 自带 `docker-compose.test.yml`（project `stratum-postgres-test`）与 `Makefile`；容器集成测试默认 `#[ignore]`，遵循 workspace 测试惯例。
- 非目标：retention 具体策略与表分区（等真实 SLO 出现，届时 `created_at` 列直接支持 declarative partition）；存量 filesystem 数据迁移（空库起跑，库内演进由 `state_version` 与 serde 版本纪律承担）；NATS 观测路径变更；压缩语义变更（用户可见历史永远完整，压缩只影响模型上下文）。

## Capabilities

### New Capabilities

- `postgres-execution-storage`: Postgres 执行层存储后端——journal 事件表与重放、消息提交的单事务原子写入、状态的条件更新语义、后端显式选择与双后端行为对齐。

### Modified Capabilities

- `agent-loop-resume`: resume 支持从 Postgres 事件流重放，重建结果与 filesystem 后端逐事件一致。

## Impact

- 新增 `crates/stratum-postgres`（schema、两个 trait 实现、迁移文件、测试基建）。
- `crates/stratum-store` 收缩为纯合同 crate；`crates/stratum-infra` 接收两个搬迁文件（`filesystem.rs` agent store、`decorator.rs`）。
- `crates/stratum-api` 组合根按配置装配后端；`crates/stratum-config` 增加 `[storage]` 段。
- workspace `Cargo.toml` 新增成员与 `sqlx` workspace 依赖。
- 部署形态变化：生产部署新增 Postgres 服务依赖（与既有 NATS 同等待遇）；本地单测与嵌入式场景不依赖容器。
