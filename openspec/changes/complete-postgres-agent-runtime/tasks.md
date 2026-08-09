## 1. 宪法与领域前置工作

- [x] 1.1 实现前修订 `CONSTITUTION.md` 的技术栈说明、§1、crate DAG与相关red flags：删除 `stratum-store` 与 `stratum-agent-builtin`，将 `stratum-postgres` 定为由装配层 `stratum-api` 调用的具体执行存储边界，并保留kernel/`stratum-agent`不得依赖Postgres、HTTP、hosting或分页的规则
- [x] 1.2 实现前修订`CONSTITUTION.md` §5：删除对`stratum-store`/`stratum-agent-builtin`的强制所有权与装配要求，并把generic EventBus强制替换为`stratum-infra`窄concrete Agent-tail boundary；以Postgres durable events作为唯一执行真相，允许装配层`stratum-api`调用具体Postgres command/query接口，kernel持久化仍通过`DurableEventSink`，telemetry仍通过`TelemetryEventSink`；继续禁止业务crate直连`sqlx`/`async-nats`，禁止filesystem执行持久化，同时保留`stratum-filesystem`模板读取与Agent可见业务文件操作；明确已接受的opaque user text按对话敏感数据原样持久化，但runtime-managed secret/token/credential value只能通过安全reference在执行时注入，永不进入持久流
- [x] 1.3 实现前修订 `CONSTITUTION.md` §8：将 Postgres 设为核心 readiness 依赖；NATS 故障只把实时能力标记为 degraded，不禁用 Postgres-backed commands；保留停止新 admission 且不把进程 shutdown 转化为业务 cancellation 的 shutdown 语义
- [x] 1.4 更新 `ARCH.md` 与 `CONTEXT.md`，记录最终 Agent/Session/Turn 词汇、四表所有权模型、agent-wide event sequence、exact-Turn process registry、compaction summary companion，以及 Postgres-before-NATS durability boundary
- [x] 1.5 在 `TODO.md` 中分别记录并明确延期durable scheduling/lease/fencing/multi-instance ownership/rolling deployment/automatic takeover-resume/durable cancel/Agent-Workflow coordination，以及未来Agent-template management；scheduler条目必须明确未来由ownership/placement替换`resume_required`的process-local判定来源，本change不实现这些patch
- [x] 1.6 更新受影响的根目录与各 crate `AGENTS.md`，删除陈旧的 `stratum-store`、filesystem execution、filesystem CAS、旧 bus 与旧 sequence 指引，并记录最终具体 Postgres、template reader、kernel restraint 与测试合同
- [x] 1.7 在修改实现代码前审查前置文档 diff 并取得一致；允许已批准的 storage-agnostic prepared-resume seam，以及删除旧 `StreamEnvelope`、`RuntimeEvent`、`AgentEvent` 等 transport DTO，除此之外的 kernel 行为改动都必须停止并讨论

## 2. 退役已被取代的 Change

- [x] 2.1 将 `add-postgres-execution-storage` 作为历史工作归档，不把其中过时的双后端或三表 delta 同步到 main specs
- [x] 2.2 验证已归档 change 不能再作为目标架构被选择、同步或应用，并确认 `complete-postgres-agent-runtime` 是本次切换唯一活跃的实现 change

## 3. 破坏性最终数据库基线

- [x] 3.1 删除已提交的 beta execution migration，以单一最终 baseline migration 替代；要求已有 beta 数据库连同 `_sqlx_migrations` 一并删除重建，不做原地升级
- [x] 3.2 仅创建 `agents`、`agent_state`、`durable_events` 与 `transcript_compactions`；不得创建 Session 表、Session-operation claim 表、message projection、approval projection、outbox 或 rebuild metadata
- [x] 3.3 定义 `agents`：包含不可变 `agent_id`、一对一 unique `agent_version_id`、来源 template 名、nullable creation model override、definition schema version、resolved definition、永久 client idempotency key 与创建时间；不得保存 raw TOML、template path/digest 或 credentials
- [x] 3.4 定义精简 `agent_state`：仅包含 Agent identity、durable status、可选的已绑定 Session/current Turn、mutable default model config、last event high-water 与更新时间；不得增加 runtime snapshot、outcome、usage、hosting、resume、approval 或 claim 字段
- [x] 3.5 定义 `durable_events`：包含 `(agent_id,event_seq)` 主键、Session/Turn identity、受检查约束的 event type、event version、仅含变体内容的 JSON payload、仅 LoopStarted 可用的可选 runtime snapshot/version，以及创建时间
- [x] 3.6 定义 `transcript_compactions`：包含关联durable event identity、Turn、compacted iteration、`upto`、`retained_from_event_seq`、唯一完整summary与创建时间；对应TranscriptCompacted durable payload固定为空对象，不复制任何companion字段，也不得保存messages副本、digest或filesystem line coordinate
- [x] 3.7 增加 lifecycle、payload-version、LoopStarted snapshot、exact-Turn、唯一 LoopStarted/terminal/approval-event、foreign-key、overflow 与 compaction-pointer 约束，使畸形 durable state fail closed
- [x] 3.8 增加同一 Session 仅一个 `running` Agent 的 partial unique index、durable Turn/event indexes、filtered history index、approval-ledger lookup indexes 与 latest-companion index，不引入跨 runtime claim abstraction
- [x] 3.9 增加 ignored real-Postgres baseline 测试：在空数据库应用 migration，并证明所有禁止的 beta 表与字段均不存在

## 4. 具体 Postgres Runtime、API 类型与错误

- [x] 4.1 定义窄领域 newtype 与 enum，覆盖 Agent status、schema/event/protocol version、history cursor、approval identity/decision 和对外 decimal-string sequence，且不依赖已删除的 store contract
- [x] 4.2 在 `stratum-postgres` 中实现窄而具体的 execution command/query 接口；保持单实现且不引入 trait，只暴露 `stratum-api` 装配/运行时编排与 durable sink adapter 所需操作
- [x] 4.3 为 resolved definition、durable event payload、runtime snapshot 与 API stream frame 实现严格 v1 解码；durable typed shape须canonical round-trip相等，approval store扩展走exact deny-unknown wire DTO；不支持的已声明版本返回 runtime-incompatible，受支持版本的未知字段/非canonical或畸形数据返回 durable-state-corrupt，不引入 upcaster framework
- [x] 4.4 仅在 LoopStarted durable row 上持久化和读取现有六字段 Turn runtime snapshot：Agent version、effective model、Tool-set fingerprint、Skill-set version、Extension-set version 与有序 Hook-handler versions
- [x] 4.5 从`agents`、`agent_state`与`durable_events`实现Agent cold read：令`snapshot_event_seq`直接等于同一MVCC snapshot中的`agent_state.last_event_seq`且不另存第二个cursor，从exact local process registry派生`resume_required`，从ledger events派生pending approvals，从当前Turn派生latest usage；不得返回或持久化outcome
- [x] 4.6 直接在过滤后的 `durable_events` 上为 MessageAppended、TranscriptCompacted、安全的 LoopFailed 与 LoopCancelled 实现 history pagination，支持固定 through barrier、exclusive before cursor、升序响应、默认/最大 limit 与 1 MiB soft page budget
- [x] 4.7 在独立 `error.rs` 中使用 `thiserror::Error` 定义 library errors，保留 source chain 与安全消息，并明确区分 stale/busy/hosting/runtime/version/corruption/storage/approval/preamble failures
- [x] 4.8 在 HTTP 边界把 typed errors 映射为稳定、安全的 envelope 与约定的 400/404/409/410/413/422/500/503 状态，不暴露 SQL、path、provider detail、prompt、Tool input/result 或 credential
- [x] 4.9 增加聚焦的 store/query/error 测试，覆盖严格解码、exact state/event 一致性、派生 AgentView 字段、直接 ledger history pagination、过滤后的 sequence gap、terminal marker 与安全 error mapping

## 5. 只读模板与幂等 Agent 创建

- [x] 5.1 用严格的 `agent.templates_root` 替换 `agent.storage_root`，并让其直接指向只读 template 目录；删除兼容 alias、`/templates` 后缀拼接、目录创建、Agent/history 子目录与 execution-root 语义
- [x] 5.2 启动时校验 `templates_root`：路径缺失、不是目录或不可读时启动失败；允许可读空目录，且不得创建 execution 目录
- [x] 5.3 对每个新的 create key 热解析 template，解析并校验 prompt、有序 tools、default model、provider 与 version identities；不得持久化 raw TOML，也不得为已有 Agent 重新读取 template
- [x] 5.4 实现严格 all-or-nothing 的 `GET /v1/agent-templates` catalog 读取，只暴露安全 catalog 字段，任一无效 `*.toml` 都使请求失败；不得实现 template CRUD 或延期的 template-management module
- [x] 5.5 保留 `GET /v1/models`，作为 create 与后续完整 model override 使用的已校验 model catalog
- [x] 5.6 将`POST /v1/agents`实现为纯创建command：要求client生成UUID `Idempotency-Key`，只接受template名与可选完整model override，返回`201 Created`、`Location`和包含AgentId的稳定response body，不得创建Session、Turn、runtime task或event
- [x] 5.7 永久保证 create 幂等：相同 key 与等价请求返回原 Agent 和相同成功语义；相同 key 携带不同 template 或 model override 返回 `409 idempotency_key_conflict`；失败请求不占用 key；并发重复请求通过数据库约束收敛
- [x] 5.8 原子持久化包含prompt、tools与creation-time effective model的immutable resolved Agent definition和idle state，以同一effective creation model初始化mutable default model，并证明template编辑或删除只影响之后创建的Agent
- [x] 5.9 增加 template/config/create 测试，覆盖启动校验、all-or-nothing catalog error、model/tool preflight、immutable snapshot、不同 key 从同一 template 创建、丢失响应后的重试、key 冲突、并发 key 与不存在 filesystem write

## 6. Turn Admission、Durable Append、状态与取消

- [x] 6.1 实现 exact `(AgentId,TurnId)` process registry，包含唯一 local claim identity、`starting`/`running` 状态、LoopStarted 前安装 CancellationToken、bounded managed futures 与按 claim 比较清理；不得持久化任何 hosting state
- [x] 6.2 实现 `POST /v1/agents/{agent_id}/messages`，接受非空且原样保留的 text、必填且可为 null 的 `expected_current_turn_id`、可选首个 Session、可选完整 model replacement，并执行约定的 64 KiB JSON request limit
- [x] 6.3 将首个已接受 Turn 绑定到调用方提供且已校验的 SessionId，或 server 生成的 SessionId；后续 Turn 永久复用该 Session 并拒绝不一致值；通过 partial unique index 强制当前仅 Agent 范围的 Session single-active，且不创建 Session record
- [x] 6.4 在任何 durable mutation 前完成 provider/model/parameter/Tool/runtime preflight，在 Agent state lock 下比较 `expected_current_turn_id`，并以稳定 stale/busy/session error 拒绝请求，不能打开第二个 Turn
- [x] 6.5 保留kernel的两个durability boundary：LoopStarted原子安装新Turn与versioned runtime snapshot，首条user MessageAppended随后独立提交；只有managed task与第二个commit barrier均存在时才返回`202 Accepted`和包含AgentId/SessionId/TurnId的稳定response body
- [x] 6.6 将 Turn 历史 base 推导为 `LoopStarted.event_seq - 1`；不得在 Agent state 或 runtime snapshot 中持久化 `base_event_seq`
- [x] 6.7 实现集中 append transaction：锁定 `agent_state`，校验 exact Agent/Session/current Turn/status，分配连续 agent-wide `event_seq`，插入 versioned durable row，仅应用该 event 所需的 state 或 compaction 变更，并在同一 commit 中推进 high-water
- [x] 6.8 仅在 Postgres commit 后返回内部 commit receipt，并把它适配到保持不变的 kernel `DurableEventSink::append -> Result<(), _>` acknowledgement
- [x] 6.9 只在首条 user MessageAppended transaction 中、且 effective full replacement 与当前值不同时更新 mutable default model；started-only Turn 与 no-op override 都不得重写它
- [x] 6.10 LoopFinished/LoopFailed/LoopCancelled 只更新精简 Agent state，保留 Session/current Turn，并让历史 runtime snapshot 留在 LoopStarted；阻止第二个 terminal event，且不得持久化 outcome 或 usage cache
- [x] 6.11 将exact-Turn cancel实现为仅process-local signal：running future接受token后返回空body的202；同一已cancelled Turn返回空body的204；区分starting/unhosted/stale/not-running typed result；不得abort/drop，也不得持久化cancel intent
- [x] 6.12 增加 admission/append/control 测试，覆盖首次/后续 Session binding、同 Session 冲突、双 tab 与丢失 202 后的 CAS、并发 durable writer、回滚不产生 sequence gap、model no-op/update 时机、terminal 后新 Turn、cancel race 与 stale registry cleanup

## 7. Resume、Compaction 与 Tool Continuation

- [x] 7.1 为 exact resume 捕获不可变 base 与固定 through frontier，校验 state/Agent/current Turn，并要求完整连续的 current-Turn durable slice，同时不向 kernel 暴露 event sequence
- [x] 7.2 在派生 base 以内选择最新且结构有效的 compaction summary companion，物化 `[summary] + MessageAppended[retained_from_event_seq..base]`，再通过固定 barrier 单独 replay current Turn
- [x] 7.3 原子写入 TranscriptCompacted discriminator 及其单 summary companion，推导 non-null `retained_from_event_seq`，仅在结构校验通过后使用 pointer；缺少必需 companion/summary 必须视为 durable corruption，不能当作普通 checkpoint miss
- [x] 7.4 仅在不存在 compaction，或其他内容完整的 companion 只有 acceleration pointer 无效时，执行纯内存 full replay：关联每个必需 summary companion，并按序应用 MessageAppended、TranscriptCompacted 与历史 terminal control；绝不重写 row、执行 projection rebuild 或复活已压缩前缀
- [x] 7.5 只增加已批准的纯 kernel seam：exact `Arc<AgentLoop>::prepare_resume` 返回绑定到该 runtime、不可 Clone/Serialize 的 opaque prepared value，并只暴露一次 consuming run path；复用现有 replay validator，不改变其他 kernel 行为
- [x] 7.6 为exact durable running且当前进程unhosted的Turn实现`POST /v1/agents/{agent_id}/resume`：新建managed execution返回`202 Accepted`和包含AgentId/SessionId/TurnId的稳定response body，已有local claim返回空body的`204 No Content`；不得创建新的LoopStarted、Session、Turn或model override
- [x] 7.7 通过原子追加安全 LoopFailed 协调 started-only Turn，并返回 `turn_preamble_incomplete`；commit 结果不确定时重读 exact Turn，绝不猜测 terminal state
- [x] 7.8 Tool completion仅由result hook之后最终的`MessageAppended(role=tool, tool_call_id=CallId)`表示；raw output必须先变换为durable-safe result，无法移除typed secret时只提交安全结构化error；不得增加ToolExecutionCompleted、AttemptId、runtime dedupe或Tool idempotency contract
- [x] 7.9 以至少一次语义恢复 Tool execution：已提交 Tool message 永不重试；有 ToolExecutionStarted 但无 result 时可使用相同逻辑 CallId 重试；外部服务自行负责幂等
- [x] 7.10 只为后续 Turn 的 model context 规范化 failed/cancelled 历史末尾未闭合 Tool group，保留已提交 result prefix 与不可变 durable/history row；current running Turn 继续正常 continuation
- [x] 7.11 增加 recovery 测试，覆盖 companion fast path、pointer-only fallback、缺少必需 summary 时的 corruption、多次 compaction、固定 barrier、started-only 不确定提交、runtime incompatibility/corruption、exact prepared-runtime binding、部分 Tool prefix、未知 Tool outcome、历史规范化与字节级一致的 context reconstruction

## 8. 从 Ledger 派生的 Durable Approval

- [x] 8.1 只在Hook Pending boundary之后的versioned ToolApprovalRequested/Resolved durable events中持久化稳定ApprovalId、exact HookInvocationId、CallId、最终durable-safe Tool name/arguments、非敏感authorization identity/reference与decision；真实credential value必须在approval消费后从安全provider注入且不得落库
- [x] 8.2 仅从 ledger 派生 approval state：Requested 来自 ToolApprovalRequested，Resolved 来自 ToolApprovalResolved，Consumed 来自匹配的 HookInvocationCompleted，Invalidated 来自 exact Turn terminal event
- [x] 8.3 强制 request/resolve identity 唯一，并用共同的 `agent_state` lock 线性化 resolver 与 terminal writer；不得创建或锁定 approval projection row
- [x] 8.4 实现幂等 resolve：首个 decision 胜出；相同重试不追加第二个 event 并返回 204；冲突重试返回 `approval_already_resolved`；terminal 后返回 `approval_invalidated`；unhosted running Turn 可以 resolve，但不得隐式 resume
- [x] 8.5 将 `decide_tool_call` Handler 实现为普通 Hook handler：复用 ledger Requested/Resolved events，将 durable decision 映射为 Execute/Block，并让 AgentLoop 不感知 approval 是独立概念
- [x] 8.6 实现 register-then-read 的 process-local waiter 与 commit-before-notify resolution，使提前、并发、丢失或重复 notification 都不会丢失或重复 durable decision
- [x] 8.7 从 current-Turn Requested events 中派生 AgentView 的 `pending_approvals`，排除已有 Resolved、matching HookInvocationCompleted 或 terminal event 的记录，并按 requested event sequence 排序；refresh 与 resume 永远不得依赖 NATS 作为 approval truth
- [x] 8.8 保持 durable ordering：HookPending < ApprovalRequested < ApprovalResolved < HookCompleted < ToolStarted/blocked result，并让 Hook journal events 仅存在于 PG
- [x] 8.9 增加 approval 测试，覆盖重复 request identity、相同/冲突 resolve、terminal race、等待中 cancellation、notification 丢失、refresh、unhosted resolve 后 resume、consumed/invalidated 派生、多个顺序 approval，以及不依赖 `tool_approvals` 表

## 9. NATS Tail、SSE 与 Web 恢复

- [x] 9.1 定义 API-owned `AgentStreamFrameV1` control/durable/telemetry variants；control封闭为`stream_ready`与`stream_reset { reason: buffer_overflow }`，其余包含protocol version、Agent identity、可选Session/Turn identity、decimal-string durable event sequence、telemetry的decimal-string `durable_before_event_seq` ordering watermark与call-local telemetry identity；不得因transport需求修改kernel event type
- [x] 9.2 配置 Agent-scoped NATS tail，使用明确且可配置的短 age/byte/message 上限与 discard-old retention；不得编码固定历史保留保证，也不得把 NATS 用作 durable history
- [x] 9.3 实现 volatile per-Agent ordered publisher：从启动 high-water 初始化，在收到 receipt 后扫描已提交 PG row，使 product event 按 event-sequence 顺序发布；满队列的 durable wake 合并进单调 high-water，coalesced flush 先 snapshot target、再确认 accepted queue 为空且只 flush 旧 snapshot，之后推进的 target 留给下一次 drain/idle 循环并在 idle 退休前追平，不以 realtime 背压阻塞PG acknowledgement；跳过仅 PG 可见的 Hook row、`ToolExecutionStarted`与其他internal fact，不引入outbox或跨重启backlog
- [x] 9.4 在 bounded queue 前为 API `TelemetryEventSink` 的 LlmStarted/delta/LlmFinished 分配 call-local `(llm_call_id,telemetry_seq)`并冻结当时PG high-water；frame以`durable_before_event_seq`公开该ordering watermark，保留可检测 gap但不改变telemetry identity或分配durable sequence
- [x] 9.5 实现 Agent SSE current-tail 与 retained-cursor mode；只有 subscription 生效后才发送 `stream_ready`；transport cursor 过期在建流前返回410；建流后的bounded-buffer overflow直接发送不带SSE id且不进入NATS/PG的`stream_reset(reason=buffer_overflow)`并关闭连接；删除public Session SSE与legacy replay parameter
- [x] 9.6 更新 Web client：NATS cursor 只保存在当前页面内存；等待 `stream_ready`；cold bootstrap 期间 buffer；通过 `snapshot_event_seq` 读取 AgentView/history，并用 barrier-governed `telemetry_floor_event_seq` 初始化已收敛 assistant final floor而非只靠最新history page；只应用 barrier 之后的 buffered durable event；丢弃全部 cold-buffer telemetry；收到`stream_reset`时主动关闭原EventSource并丢弃buffer、draft与cursor，禁止携旧Last-Event-ID自动重连，改为无cursor cold bootstrap
- [x] 9.7 实现 Web reducer：以 `(agent_id,event_seq)` 作为 durable identity，以 `(llm_call_id,telemetry_seq)` 作为 transient draft identity，支持 gap detection、final assistant replacement与late-delta suppression；cold AgentView 以ledger派生的`telemetry_floor_event_seq`覆盖最新history页外的final，PG reconcile先应用assistant final时以`durable_before_event_seq`拒绝final前旧tail但保留final后新call；accepted Turn优先、否则running current Turn的exact identity fence拒绝跨Turn backlog，并在每一种terminal state下清理draft与interrupted Tool
- [x] 9.8 首次只加载最新 history page，仅在确实需要向上滚动时请求 exclusive-before page，保持升序渲染，并展示 TranscriptCompacted summary 与安全 failed/cancelled marker，不删除原始 message
- [x] 9.9 refresh 后从 PG 恢复 pending approvals，保持 approval resolve 与 resume 分离，暴露 advisory `resume_required`；message 202 后保留 exact accepted Turn，直到 AgentView 或同一 Turn 的 exact durable `LoopStarted`/terminal product frame 证明，command 完成、accepted Turn 首次可读或页面 focus 后立即 reconcile；running、accepted/cancel待确认、realtime degraded或存在 pending approval 时以 single-flight + coalesced rerun 低频 reconcile
- [ ] 9.10 增加in-process dispatcher unit tests与real NATS/API/Web integration tests，覆盖subscription readiness、Agent isolation、commit-before-publish、writer顺序、publish loss、cursor continuation/expiry、丢弃cold-buffer telemetry、overflow restart、durable/telemetry gap、terminal convergence、approval refresh、pagination与PG reconciliation

## 10. 删除旧 Store、Filesystem Execution、CAS、Bus 与配置

- [x] 10.1 当所有生产组合都使用 deterministic AgentLoop 后，删除 legacy Agent loop/API host path 及其测试，并从 workspace/dependency graph 彻底删除整个 `stratum-agent-builtin` crate 与 builtin REPL，不保留 compatibility island
- [x] 10.2 删除 `AgentStore` trait、state/error types、全部实现与调用方，再从 workspace 与 dependency graph 中删除整个 `stratum-store` crate
- [x] 10.3 删除 filesystem Agent state/history、filesystem durable sink/checkpoint reader、store event decorator、compact.jsonl、dual-backend replay、execution backend selector、fallback，以及全部关联 fixture/test
- [x] 10.4 删除专用于 execution storage 的 filesystem CAS record/version API、retry helper、error、local CAS machinery、export 与 CAS-specific test；保留 `VirtualPath`、sandboxed `LocalFilesystem`、template read，以及真实 read/list/write/create/remove/apply-patch 业务文件接口
- [x] 10.5 删除旧 generic EventBus、memory/NATS bus implementation、scoped Agent sink、StreamEnvelope、RuntimeEvent/AgentEvent transport DTO、Session-scoped SSE/replay code、旧 message-history transport 与过时 bus configuration；`stratum-infra` 中仅保留新的窄具体 Agent-tail boundary
- [x] 10.6 删除 `storage_root`、execution/history root 配置、backend-selection flag、自动创建 execution 目录、legacy NATS retention/replay 设置、compatibility alias，以及陈旧 environment/Docker/example configuration
- [x] 10.7 删除对外 per-Turn sequence、message sequence、next/last message counter、依赖 registry 判断 Agent 存在、implicit host creation、projection DTO，以及所有失效的 outcome/usage/runtime-snapshot state plumbing
- [x] 10.8 确保切换过程不会扫描、迁移、修改或删除用户现有 legacy filesystem 数据；只删除生产代码路径与配置合同

## 11. 文档、测试与删除证明

- [x] 11.1 为全部最终 route、request/response/error、Idempotency-Key、decimal event sequence、包含stream-ready/reset control的AgentStreamFrameV1、history page、approval view与已删除Session/replay surface重新生成OpenAPI
- [x] 11.2 更新 schema、runtime、recovery、approval、cancellation、NATS retention、template、destructive reset 与 operator 文档，并明确保持 scheduler 与 template management 延期
- [ ] 11.3 为最终设计的每个 transaction、crash window、race、严格 version boundary 与 typed error 增加聚焦 Rust unit test 和 ignored crate-local Postgres/NATS integration test
- [x] 11.4 对 create、streaming draft、approval refresh、recovery、terminal cleanup 与 upward pagination 运行 Web typecheck、lint、unit/component test 与 production build
- [ ] 11.5 在真实 Postgres 与 NATS 上验证完整端到端路径：create → message → durable stream → Tool result/approval → cancel 或 resume → process restart → refresh → history pagination
- [ ] 11.6 验证两个 preamble boundary、terminal commit 不确定、resolver/kernel 并发 append、NATS loss、expired cursor、bounded-buffer overflow、stale task cleanup，以及不使用 projection rebuild 的 compaction pointer-only fallback
- [x] 11.7 对 production code、manifest、config、test 与 docs 运行 `rg`，证明已删除 `AgentStore`、`stratum-store`、filesystem execution/CAS symbol、legacy sink/bus/SSE、compact.jsonl、beta projection/claim table、message_seq/per-Turn seq、replay parameter、旧 root 与 backend fallback
- [x] 11.8 审计完整 diff 的 kernel restraint：不得让 Postgres、Session、hosting、pagination、scheduler、approval projection 或 frontend state 进入 `stratum-agent`；允许纯 prepared-resume split 与已批准删除旧 `StreamEnvelope`、`RuntimeEvent`、`AgentEvent` 等 transport DTO，除此之外不得改变 kernel 行为
- [x] 11.9 运行 `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --all-targets` 与全部 crate-local ignored integration suites，不得通过压制 lint 规避失败

## 12. 最终校验与独立审查

- [x] 12.1 运行 `openspec validate complete-postgres-agent-runtime --type change --strict --no-interactive`，修复全部 artifact inconsistency
- [x] 12.2 运行 `openspec validate --all --strict`，并验证已归档的 superseded change 不能把过时 delta 带入 main specs
- [x] 12.3 针对完整 implementation diff 派发独立 constitution-review 子代理，要求逐条审查已修订的根 Constitution，并在 merge 前修复全部 red flag 与 violation
- [x] 12.4 在修复 constitution-review 问题后，重新运行 OpenSpec、Rust、Postgres/NATS integration、Web、deletion-proof 与 kernel-restraint gates
- [x] 12.5 逐项核对 checkbox 与具体 evidence，以最终 implementation convention 更新受影响 crate `AGENTS.md`，并在 merge 前提醒用户确认这些归档文档
- [ ] 12.6 只有在 implementation、verification、独立审查与 documentation 全部完成后，才准备同步并归档 `complete-postgres-agent-runtime`
