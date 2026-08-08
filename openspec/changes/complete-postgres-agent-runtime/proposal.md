## Why

当前 Agent 执行链同时依赖 filesystem、旧 `AgentStore`、Postgres 初版表、Session-scoped NATS 和进程内状态，导致创建、恢复、审批、取消、历史与前端流式视图没有共同真相。现在需要把 beta 架构一次性收敛为 Postgres-first 的完整运行时，并彻底删除旧 filesystem execution 路径，避免继续为两套不兼容语义付出成本。

## What Changes

- **BREAKING** Postgres 成为 Agent 定义、current-Turn 状态和 durable event ledger 的唯一执行真相；最终 schema 只保留 `agents`、薄 `agent_state`、`durable_events` 与 `transcript_compactions` 四张核心表。
- **BREAKING** 删除 `stratum-store`、`AgentStore`、filesystem Agent state/history/durable/checkpoint、`agent_messages`、`tool_approvals`、`session_operation_claims`、旧 beta migration 与所有 backend selector/fallback；数据库和 sqlx migration history 必须整体重建，不迁移 beta 执行数据。
- **BREAKING** 删除 `stratum-agent-builtin`、旧 Session-scoped `EventStreamBus`、`ScopedAgentEventSink`、`StreamEnvelope`、旧 `AgentEvent`、`message_seq` 和公开 Session SSE；保留 kernel 的 `DurableEventSink`、`TelemetryEventSink` 及 durable approval facts。
- 创建 Agent 时从只读 template catalog 热读并校验最新 TOML，将 prompt、tools、creation-time effective model 与解析后的版本身份固化到 immutable Agent snapshot；既有 Agent 永不重新读取模板。创建要求客户端 UUID `Idempotency-Key`，相同 key 与相同请求永久返回同一 Agent。
- 使用 Agent-wide、无空洞的 `event_seq` 线性化 kernel、approval resolver、compaction 与 terminal durable 写入；event row 使用显式版本和 variant-only JSON payload，runtime snapshot 只附着在 `LoopStarted` row 上。
- Agent 状态只描述 current/recent Turn 的 `idle | running | finished | failed | cancelled`；hosting、`resume_required`、pending approval、latest usage 和 outcome 不复制到 state。terminal 状态不终结 Agent，后续消息可以用 exact current-Turn CAS 开始新 Turn。
- 审批完全由 durable ledger 派生：Requested/Resolved 是 durable facts，Consumed 由匹配的 `HookInvocationCompleted` 推导，Invalidated 由 Turn terminal 推导；resolve 与 resume 分离，刷新后仍能从 Postgres 恢复待审批 UI。
- Tool 完成事实统一为 `MessageAppended(role=tool, tool_call_id=CallId)`；不增加 `ToolExecutionCompleted`，恢复仅重试尚无 tool result message 的有序后缀。
- 固定 durable-safe payload 边界：已接受的 user text 作为对话级敏感数据原样持久化，但 runtime/provider/tool credential value 永不进入 definition、snapshot、event、NATS 或日志；Tool 参数中的凭据只能是 opaque reference，执行时再注入，Tool result 必须经 result Hook 脱敏后才能 durable append。
- 新增纯持久化 Agent create、AgentView、exact-Turn message/resume/cancel/approval、模型与模板目录、固定屏障 history 分页及 Agent-scoped SSE 合同；请求、成功码和类型化错误码固定，model override 是完整替换。
- Postgres commit 永远先于 NATS；NATS 只保留短期、可丢失的 Agent tail 和 LLM delta。前端以 PG snapshot barrier 冷恢复、增量 reconcile、按需向上分页，并在 NATS 不可用时降级为 PG 读取。
- 压缩以原子 `TranscriptCompacted + transcript_compactions` 记录保存单一 summary 和 retained frontier；原始 durable messages 永久保留，前端以可折叠 marker 展示压缩信息。
- **BREAKING** 最小修订 `CONSTITUTION.md` §1、§5 与 §8：取消强制 `stratum-store` 合同层、`stratum-agent-builtin` 装配层和 generic EventBus 要求，由 concrete `stratum-postgres` 承担执行存储接口，由 `stratum-infra` 提供窄 concrete Agent-tail transport；§5 同时明确“原样 user-authored conversation”与“永不持久化 runtime-managed credential value”的边界；业务 crate 仍禁止直连 `sqlx`/`async-nats`；Postgres 决定核心 readiness，NATS 故障只标记 realtime degraded，不禁用核心 HTTP command。
- 本 change 明确取代 `add-postgres-execution-storage`；旧 change 移入 archive 并标记 superseded，不再同步其 delta specs。
- 当前 `/chat` runtime 只使用 process-local registry，但本 change 不定义“单实例部署”合同，也不提前设计跨进程 ownership。lease/fencing、多实例 hosting、rolling deployment、自动 takeover、durable cancel、Agent/Workflow Session 协调及 `resume_required` 的 scheduler 判定来源，统一延期为独立 scheduler PATCH/TODO。

## Capabilities

### New Capabilities

- `postgres-agent-runtime-storage`: 四表 Postgres execution schema、immutable Agent snapshot、薄状态、Agent-wide durable ledger、版本与事务约束，以及旧存储层的彻底删除。
- `agent-runtime-api`: Template/Model catalog、Agent create/read、Turn admission、history、resume、cancel、approval、SSE、错误和 Web 冷恢复的端到端 HTTP 合同。
- `durable-tool-approval`: 无 projection table 的 durable approval ledger、幂等 resolve、waiter 唤醒、刷新恢复与 terminal invalidation。

### Modified Capabilities

- `agent-loop-resume`: 从 Postgres Agent-wide ledger 与压缩 summary companion 恢复 exact Turn，并固定 tool-result 对账、started-only 与 preflight 失败语义。
- `runtime-event-protocol`: 用 API-owned Agent stream frame、durable `event_seq` 与 call-local `telemetry_seq` 取代旧 Session envelope、EventBus 和 `message_seq`。
- `context-compaction`: 用原子 Postgres summary companion 取代 filesystem index，同时永久保留原始历史并公开 typed compaction marker。
- `session-runtime-identity`: 固定首 Turn Session 绑定、current-Turn CAS、terminal 后复用 Agent、模型默认值更新与当前版本的 Agent-only Session 单活约束。
- `agent-hook-runtime`: 审批 Handler 从 durable ledger 复用 Requested/Resolved facts，同时保持 Hook journal 为 kernel decision truth。

## Impact

- 主要影响 `stratum-api`、`stratum-postgres`、`stratum-agent`、`stratum-core`、`stratum-infra`、`stratum-filesystem`、`stratum-config`、`stratum-web`、workspace 配置、Docker、OpenAPI 和运行文档。
- 删除整个 `stratum-store` 与 `stratum-agent-builtin` crate；`stratum-filesystem` 只保留 sandbox/template/Agent 文件能力，并删除 CAS/record 家族。
- kernel 改动保持克制：不引入 Postgres、Session、hosting、pagination 或 scheduler；保留现有 sink 和 Tool 串行语义，只允许恢复复用所必需的纯 replay seam，以及删除已经失效的 transport DTO。
- 部署是破坏性 beta cutover：删除旧 migration，建立单一最终 baseline，重建 Postgres 与旧 NATS stream；磁盘上的旧用户文件不由程序自动删除。
- 实现前必须先完成已批准的 Constitution、`ARCH.md`、`TODO.md` 与相关 `AGENTS.md` 所有权修订；实现后必须通过 PG/NATS、API、Web、OpenSpec strict validation 和完整 constitution review。
