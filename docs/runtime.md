# Stratum 运行时运维手册

> 面向部署与运维人员。架构原理见 [`ARCH.md`](../ARCH.md)，HTTP 合同以运行时
> `/api-docs/openapi.json`（utoipa 生成）为唯一权威，crate 级实现约定见各 crate 的
> `AGENTS.md`。本文对应 change `complete-postgres-agent-runtime` 的最终状态。

## 1. 存储模型

Postgres 是 Agent 执行事实的**唯一**持久化存储，没有 backend selector 或静默回退。
最终 schema 只有四张核心表（`stratum-postgres` crate 内嵌 sqlx migration，
`PostgresBackend::connect` 启动时自动应用 baseline）：

- `agents`：immutable Agent template 版本。每行由服务端 `AgentId` 标识，template 作者以
  大小写敏感且无排序语义的 `(name, version)` 字符串 tag 命名；resolved definition 只含
  prompt、按序 tools、template default model 与非敏感定义身份。相同版本可被多个 runtime
  复用，已有行永不覆盖。
- `agent_states`：每个 `AgentRuntimeId` 一行薄状态，通过 `agent_id` 永久 pin 一个
  `agents` row，并拥有创建幂等键、durable status（`idle/running/finished/failed/cancelled`）、
  绑定的 Session/current Turn、唯一可变 `model_config` 与 `last_event_seq` high-water。
  不保存 outcome、usage、snapshot、approval 或 hosting。
- `durable_events`：append-only ledger。`(agent_runtime_id, event_seq)` 主键；`event_seq` 是
  AgentRuntime-wide、无空洞的十进制序列，由 exact `agent_states` 行锁在集中 append 事务中分配。
  对外（API/history/SSE frame）一律编码为十进制字符串。payload 为 variant-only JSON，
  显式 `event_version`；runtime snapshot 只附着在 `LoopStarted` row。
- `transcript_compactions`：与 `TranscriptCompacted` discriminator 同事务写入的
  durable companion，只保存单一 typed summary、`upto`、`compacted_iteration`
  与 `retained_from_event_seq` 保留指针。原始 durable messages 永久保留，
  压缩不改写历史。

没有 projection 表：AgentRuntimeView、history 分页、pending approvals、latest usage 全部
从 ledger 派生读取。核心资产没有 delete API，外键一律 `RESTRICT`。

### 破坏性 beta cutover

本次切换**不做数据迁移**：旧 beta migration 已删除，部署时整体 drop 并重建数据库
（包括 `_sqlx_migrations` 表），并重建 NATS stream。回滚到旧 binary 必须同时恢复旧
schema/config/NATS，不能只回滚应用。

程序不会扫描、迁移、修改或删除磁盘上的旧 filesystem 执行数据（旧
`storage_root`/state/history/compact.jsonl 等）；这些文件的备份与清理由运维自行处理。

## 2. 配置

完整可注释样例见根目录 `config.example.toml`；配置解析严格拒绝未知字段。
Execution、Ontology 与 Studio 的三个 PostgreSQL URL 都必须显式写端口（例如 `:5432`），
不会从 `PGPORT` 补隐式连接身份；三者必须指向彼此隔离的 database。

- `[postgres].url`：必填，缺失或为空即启动失败（fail closed）。Postgres 不可用则
  启动失败、readiness 503。
- `[ontology].database_url`：必填，指向独立的 Ontology PostgreSQL database；连接或
  migration 失败同样使启动 fail closed。
- `[studio].database_url`：必填，指向与 execution ledger 隔离的 Studio PostgreSQL database。
  Provider、Model、credential 与 Agent definition 只从这里读取；缺失、连接失败、migration
  失败或 catalog 损坏都使启动 fail closed。空 database 合法且保持为空，不从配置、环境变量或
  template 文件 seed。
- `[studio].management_enabled`：只控制 Provider、Model 与 Agent definition 管理 routes。
  启用时 `[api].bind` 必须是 loopback；关闭时 runtime 仍连接并使用 Studio database。
- `[tools].workspace_root`：`shell` 与 `apply_patch` 共享的默认工作目录。启动时校验为已有
  目录，服务不自动创建；该目录不承载 AgentRuntime history、event 或 checkpoint。根
  Compose 镜像内的 `/workspace` 是不挂载宿主目录或 volume 的空白临时目录，容器重建后
  内容丢失；当前镜像也不把项目源码或完整开发工具链复制进去。
- `[nats]`：连接与 AgentRuntime-scoped 短 tail 上限——`url`、`stream_name`、
  `subject_prefix`、`replicas`（1..=5）、`max_age_seconds`、`max_bytes`、
  `max_messages`（有限上限 + discard-old）与 `connect_timeout_seconds`。
- `[api]`：`bind`（省略时固定 `127.0.0.1:8080`）与 `allowed_origins`（CORS，拒绝
  `*`），以及 shutdown drain、SSE keepalive 与 dispatcher idle timeout。approval 的
  fallback reread tick 是进程内固定上限，不是用户配置。所有配置化 timeout 是正秒数，
  零值启动失败。所有 JSON request body 硬限制 64 KiB。

旧 `[agent]` 与 `[llm]` 配置会被严格解析器作为未知字段拒绝。Provider endpoint 与四个出站
timeout 是闭集 adapter 的可信代码策略，不是部署配置或 Studio 可写资源；credential 与 Model
必须经 loopback Studio 管理 API 显式建立。

## 3. 健康检查与 readiness 语义

本地完整栈从 worktree 根目录启动：

```shell
podman compose up -d --build --wait
```

Compose 的一次性 `postgres-provision` 服务会在 API 启动前检查并创建缺失的
`stratum_ontology` 与 `stratum_studio` database。它对已有 database 不做修改，因此从旧
`postgres-data` volume 升级时会保留 execution ledger，只补齐缺失的独立 database；无需
`down -v`。`/docker-entrypoint-initdb.d` 脚本仍只负责全新 volume 的首次初始化。

默认暴露 Web `http://127.0.0.1:5173`、API `http://127.0.0.1:18080` 与 NATS
监控端口 `http://127.0.0.1:8222`；Postgres 只在 compose 网络内可达。样例
`config.docker.toml` 的 API 绑定 `0.0.0.0`，因此按安全合同关闭 management routes，但 runtime
仍只使用 `stratum_studio`。全新空库不会自动生成 Provider、Model 或 Agent definition；首次
provision 应使用绑定 loopback 的 API 进程连接同一 Studio database，经管理界面或 API 按
Provider → Model → Agent definition 建立资源。不要为容器端口转发放宽 loopback 校验。

- `GET /health/live`：进程存活即 200。
- `GET /health/ready`：execution、Ontology 与 Studio 三个 PostgreSQL database 都是核心依赖；
  任一不可达则 503。NATS 只影响 realtime capability：不可用时 readiness 仍 200，但响应体
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

- **hosting 是易失观察**：进程内 exact `(AgentRuntimeId, TurnId)` registry 永不持久化；
  进程重启后 registry 为空。`AgentRuntimeView.resume_required` 是由该 registry 派生的
  advisory（durable `running` 且本机 unhosted），客户端据此提示显式 resume。
- **resume**：只接管 durable `running` 且本机 unhosted 的 exact Turn；恢复窗口 =
  压缩基线 + current-Turn 后缀 replay。snapshot/version 不兼容或依赖不可用时不猜测
  终态，保留 `running + unhosted`；必需 compaction companion/summary 损坏返回
  `durable_state_corrupt`（fail closed，不修表、无 rebuild API）。
- **cancel**：cancel signal 返回 202 仅代表"取消请求已发送"，不代表已 cancelled；
  终态以 durable event / AgentRuntimeView 为准。cancel intent 在进程崩溃时可能丢失（已知
  限制，durable cancel 属于延期的 scheduler change）。
- **approval**：完全从 durable ledger 派生（Requested/Resolved 是 durable facts，
  Consumed 由 Hook journal 推导，Invalidated 由 Turn terminal 推导）。resolve 与
  resume 是分离的 endpoint：unhosted Agent 的 resolve 只持久化决定，不隐式 resume；
  相同决定重复 resolve 返回 204，相反决定返回 409。浏览器刷新后 pending approvals
  从 Postgres 恢复。

## 6. NATS 短 tail 与 cursor 语义

NATS（JetStream）只承载 AgentRuntime-scoped 的短期实时 tail：有限 age/bytes/messages 上限、
discard-old，**不是** durable history，也不保证跨重启补发；恢复真相永远是 Postgres
ledger。持久化顺序固定为 PG commit 先于 NATS publish；publish 丢失只记录一次安全
错误，由 PG snapshot/history 收敛。

SSE cursor（SSE `id` / `Last-Event-ID` / `after_cursor`）绑定 AgentRuntimeId、JetStream stream
creation generation 与 stream sequence，是不透明 NATS 位置，不得与 `event_seq` 比较或
持久化为业务状态：

- cursor 仍在 retention 内：从其后继续 tail；
- cursor 已被淘汰：建流前返回 **410 `cursor_expired`**，客户端必须无 cursor cold
  bootstrap；
- cursor 属于另一 AgentRuntime 或旧 stream generation：同样在建流前返回 410，绝不把
  全局 stream sequence 静默套到当前 runtime；
- 建流后服务端有界 buffer 溢出：发送无 SSE id 的
  `stream_reset { reason: "buffer_overflow" }` 后主动关闭连接，客户端丢弃 buffer、
  draft 与 cursor 重新 cold bootstrap；
- 无 cursor：从当前 tail 起点开始，不从 NATS history 起点开始。

## 7. Studio definition 与 immutable runtime 版本

`GET /v1/agent-templates` 是 Studio Agent definitions 的只读兼容投影，不读取本地文件。
definition 的 version tag 保持作者命名、原值比较、大小写敏感、UTF-8 `1..=128` bytes、无控制
字符与首尾空白，不做 trim、Unicode normalization、SemVer 解析或排序。Studio definition 的
管理 create/update 都必须显式提交 tag，update 必须使用不同的新 tag；下述 AgentRuntime create
request 不接收、选择或覆盖 version。

`POST /v1/agent-runtimes` 在 idempotency key 未命中时读取 Studio database 的当前 definition，
再把 exact `(name, version)` 物化到 execution ledger 的 immutable `agents` row：同 pair 与相同
canonical definition 复用 AgentId，同 pair 却改变 definition 返回
`409 agent_version_conflict`，不同 tag 即使 definition 相同也创建不同 AgentId。每次成功 create
都创建独立 AgentRuntimeId；既有 runtime 永久使用其 pinned definition，不受后续 Studio 更新或
删除影响。

启动不会 seed Agent definition；开发阶段通过 Studio 管理 API 或 UI 显式创建，并为 coding
Agent 选择 `shell` 与 `apply_patch`。既有 runtime 永远不受后续 definition 更新影响。

## 8. 测试

- 单元测试（无需容器）：`cargo test -p <crate> --all-targets`；容器集成测试默认
  `#[ignore]`，普通 workspace 测试不依赖容器。
- 每个 crate 的集成测试经 crate 内 `Makefile` 运行，默认 `podman compose`（需 Docker
  时用 `COMPOSE="docker compose"` 覆盖），各自独立的 `docker-compose.test.yml` 与
  端口：
  - `make -C crates/stratum-postgres test-integration`（Postgres 17，端口 45432）；
  - `make -C crates/stratum-infra test-integration`（NATS `-js`）；
  - `make -C crates/stratum-studio test-integration`（独立 Studio PostgreSQL，覆盖事务回滚、
    Provider cascade/blocker、version CHECK 与损坏 credential fail-closed）；
  - `make -C crates/stratum-api test-integration`（execution、Ontology、Studio 三个独立
    PostgreSQL database + NATS 动态发布 loopback host ports 并注入测试进程，`tests/api.rs`
    以 `--test-threads=1` 运行；手动
    `test-up` 默认仍为 45433 / 44228）。
- 细节见 `crates/stratum-api/TESTING.md` 与各 crate `AGENTS.md` 的测试章节。

## 9. 明确延期（本 runtime 不提供）

以下能力已确认需要但**明确延期**为独立 change，当前不要依赖：

- **scheduler**：durable scheduling、ownership lease/fencing、多实例
  hosting/takeover、rolling deployment、自动 resume、durable cancel、Agent/Workflow
  Session 协调。当前 hosting 判定是 process-local 的，因此**不声明任何多实例部署
  保证**；引入第二实例前必须完成 scheduler change。届时 `resume_required` 的判定
  来源由 scheduler ownership/placement 替换，API 字段保留。
- **发布与升级管理**：immutable Agent version 历史浏览、显式发布/提升/回滚与既有
  AgentRuntime upgrade。当前 Studio 只管理未来 runtime 使用的可变 Agent definition；创建时
  自动物化/复用 immutable execution version，历史 runtime 永不热升级。
