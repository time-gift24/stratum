# Stratum 运行时运维手册

> 面向部署与运维人员。架构原理见 [`ARCH.md`](../ARCH.md)，HTTP 合同以运行时
> `/api-docs/openapi.json`（utoipa 生成）为唯一权威，crate 级实现约定见各 crate 的
> `AGENTS.md`。本文对应 change `complete-postgres-agent-runtime` 的最终状态。

## 1. 存储模型

Postgres 是 Agent 执行事实的**唯一**持久化存储，没有 backend selector 或静默回退。
最终 schema 只有四张核心表（`stratum-postgres` crate 内嵌 sqlx migration，
`PostgresBackend::connect` 启动时自动应用 baseline）：

- `agents`：immutable Agent identity、创建幂等键与创建时固化的 resolved definition
  snapshot（prompt、按序 tools、creation-time effective model）。Agent 创建后永不
  重新读取模板。
- `agent_state`：薄状态——durable status（`idle/running/finished/failed/cancelled`）、
  绑定的 Session/current Turn、mutable default model、`last_event_seq` high-water。
  不保存 outcome、usage、snapshot、approval 或 hosting。
- `durable_events`：append-only ledger。`(agent_id, event_seq)` 主键；`event_seq` 是
  Agent-wide、无空洞的十进制序列，由 `agent_state` 行锁在集中 append 事务中分配。
  对外（API/history/SSE frame）一律编码为十进制字符串。payload 为 variant-only JSON，
  显式 `event_version`；runtime snapshot 只附着在 `LoopStarted` row。
- `transcript_compactions`：与 `TranscriptCompacted` discriminator 同事务写入的
  durable companion，只保存单一 typed summary、`upto`、`compacted_iteration`
  与 `retained_from_event_seq` 保留指针。原始 durable messages 永久保留，
  压缩不改写历史。

没有 projection 表：AgentView、history 分页、pending approvals、latest usage 全部
从 ledger 派生读取。核心资产没有 delete API，外键一律 `RESTRICT`。

### 破坏性 beta cutover

本次切换**不做数据迁移**：旧 beta migration 已删除，部署时整体 drop 并重建数据库
（包括 `_sqlx_migrations` 表），并重建 NATS stream。回滚到旧 binary 必须同时恢复旧
schema/config/NATS，不能只回滚应用。

程序不会扫描、迁移、修改或删除磁盘上的旧 filesystem 执行数据（旧
`storage_root`/state/history/compact.jsonl 等）；这些文件的备份与清理由运维自行处理。

## 2. 配置

完整可注释样例见根目录 `config.example.toml`；配置解析严格拒绝未知字段。

- `[postgres].url`：必填，缺失或为空即启动失败（fail closed）。Postgres 不可用则
  启动失败、readiness 503。
- `[agent].templates_root`：只读 Agent template catalog 目录。启动时校验存在、是目录
  且可读（空目录允许）；服务绝不自动创建该目录。
- `[nats]`：连接与 Agent-scoped 短 tail 上限——`url`、`stream_name`、
  `subject_prefix`、`replicas`（1..=5）、`max_age_seconds`、`max_bytes`、
  `max_messages`（有限上限 + discard-old）与 `connect_timeout_seconds`。
- `[llm]`：默认模型与各 provider 的 `api_key`/`models`；默认模型与 template 选择的
  模型必须属于对应 provider 的 `models` 列表。每个 provider 显式配置 connect、
  non-stream request、stream first-response 与 stream chunk-idle timeout；长流允许持续，
  但任一 chunk 静默超过 idle bound 会以 typed transport error 结束。非流成功体、provider
  error body 与单个 SSE event 另有固定安全 byte cap，避免无限聚合；不限制长流累计长度。
- `[api]`：`bind`（省略时固定 `127.0.0.1:8080`）与 `allowed_origins`（CORS，拒绝
  `*`），以及 shutdown drain、SSE keepalive、approval fallback poll、dispatcher idle
  timeout。所有 timeout 是正秒数，零值启动失败。所有 JSON request body 硬限制 64 KiB。

## 3. 健康检查与 readiness 语义

- `GET /health/live`：进程存活即 200。
- `GET /health/ready`：**Postgres 是核心依赖**——可达则 200，不可达则 503。NATS
  只影响 realtime capability：不可用时 readiness 仍 200，但响应体
  `realtime = "degraded"`；此时 SSE 返回稳定的 `503 realtime_unavailable`，而
  create/read/history/message/resume/cancel/approval 等核心 command 继续可用。

## 4. 优雅关闭

进程收到 shutdown 信号后：关闭 admission gate（新 durable work 返回安全稳定的
503）；state-aware middleware 立即 drop 尚未返回 response 的 handler future，使挂起的
Postgres/NATS/provider 请求释放 admission guard；结束 SSE 连接。Axum graceful server、
admission 与 process-owned Turn/dispatcher/SSE task set 共享一个配置化总 deadline，不为每个
阶段重新计时；deadline 到达后剩余 managed tasks 会 abort 并 join，任务不会 detached。

**shutdown 绝不转化为业务取消**：managed Turn 的 CancellationToken 在 shutdown 时
从不被 signal。超时未能完成的 Turn 在 Postgres 中保持 `running` 且不伪造终态，由
后续显式 resume 接管。

## 5. Resume / Cancel / Approval 运维行为

- **hosting 是易失观察**：进程内 exact `(AgentId, TurnId)` registry 永不持久化；
  进程重启后 registry 为空。`AgentView.resume_required` 是由该 registry 派生的
  advisory（durable `running` 且本机 unhosted），客户端据此提示显式 resume。
- **resume**：只接管 durable `running` 且本机 unhosted 的 exact Turn；恢复窗口 =
  压缩基线 + current-Turn 后缀 replay。snapshot/version 不兼容或依赖不可用时不猜测
  终态，保留 `running + unhosted`；必需 compaction companion/summary 损坏返回
  `durable_state_corrupt`（fail closed，不修表、无 rebuild API）。
- **cancel**：cancel signal 返回 202 仅代表"取消请求已发送"，不代表已 cancelled；
  终态以 durable event / AgentView 为准。cancel intent 在进程崩溃时可能丢失（已知
  限制，durable cancel 属于延期的 scheduler change）。
- **approval**：完全从 durable ledger 派生（Requested/Resolved 是 durable facts，
  Consumed 由 Hook journal 推导，Invalidated 由 Turn terminal 推导）。resolve 与
  resume 是分离的 endpoint：unhosted Agent 的 resolve 只持久化决定，不隐式 resume；
  相同决定重复 resolve 返回 204，相反决定返回 409。浏览器刷新后 pending approvals
  从 Postgres 恢复。

## 6. NATS 短 tail 与 cursor 语义

NATS（JetStream）只承载 Agent-scoped 的短期实时 tail：有限 age/bytes/messages 上限、
discard-old，**不是** durable history，也不保证跨重启补发；恢复真相永远是 Postgres
ledger。持久化顺序固定为 PG commit 先于 NATS publish；publish 丢失只记录一次安全
错误，由 PG snapshot/history 收敛。

SSE cursor（SSE `id` / `Last-Event-ID` / `after_cursor`）绑定 AgentId、JetStream stream
creation generation 与 stream sequence，是不透明 NATS 位置，不得与 `event_seq` 比较或
持久化为业务状态：

- cursor 仍在 retention 内：从其后继续 tail；
- cursor 已被淘汰：建流前返回 **410 `cursor_expired`**，客户端必须无 cursor cold
  bootstrap；
- cursor 属于另一 Agent 或旧 stream generation：同样在建流前返回 410，绝不把全局
  stream sequence 静默套到当前 Agent；
- 建流后服务端有界 buffer 溢出：发送无 SSE id 的
  `stream_reset { reason: "buffer_overflow" }` 后主动关闭连接，客户端丢弃 buffer、
  draft 与 cursor 重新 cold bootstrap；
- 无 cursor：从当前 tail 起点开始，不从 NATS history 起点开始。

## 7. Template 热读规则

`templates_root` 下的 TOML template 在**每次 catalog 读取与每次 Agent 创建时**热读
并全量校验（all-or-nothing：任一模板非法则 `GET /v1/agent-templates` 整体 422）。
磁盘整改只影响**之后创建**的 Agent；既有 Agent 的定义在创建时已固化进
`agents.resolved_definition`，永不重读模板。

## 8. 测试

- 单元测试（无需容器）：`cargo test -p <crate> --all-targets`；容器集成测试默认
  `#[ignore]`，普通 workspace 测试不依赖容器。
- 每个 crate 的集成测试经 crate 内 `Makefile` 运行，默认 `podman compose`（需 Docker
  时用 `COMPOSE="docker compose"` 覆盖），各自独立的 `docker-compose.test.yml` 与
  端口：
  - `make -C crates/stratum-postgres test-integration`（Postgres 17，端口 45432）；
  - `make -C crates/stratum-infra test-integration`（NATS `-js`）；
  - `make -C crates/stratum-api test-integration`（Postgres 45433 + NATS 44228，
    `tests/api.rs` 以 `--test-threads=1` 运行）。
- 细节见 `crates/stratum-api/TESTING.md` 与各 crate `AGENTS.md` 的测试章节。

## 9. 明确延期（本 runtime 不提供）

以下能力已确认需要但**明确延期**为独立 change，当前不要依赖：

- **scheduler**：durable scheduling、ownership lease/fencing、多实例
  hosting/takeover、rolling deployment、自动 resume、durable cancel、Agent/Workflow
  Session 协调。当前 hosting 判定是 process-local 的，因此**不声明任何多实例部署
  保证**；引入第二实例前必须完成 scheduler change。届时 `resume_required` 的判定
  来源由 scheduler ownership/placement 替换，API 字段保留。
- **template 管理**：正式 template 版本、catalog 管理与 Agent 列表
  （`GET /v1/agents`）。当前只有只读 `templates_root` 热读 catalog。
