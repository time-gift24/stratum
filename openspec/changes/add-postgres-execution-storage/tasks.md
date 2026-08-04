## 1. stratum-store 纯合同化（独立 commit，先行）

- [x] 1.1 `FilesystemAgentStore`（含其错误类型与测试）迁往 `stratum-infra`，与 filesystem event sink 同 crate；`stratum-store` 移除 `stratum-infra`、`stratum-filesystem` 依赖
- [x] 1.2 `StoreEventStreamBus` decorator 迁往 `stratum-infra`；`stratum-store` 只保留 `AgentStore` trait、`AgentState`/`AgentStatus`、`StoreError`
- [x] 1.3 全 workspace import 修正；`cargo check --workspace --all-targets` 通过

## 2. stratum-postgres crate 骨架与 schema

- [x] 2.1 workspace 新增 `stratum-postgres` 成员；引入 `sqlx`（postgres + runtime-tokio + uuid + chrono + json）workspace 依赖；crate 结构按 design §4（`error.rs` / `events.rs` / `store.rs` / `tx.rs`）
- [x] 2.2 `sqlx migrate` 迁移文件：`durable_events`、`agent_state`、`agent_messages` 三表（design §2/§3 schema），约束与索引齐全（`UNIQUE(turn_id, seq)`、`PRIMARY KEY(agent_id, message_seq)`、`INDEX(session_id, id)`）
- [x] 2.3 crate 测试基建：`docker-compose.test.yml`（project `stratum-postgres-test`）、`Makefile`（默认 podman compose，`COMPOSE` 可覆盖）、集成测试默认 `#[ignore]`

## 3. Postgres 后端实现

- [x] 3.1 `PostgresDurableEventSink`：构造绑定 run 寻址（session/agent/turn），内部 per-run seq 计数器；`append` 单条事务写入，payload 序列化字节与 jsonl 一致；唯一约束冲突映射类型化错误
- [x] 3.2 事件读取器：按 `turn_id` ORDER BY seq 取回完整 `DurableAgentEvent` 序列，供 resume 重放
- [x] 3.3 `PostgresAgentStore`：`load_agent` / `update_state` / `start_turn` / `complete_iteration` 以条件 UPDATE 表达前置校验，影响零行映射为与 filesystem 后端一致的前置条件错误
- [x] 3.4 `append_message` 单事务原子写入：`next_message_seq` 原子递增 RETURNING + `agent_messages` 行；`history_page` 走主表主键范围读

## 4. stratum-api 组合根与配置

- [x] 4.1 `stratum-config` 增加 `[storage]` 段：`backend = "postgres" | "filesystem"`（拒绝未知值）、postgres 连接串与 pool 参数
- [x] 4.2 组合根按 backend 装配；缺失/非法/无法连接即启动失败（fail closed），无静默回退
- [x] 4.3 `docker-compose.yml` 增加 postgres 服务（healthcheck + volume）；`config.example.toml`、`config.docker.toml` 增加 `[storage]` 段，生产默认 postgres

## 5. 测试

- [x] 5.1 单测：事件序列化字节与 jsonl 一致、序号分配与消息行同事务原子性（失败整体回滚）、条件 UPDATE 前置失败映射
- [x] 5.2 双后端对齐测试：同一事件序列（覆盖五个 Hook 点、压缩、终态）经两种后端各 replay 一遍 resume，committed context / 迭代前沿 / Hook 查表逐项一致
- [x] 5.3 容器集成测试（`#[ignore]`）：迁移 up 与表结构、崩溃窗口恢复矩阵（复用 H3a 用例）跑 PG 后端、并发 append_message 序号无竞态（postgres_backend 12 例 + dual_backend_replay 3 例，podman compose 实跑通过）
- [x] 5.4 既有测试不回退：filesystem 后端单测、API 测试全绿

## 6. 文档、质量门禁与校验

- [x] 6.1 TODO.md：H3b（sqlite per-session）改写为 Postgres 统一执行层存储；确认 H3a 遗留项"ExtensionSet/Handler 版本固定"已被 H2 覆盖并勾掉
- [x] 6.2 归档 `crates/stratum-postgres/AGENTS.md`（schema、事务纪律、测试基建用法、投影器未来边界），更新 `crates/stratum-store`、`crates/stratum-infra`、`crates/stratum-api` 的 AGENTS.md（新定位与后端装配）
- [ ] 6.3 运行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`
- [ ] 6.4 constitution-review：对照仓库根 CONSTITUTION.md 派发子代理分条款审查本 change 的完整 diff，修复全部 red-flag 与 violation
- [ ] 6.5 运行 `openspec validate add-postgres-execution-storage --type change --strict --no-interactive` 与 `openspec validate --all --strict`
