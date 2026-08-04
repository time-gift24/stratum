# stratum-postgres 约定

## Scope

- `stratum-postgres` 是执行事实的 Postgres 存储后端：单一 schema 承载
  `durable_events`（agent-loop 耐久事件 journal）、`agent_state`（每 agent 运行态）、
  `agent_messages`（已提交消息历史，append-only）。定义层（agent 定义/配置）留在
  filesystem，本 crate 只管系统跑出来的执行事实。
- `PostgresBackend::connect` 建共享连接池并在连接时跑完所有 pending migrations；
  `agent_store(agent_id)` 与 `event_sink(...)` 从它构造。`PostgresAgentStore` 实现
  `stratum-store` 的 `AgentStore` 合同，`PostgresDurableEventSink` 实现 `stratum-infra`
  的 `DurableEventSink` 合同；后端自身错误只经 `StoreError::backend` /
  `DurableEventSinkError::backend` 进入合同错误，合同 crate 保持后端无关。
- 依赖方向 `core ← store ← infra ← postgres ← api`；workspace 中只有本 crate 依赖
  sqlx，不引入 ORM。连接池、迁移、事务 helper（`tx.rs`）是 crate 内部实现细节，
  不泄漏成跨 crate 合同。

## Schema 与迁移纪律

- 三表：`durable_events`（`UNIQUE (turn_id, seq)` 兜底重复/乱序写入，
  `INDEX (session_id, id)`，payload 为 jsonb）、`agent_messages`
  （`PRIMARY KEY (agent_id, message_seq)`，`history_page` 是主键范围哑读）、
  `agent_state`（`agent_id` 主键，`state_version` 管 `AgentState` 格式演进，
  `next_message_seq` 记录已分配的最后一个消息序号）。
- 索引列（`session_id`/`agent_id`/`turn_id`/`seq`/`event_type`/`location`）从事件或
  envelope 投影物化为列；查询不从 JSONB 里挖，不给 JSONB 建 GIN。
- schema 不镜像事件 enum：`DurableAgentEvent` 的 `#[non_exhaustive]` 演进归 serde
  版本管，加变体不写 migration。
- schema 演进只走新 migration 文件（sqlx migrate，文件内嵌于 crate）；已应用的
  migration 文件不得修改。retention 策略与表分区暂不实现，`created_at` 列已预留。

## 事务纪律

- `append_message` 收进单个 PG 事务：`agent_state.next_message_seq += 1 RETURNING`
  分配序号 + 插入 `agent_messages` 行一次 WAL flush，序号空洞/重复与部分提交
  两类损坏在结构上不存在；`load_agent` 因此是哑读，无 filesystem 后端的
  崩溃窗口对账。消息序号从 1 起（`next_message_seq` 初值 0，分配即递增添一）。
- `start_turn` / `complete_iteration` / `update_state` 的前置语义镜像 filesystem
  后端：先读状态行按相同顺序跑校验（产出完全相同的 `StoreError` 变体），再发
  条件 UPDATE——WHERE 子句把可变前置条件重写为原子守卫。影响行数为 0 表示并发
  写入赢了就绪竞争，此时重读状态行、重跑有序校验来分类出精确错误。
- `durable_events` 的 per-run `seq` 从 1 起，镜像 `events.jsonl` 行号；sink 内部
  async lock 串行化 append，提交顺序即 append 顺序。重开同一 `turn_id` 的 sink
  从已持久化的最大 `seq` 后续号（resume 语义）；`UNIQUE (turn_id, seq)` 违规映射为
  `DuplicateSequence` fail closed。
- 每次 `append` 是单语句事务，返回即已提交，语义对齐 filesystem sink 的 fsync
  确认。`read_events`：无行即空事件流；读失败与 malformed payload 都是 typed
  error——单事务写入不会产生 filesystem jsonl 那种崩溃截断尾行，不容忍。

## payload 的 jsonb 语义

- jsonb 不保字节级 key 顺序与空白，因此"与 jsonl 行一致"的合同是反序列化后
  逐字段相等（`serde_json::from_value::<DurableAgentEvent>(payload) == event`），
  不是字节一致。`read_events` 与 round-trip 测试按此断言。
- `agent_messages.envelope` 读出时先过严格形状校验（strict key 白名单）再反序列化，
  与 filesystem 后端同规则。

## 测试基建

- 单元测试不需要容器：序号/校验逻辑用纯函数与 mock `sqlx::DatabaseError` 覆盖。
- 集成测试在 `tests/`（`postgres_backend.rs`、`dual_backend_replay.rs`），全部默认
  `#[ignore]`，经 crate 内 `Makefile` 跑：`make test-integration` 自动
  `up -d --wait` compose 栈（project `stratum-postgres-test`，postgres:17-alpine，
  宿主机端口 45432）并在退出时 `down -v`。默认 `podman compose`，需 Docker 时用
  `COMPOSE="docker compose"` 覆盖；也可 `make test-up` 后手动
  `cargo test -p stratum-postgres -- --ignored`。
- 数据库 URL 默认指向 compose 栈，可用环境变量 `STRATUM_POSTGRES_TEST_URL` 覆盖。
- `dual_backend_replay.rs` 是双后端行为对齐证明：同一事件序列经 filesystem 与
  postgres 两个后端各持久化并读回，逐事件相等，且经公开的 `AgentLoop::resume`
  重放后重建结果（committed context、迭代前沿、hook journal 判定、终态拒绝）
  完全一致。

## 未来边界（明确不做，记录在案）

- journal→`agent_messages` 投影器：新 kernel 组合进 API 前，kernel run 的消息只
  在 `durable_events` 里，`history_page` 不覆盖它们。组合发生时需实现一个投影器
  把 committed `MessageAppended`（可同事务）落成 `agent_messages` 行，写入路径在
  本 crate 内闭合。在此之前不要给本 crate 加投影或双写逻辑。
- 压缩不碰 `agent_messages`：用户可见历史永远 append-only，压缩只影响模型可见的
  loop committed context；删除的唯一入口是未来的 retention（整 session/时间窗粒度）。
- 不做存量 filesystem 数据迁移：空库起跑，库内演进由 `state_version` 列与 serde
  版本纪律承担。
