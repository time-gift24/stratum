## Why

当前 Agent 执行链同时依赖 filesystem、旧 `AgentStore`、Postgres 初版表、Session-scoped NATS 和进程内状态，导致创建、恢复、审批、取消、历史与前端流式视图没有共同真相。现在需要把 beta 架构一次性收敛为 Postgres-first 的完整运行时，并彻底删除旧 filesystem execution 路径；同时必须把“不可变 Agent template 版本”与“长期运行实例”拆成两个独立身份，避免每创建一次对话就复制一份相同 template。

## What Changes

- **BREAKING** Postgres 成为 Agent template 版本、runtime state 与 durable event ledger 的唯一执行真相；最终 schema 只保留复数命名的 `agents`、`agent_states`、`durable_events` 与 `transcript_compactions` 四张核心表。
- **BREAKING** `agents` 只保存可复用、不可变的 template 历史版本：`id` 是 `AgentId`，`name` 标识 template，`version` 是 template 作者命名的字符串 tag，`(name, version)` 唯一；tag 大小写敏感、无排序语义，`definition_schema_version` 单独表示 `resolved_definition` 的编码版本。删除 `AgentVersionId`、`agent_version_id`、`source_template_name` 与 definition fingerprint。
- **BREAKING** `agent_states` 每行表示一个长期 runtime aggregate：`id` 是 `AgentRuntimeId`，`agent_id` 永久引用 `agents.id`；同一个 `AgentId` 可被多个互相隔离的 runtime 复用。`idempotency_key`、唯一可变的 `model_config`、Session/current Turn、生命周期状态与 durable high-water 全部由 `agent_states` 持有。
- filesystem template catalog 只表示当前最新源；每份 TOML 必须由 template 作者提供非空、受长度和控制字符约束的 `version` tag，runtime create request 不得指定或覆盖它。创建 runtime 时，系统热读并校验 TOML，将 prompt、按序 tools、template 默认模型与非敏感定义身份解析为 canonical `resolved_definition`：精确 `(name,version)` 不存在时创建新 `AgentId`；已存在且定义严格相同时复用原 `AgentId`；已存在但定义不同则返回 `409 agent_version_conflict`，绝不覆盖历史。不同 tag 即使定义相同也表示不同的用户命名版本并创建独立 row。既有 runtime 永久 pin 原 `AgentId`，不重读模板、不自动升级。
- 创建 runtime 要求客户端 UUID `Idempotency-Key`。key 由 `agent_states` 永久唯一持有；命中 key 必须在重读 template 或重新解释请求前直接返回原 `AgentRuntimeId`，未命中才在同一事务中复用或创建 exact `(name,version)` 的 `agents` row 并插入 idle `agent_states`。失败事务既不占用 key，也不留下孤立版本。
- **BREAKING** 所有运行态资源从 Agent identity 切换为 `AgentRuntimeId`：创建使用 `POST /v1/agent-runtimes`，view、message、history、events、resume、cancel 与 approval 使用 `/v1/agent-runtimes/{agent_runtime_id}/...`；`/v1/agents/{agent_id}` 只保留给 immutable template-version resource，本 change 不实现完整 template 管理 UI/API。
- 使用 AgentRuntime-wide、无空洞的 `event_seq` 线性化 kernel、approval resolver、compaction 与 terminal durable 写入；`durable_events` 和 `transcript_compactions` 都按 `AgentRuntimeId` 分区。同一 `AgentId` 下的多个 runtime 各自从 seq=1 开始，互不锁定、混流或共享恢复上下文。
- runtime snapshot 只在 `LoopStarted` row 上持久化，并以 `agent_id: AgentId` 固定不可变定义；resume 必须验证 snapshot、`agent_states.agent_id` 与加载的 `agents.id` 一致。kernel 删除 `AgentVersionId`，但仍不理解 `AgentRuntimeId`、Postgres、Session hosting、pagination 或 scheduler；runtime identity 由 API-owned sinks 与装配层绑定。
- Agent runtime 状态只描述 pinned `AgentId`、创建命令身份、唯一可变 `model_config`、current/recent Turn 的 `idle | running | finished | failed | cancelled` 与 durable high-water；hosting、`resume_required`、pending approval、latest usage 和 outcome 不复制到 state。terminal 状态不终结 runtime，后续消息可以用 exact current-Turn CAS 开始新 Turn。
- create 的 model override 只初始化 `agent_states.model_config`，未传时继承 template 默认值；后续 Turn override 完整替换同一字段，值未变时不写入。不保留 `creation_model_override` 或 `default_model_config`。
- 审批完全由 durable ledger 派生：Requested/Resolved 是 durable facts，Consumed 由匹配的 `HookInvocationCompleted` 推导，Invalidated 由 Turn terminal 推导；resolve 与 resume 分离，刷新后仍能从 Postgres 恢复待审批 UI。
- Tool 完成事实统一为 `MessageAppended(role=tool, tool_call_id=CallId)`；不增加 `ToolExecutionCompleted`，恢复仅重试尚无 tool result message 的有序后缀。
- 固定当前 closed composition 的 payload 边界：已接受的 user text 与 Echo 参数/结果都作为 user-authored conversation data 原样持久化，且所有 Echo result 仍先经过 `AfterToolCall`。本 change 不提供 credential-aware Tool、typed credential/reference 字段、安全 provider 或通用 secret scanner，也不声称 Echo 的 opaque JSON 已被通用脱敏；HTTP strict DTO 拒绝专用 credential 字段。未来 credential-aware Tool 必须由独立 PATCH 同时定义 opaque reference、批准消费后的安全注入与 fail-closed result transform，在该 PATCH 完成前不得注册到 runtime。
- 新增 `AgentRuntimeView`、exact-Turn message/resume/cancel/approval、模型与模板目录、固定屏障 history 分页及 AgentRuntime-scoped SSE 合同；请求、成功码和类型化错误码固定。
- Postgres commit 永远先于 NATS；NATS 只保留短期、可丢失的 AgentRuntime tail 和 LLM delta。dispatcher、subject、cursor、snapshot barrier、telemetry floor 与前端 reconcile 全部按 `AgentRuntimeId` 隔离；前端以 PG snapshot barrier 冷恢复、增量 reconcile、按需向上分页，并在 NATS 不可用时降级为 PG 读取。
- 压缩以原子 `TranscriptCompacted + transcript_compactions` 记录保存单一 summary 和 retained frontier；原始 durable messages 永久保留，前端以可折叠 marker 展示压缩信息。
- **BREAKING** 删除 `stratum-store`、`AgentStore`、filesystem Agent state/history/durable/checkpoint、`agent_messages`、`tool_approvals`、`session_operation_claims`、旧 beta migration 与所有 backend selector/fallback；数据库和 sqlx migration history 必须整体重建，不迁移 beta 执行数据。
- **BREAKING** 删除 `stratum-agent-builtin`、旧 Session-scoped `EventStreamBus`、`ScopedAgentEventSink`、`StreamEnvelope`、旧 `AgentEvent`、`message_seq` 和公开 Session SSE；保留 kernel 的 `DurableEventSink`、`TelemetryEventSink` 及 durable approval facts。
- **BREAKING** 最小修订 `CONSTITUTION.md` §1、§5 与 §8：取消强制 `stratum-store` 合同层、`stratum-agent-builtin` 装配层和 generic EventBus 要求，由 concrete `stratum-postgres` 承担执行存储接口，由 `stratum-infra` 提供窄 concrete AgentRuntime-tail transport；§5 同时明确“原样 user-authored conversation”与“永不持久化 runtime-managed credential value”的边界；业务 crate 仍禁止直连 `sqlx`/`async-nats`；Postgres 决定核心 readiness，NATS 故障只标记 realtime degraded，不禁用核心 HTTP command。
- 本 change 明确取代 `add-postgres-execution-storage`；旧 change 移入 archive 并标记 superseded，不再同步其 delta specs。
- 当前 `/chat` runtime 只使用 process-local registry，但本 change 不定义“单实例部署”合同，也不提前设计跨进程 ownership。lease/fencing、多实例 hosting、rolling deployment、自动 takeover、durable cancel、Agent/Workflow Session 协调及 `resume_required` 的 scheduler 判定来源，统一延期为独立 scheduler PATCH/TODO。

## Capabilities

### New Capabilities

- `postgres-agent-runtime-storage`: 四表 Postgres execution schema、user-authored string-tag immutable Agent template 版本、`AgentRuntimeId`-keyed 薄状态与 durable ledger、tag 冲突与事务约束，以及旧存储层的彻底删除。
- `agent-runtime-api`: Template/Model catalog、runtime create/read、Turn admission、history、resume、cancel、approval、SSE、错误和 Web 冷恢复的端到端 HTTP 合同；运行态资源统一使用 `AgentRuntimeId`。
- `durable-tool-approval`: 无 projection table 的 durable approval ledger、幂等 resolve、waiter 唤醒、刷新恢复与 terminal invalidation，并以 exact `AgentRuntimeId` 隔离并发 runtime。

### Modified Capabilities

- `agent-loop-resume`: 从 Postgres AgentRuntime-wide ledger 与压缩 summary companion 恢复 exact Turn，以 pinned `AgentId` 加载不可变定义，并固定 tool-result 对账、started-only 与 preflight 失败语义。
- `runtime-event-protocol`: 用 API-owned AgentRuntime stream frame、durable `event_seq` 与 call-local `telemetry_seq` 取代旧 Session envelope、EventBus 和 `message_seq`。
- `context-compaction`: 用 AgentRuntime-scoped 原子 Postgres summary companion 取代 filesystem index，同时永久保留原始历史并公开 typed compaction marker。
- `session-runtime-identity`: 固定 `AgentId` template 版本、`AgentRuntimeId` 长期运行实例、首 Turn Session 绑定、current-Turn CAS、terminal 后复用 runtime、单一 `model_config` 更新与当前版本的 runtime-only Session 单活约束。
- `agent-hook-runtime`: 审批 Handler 从 AgentRuntime-scoped durable ledger 复用 Requested/Resolved facts，同时保持 Hook journal 为 kernel decision truth。

## Impact

- 主要影响 `stratum-api`、`stratum-postgres`、`stratum-agent`、`stratum-core`、`stratum-infra`、`stratum-filesystem`、`stratum-config`、`stratum-web`、workspace 配置、Docker、OpenAPI 和运行文档。
- 已实现的 `agents` 与 `agent_state` 一对一所有权将被替换为 `agents 1 -> N agent_states`；template TOML schema、catalog DTO、migration、storage API、core identity、runtime snapshot、OpenAPI DTO、所有运行态 route、NATS/SSE subject 与 frame、Web create/recovery flow 及相关测试都必须按 string version tag 与 `AgentId`/`AgentRuntimeId` 分工重写。
- 删除整个 `stratum-store` 与 `stratum-agent-builtin` crate；`stratum-filesystem` 只保留 sandbox/template/Agent 文件能力，并删除 CAS/record 家族。
- kernel 改动保持克制：不引入 Postgres、`AgentRuntimeId`、Session hosting、pagination 或 scheduler；保留现有 sink 和 Tool 串行语义，只机械地以 `AgentId` 取代旧 `AgentVersionId` 来固定不可变定义，并保留恢复复用所必需的纯 replay seam。
- 部署是破坏性 beta cutover：删除旧 migration，建立单一最终 baseline，重建 Postgres 与旧 NATS stream；磁盘上的旧用户文件不由程序自动删除。
- 实现前必须先完成已批准的 Constitution、`ARCH.md`、`TODO.md` 与相关 `AGENTS.md` 所有权修订；实现后必须通过 PG/NATS、API、Web、OpenSpec strict validation 和完整 constitution review。
