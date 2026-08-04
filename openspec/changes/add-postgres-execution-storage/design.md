# add-postgres-execution-storage 设计记录

## Context

H3a 落地了 filesystem 后端的 journal（`<root>/<run_id>/events.jsonl` + fsync + 派生检查点 `compact.jsonl`），`AgentStore` 与 `DurableEventSink` 两个 trait 边界已就位。H3b 原计划 sqlite per-session，经重新评估否决（维护成本：N 库文件迁移、无跨 session 查询、server 语境并发调优、三引擎碎片）。决策：统一 Postgres 承载全部执行事实，filesystem 退守定义层与 dev/test/嵌入后端。

既有事实（代码核实）：

- `DurableAgentEvent` 13 个变体，tagged JSON `{"type","data"}`，`#[non_exhaustive]`，载荷不含 session/turn 身份——寻址完全外部化（filesystem 的 run 目录）。
- 所有身份类型是 UUIDv7 newtype（时间有序，B-tree 右追加友好）。
- 两条 agent loop 尚未统一组合：新 kernel（`agent_loop`， Hook/journal/resume 所在）只感知 `DurableEventSink`；legacy loop（`loop.rs`，stratum-api 当前驱动）只写 `AgentStore` 历史。**不存在**跨 journal 与消息历史的写入路径——早期讨论中"双写今天已存在"的论断系误读，特此修正记录。
- `stratum-store` 已依赖 `stratum-infra`（`StoreEventStreamBus` 用 `EventStreamBus`），依赖方向拧着。
- `AgentStore` 的 `start_turn` / `complete_iteration` 带前置条件语义（"state 必须是期望的运行迭代"），filesystem 靠原子 rename + 读校验模拟。

## Goals / Non-Goals

**Goals:**

- 单一 PG schema 承载 journal、state、消息历史；单迁移路径（`sqlx migrate`）。
- `append_message` 单事务原子写入：`next_message_seq` 递增 + 消息行，一次 WAL flush。journal 与消息历史的跨 trait 统一（投影器同事务落 `agent_messages`）待新 kernel 组合进 API 时再做，本 change 不建。
- 后端显式可选，生产只支持 postgres；filesystem 保留为 dev/test/嵌入后端与行为参照。
- 双后端行为对齐：同一批事件两种后端各 replay，重建结果逐事件一致。

**Non-Goals:**

- retention 具体策略与表分区（`created_at` 列预留，等真实 SLO）。
- 存量 filesystem 数据迁移（空库起跑）。
- NATS 观测路径、压缩语义、Hook 合同变更。
- ORM、TimescaleDB 等扩展依赖。

## Decisions

### 1. 数据归属：filesystem 管定义，Postgres 管执行事实

```text
定义层（filesystem 保留）          执行层（Postgres 接管）
  agent 定义/配置                    agent_state（status/usage/前沿/快照）
  （声明式、可 git 化）              agent_messages（消息历史）
                                   durable_events（journal 全事件流）
观测层（不动）：NATS JetStream 纯观测 fan-out
```

被否决：sqlite per-session（见 Why）；执行事实留 filesystem（原子提交靠 rename 手搓，分页靠文件命名约定，已是拧巴实现）。

### 2. journal 表：宽表 + JSONB payload + 物化索引列

```sql
durable_events
  id           bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY  -- 全局全序
  session_id   uuid NOT NULL
  agent_id     uuid NOT NULL
  turn_id      uuid NOT NULL        -- run ≈ turn 执行；resume 重开同一 run
  seq          bigint NOT NULL      -- per-run 单调序号，对应 jsonl 行号
  event_type   text NOT NULL        -- DurableAgentEvent::event_type()
  payload      jsonb NOT NULL       -- 完整 {"type","data"} canonical JSON
  created_at   timestamptz NOT NULL DEFAULT now()

  UNIQUE (turn_id, seq)             -- resume：WHERE turn_id ORDER BY seq
  INDEX  (session_id, id)
```

纪律：索引列从事件投影、物化为列，查询不从 JSONB 里挖；schema 不镜像 enum（`#[non_exhaustive]` 演进归 serde 版本管，加变体不写 migration）；payload 存 jsonb（入库即 JSON 校验，临时分析可 `->>`）；不给 JSONB 建 GIN（写入放大主因是索引个数）。

被否决：关系化拆列/拆表——事件流是 append-only log，90% 查询是"按地址取有序前缀"，索引列已覆盖；拆列把"加个 Hook 点"变成"写 migration"，与演进策略冲突。

性能论证：append-only 是 PG 主场——纯 insert 永远往表尾写不触碰旧页；单调键走 B-tree 右边缘 split 优化；无 dead tuple 即无 vacuum 压力；commit 成本（WAL flush）与 filesystem 的 fsync 同量级，group commit 随并发自然合并。量级校准：agent loop 每迭代事件个位数到几十条，数百并发 session 也只是几千 insert/s。

### 3. agent_messages 是消息历史主表，append-only、序号同事务分配

```sql
agent_messages
  agent_id     uuid NOT NULL
  message_seq  bigint NOT NULL      -- 由 agent_state.next_message_seq 分配
  session_id   uuid NOT NULL
  turn_id      uuid NOT NULL
  location     text NOT NULL
  envelope     jsonb NOT NULL       -- 完整 StreamEnvelope
  created_at   timestamptz NOT NULL DEFAULT now()

  PRIMARY KEY (agent_id, message_seq)

agent_state
  agent_id          uuid PRIMARY KEY
  state_version     int NOT NULL    -- AGENT_STATE_VERSION，格式演进用
  status            text NOT NULL
  session_id        uuid
  active_turn_id    uuid
  usage             jsonb NOT NULL
  runtime_snapshot  jsonb           -- TurnRuntimeSnapshot，start_turn 钉死
  next_message_seq  bigint NOT NULL DEFAULT 0
  updated_at        timestamptz NOT NULL
```

关键写入路径：

```text
append_message                        start_turn / complete_iteration
BEGIN                                 UPDATE agent_state SET …
  UPDATE agent_state                     WHERE agent_id=$1 AND status=$expected …
    SET next_message_seq+=1            -- 影响行数=1 才成功，=0 即 fail closed
    RETURNING                          -- 等价 filesystem 的前置校验 + 原子 rename
  INSERT agent_messages
COMMIT  -- 一次 WAL flush
```

理由：`history_page` 是热路径（Web 每进会话都调），必须是主键范围哑读。`agent_messages` 是 `AgentStore` 消息历史的主存储（对应 filesystem 后端的历史文件），不是 journal 的投影——legacy loop 的消息只经 `append_message` 到达这里。序号分配与消息行在同一事务，消除"序号空洞/重复"与部分提交两类损坏。**未来边界（本 change 不建）**：新 kernel 组合进 API 后，kernel run 的消息在 journal 里，届时需要一个投影器把 committed `MessageAppended` 落成 `agent_messages` 行（可以同事务），`history_page` 才能覆盖 kernel run。记录在案，等组合需求出现再实现。

**修正记录在案**：早期讨论曾主张"压缩时 messages 表同步重写"，代码核实后否决——`TranscriptCompacted` 只影响模型可见的 loop committed context（resume 重放重建），用户可见历史（`AgentStore`）append-only、压缩不碰。压缩是模型上下文管理手段，不是内容删除手段；删除的唯一入口是未来的 retention（整 session/时间窗粒度）。

被否决：不建 messages 表、读路径从 journal 派生——把持久化格式复杂度转嫁给热读路径，且压缩基线叠加逻辑会泄漏出 kernel/resume 边界。

### 4. 新建 stratum-postgres crate，sqlx + sqlx migrate，不引 ORM

```text
crates/stratum-postgres/
  migrations/           -- sqlx migrate SQL 文件（内嵌 crate）
  src/
    lib.rs
    error.rs            -- thiserror，独立错误模块
    events.rs           -- PostgresDurableEventSink
    store.rs            -- PostgresAgentStore
    tx.rs               -- crate 私有：多步写入的事务 helper
  docker-compose.test.yml   -- project: stratum-postgres-test
  Makefile
  tests/postgres_store.rs   -- 默认 #[ignore] 的容器集成测试
```

- 两个 PG 实现共享连接池、迁移与事务 helper；这些协作是 crate 内部实现细节，不泄漏成跨 crate 合同——这是单独立 crate 的核心论据。未来投影器需要跨 trait 同事务写入时，也在本 crate 内闭合。
- 依赖收敛：workspace 只有这一个 crate 依赖 sqlx。
- ORM 对窄写宽读的 schema 是纯负担；`sqlx::query!` 编译期校验视 schema 稳定度采用，不稳定处用运行时 `query`。
- 测试遵循 workspace 惯例：单测用 mock/无容器；集成测试 `#[ignore]` + 自带 compose 栈 + Makefile（默认 podman compose，`COMPOSE` 可覆盖）。

### 5. stratum-store 纯合同化，搬迁独立 commit

`stratum-store` = `AgentStore` trait + `AgentState`/`AgentStatus` + `StoreError`。`FilesystemAgentStore` 与 `StoreEventStreamBus` 迁往 `stratum-infra`（宪法钦定"耐久后端允许落 stratum-infra"；decorator 本就依赖 infra 的 `EventStreamBus`）。依赖方向理顺：

```text
core ← store(合同) ← infra(本地后端) ← postgres ← api
```

搬迁与 Postgres 功能无耦合，必须独立 commit；store 不再依赖 infra/filesystem。

### 6. 组合根显式选后端，无静默回退

```toml
[storage]
backend = "postgres"        # 或 "filesystem"
```

- backend 缺失/拼错/PG 无法连接 → 启动失败（fail closed），拒绝"连不上就悄悄用 filesystem"的耐久性隐形降级。
- 生产路径（docker-compose、config.example）默认 postgres；filesystem 服务单测、嵌入式、无容器本地体验。
- 语义差异显式承认：PG 单事务 vs filesystem 两次 fsync——resume 对账逻辑本就容忍后者，两种模式都正确，但生产事故响应只支持 PG 路径。

### 7. 不做存量数据迁移

空库起跑。filesystem 数据原地保留不删（dev 模式仍可读）。库内演进由既有纪律承担：`state_version` 列管 `AgentState` 格式，serde 版本管事件载荷（legacy `loop_started` 升级是先例）。一次性迁移脚本写完即废但要全套正确性论证，纯负资产。

## 验证方法

- 单测：序号分配与消息行的同事务原子性（失败整体回滚）、条件 UPDATE 前置失败、事件 round-trip（与 jsonl 字节一致）。
- 双后端对齐测试：同一事件序列经 filesystem 与 postgres 各 replay 一遍 resume，重建结果逐事件相等。
- 容器集成测试（`#[ignore]`）：迁移 up/down、崩溃窗口恢复矩阵复用 H3a 用例跑 PG 后端。
- 质量门禁：`cargo fmt --check`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`。
