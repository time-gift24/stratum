## Context

当前 workspace 已有 `PostgresDurableEventSink`、初版 `PostgresAgentStore`、filesystem execution backend、Session-scoped EventBus 和两套 Agent composition。它们分别维护 durable event、message sequence、Agent state、history、approval、NATS cursor 与进程内 hosting，导致同一个 Turn 在崩溃、重试和浏览器刷新后可能从不同来源得到不同答案。

本设计面向当前 `/chat` 产品路径，允许破坏 beta API、schema 和配置。最高约束是保持 kernel 克制：`AgentLoop` 继续只理解 typed durable events、`DurableEventSink`、`TelemetryEventSink`、Hook runtime、Tool executor 与 cancellation token；Postgres、HTTP、Session、history、hosting、NATS 和 scheduler 不进入 kernel。Postgres 是唯一执行真相，NATS 只是低延迟、短保留且可丢失的观察通道。

当前 `CONSTITUTION.md` §1/§5 强制要求状态/定义经过 `stratum-store` 并保留 `stratum-agent-builtin` 装配层，§8 又把 NATS 不可用视为整体 readiness 失败，均与已确认目标冲突。实现前必须先做明确的最小宪法修订：删除 mandatory `stratum-store` 与 `stratum-agent-builtin` 层，由 concrete `stratum-postgres` 暴露执行存储接口、类型和 `thiserror` 错误；业务 crate 仍不得直接调用 `sqlx`；Postgres 决定核心 readiness，NATS 只决定 realtime capability 是否 degraded。

## Goals / Non-Goals

**Goals:**

- 以四张 Postgres 表和一条 agent-wide durable ledger 承载 Agent 的全部执行事实。
- 固定 Agent/current-Turn 状态机、Session 绑定、message CAS、恢复、审批、取消和崩溃窗口。
- 彻底删除 filesystem execution、旧 store/bus/sequence/projection，而不是保留 adapter、fallback 或双写。
- 在 Postgres commit 之后继续提供 NATS delta，并让 Web 在丢事件、刷新或 NATS 故障时确定性收敛。
- 永久保留原始历史和 compaction summary，同时允许恢复从 checkpoint 快速开始。
- 固定版本、类型化错误和安全 HTTP code，使前端无需解析错误字符串。

**Non-Goals:**

- 不实现 scheduler、lease/fencing、多实例 placement、rolling takeover、自动 resume 或 durable cancel。
- 不实现完整 Session 资源、Session 表或 Agent/Workflow 跨 owner 协调；当前仅约束 Agent runtime rows。
- 不实现 kernel 内并发 Tool；当前仍只有一个 active LLM call，schema 只避免把审批限制成 Turn 单例。
- 不实现 Agent template 管理、模板版本表、`GET /v1/agents` 或既有 Agent 的 template upgrade。
- 不迁移旧 filesystem/PG beta 执行数据，不提供双读、双写、upcaster 或 runtime rollback compatibility。
- 不改变 `/chat` 之外的产品范围，也不引入 Workflow/canvas UI。

## Architecture Comparison

| 维度 | 旧实现 | 本 change 收敛后 |
|---|---|---|
| 执行真相 | filesystem、`AgentStore`、PG 初版表并存 | concrete `stratum-postgres` 四表是唯一持久化真相 |
| Agent 定义 | 运行时可能重新读取当前模板 | create 时固化 immutable resolved definition，模板只影响新 Agent |
| 状态 | state 复制 snapshot、outcome、usage、approval/claim | `agent_state` 只保留 current/recent Turn 与 high-water |
| 历史与压缩 | message projection、filesystem index、messages snapshot | 直接读 durable ledger；companion 只保存一个 summary 与 retained pointer |
| 审批 | durable events 加审批 projection 双份状态 | Requested/Resolved/Consumed/Invalidated 全从 ledger 派生 |
| 顺序 | per-Turn/event/message/cursor 多套前沿 | durable 统一 Agent-wide `event_seq`，delta 只用 call-local `telemetry_seq` |
| 实时 | Session EventBus 同时承担观察与补发期待 | Agent-scoped NATS 短 tail；PG snapshot/history 负责恢复与收敛 |
| 依赖边界 | 单实现 store trait、builtin composition 与 backend selector | API 直接组合 concrete PG/NATS 能力，kernel 仍只见 sink/runtime contracts |

## Decisions

### 1. concrete `stratum-postgres` 是唯一 execution storage module

依赖方向调整为：

```text
stratum-core / stratum-agent     typed kernel contracts
              ▲
              │ DurableEventSink / typed events
stratum-postgres                concrete commands, queries, schema, errors
              ▲
              │ no raw sqlx outside this crate
stratum-api                     HTTP assembly/orchestration, process registry, NATS bridge
```

- 删除 `stratum-store` crate、`AgentStore` trait 和所有实现，同时删除 `stratum-agent-builtin` crate及其旧composition；不为单一 Postgres 实现保留 hypothetical seam。
- `stratum-postgres` 提供窄的 concrete create/admission/append/query/resolve 接口，自己拥有 storage DTO、状态类型和独立 `error.rs`。
- `stratum-agent` 不依赖 Postgres；装配层 `stratum-api` 将 concrete sink注入kernel，并把commit后的acknowledgement映射回现有kernel contract。PG-aware hosting/recovery orchestration只放在`stratum-api`可复用library modules中，`main.rs`保持薄。
- `stratum-infra` 不再承载 AgentStore 或旧通用 EventBus，但保留窄的 concrete Agent tail API 作为 NATS 唯一访问边界；业务代码不得直接使用 `async-nats`。
- `CONSTITUTION.md` §5 删除“必须经 generic event bus abstraction”的要求，改为“Agent realtime只经`stratum-infra`窄concrete tail boundary”；禁止业务crate直连`async-nats`的约束保持不变。
- `CONSTITUTION.md` §8 将 Postgres 定为核心 readiness 依赖；NATS 订阅或发布不可用只使 realtime capability degraded，create/message/resume/cancel/approval/history 等核心 PG command/query 继续可用。
- Constitution 的技术栈说明、crate DAG、§1/§5/§8 与 red flags，以及 `ARCH.md`、`TODO.md`、根 `CONTEXT.md` 和相关 crate `AGENTS.md`，必须同步所有权与 readiness 变化。

被否决方案：保留薄 `stratum-store` 会留下唯一实现 trait 和 pass-through 类型；业务 crate 直接使用 `sqlx` 会扩散事务不变量；把 Postgres 状态加入 kernel 会破坏可复用边界。

### 2. 四张表分别承载 immutable identity、薄状态、ledger 与 compaction

最终 baseline 的逻辑 schema 固定如下，实际 migration 使用 `TEXT + CHECK` 而不是 Postgres enum，所有核心外键使用 `RESTRICT`：

```text
agents
  agent_id                    uuid primary key
  agent_version_id            uuid not null unique
  idempotency_key             uuid not null unique
  source_template_name        text not null
  creation_model_override     jsonb null
  definition_schema_version   integer not null check > 0
  resolved_definition         jsonb not null
  created_at                  timestamptz not null

agent_state
  agent_id                    uuid primary key references agents on delete restrict
  status                      text not null check in
                              (idle,running,finished,failed,cancelled)
  session_id                  uuid null
  current_turn_id             uuid null
  default_model_config        jsonb not null
  last_event_seq              bigint not null default 0 check >= 0
  updated_at                  timestamptz not null

durable_events
  agent_id                    uuid not null references agents on delete restrict
  event_seq                   bigint not null check > 0
  session_id                  uuid not null
  turn_id                     uuid not null
  event_type                  text not null check known type
  event_version               integer not null check > 0
  payload                     jsonb not null
  runtime_snapshot_version    integer null check > 0
  runtime_snapshot            jsonb null
  created_at                  timestamptz not null
  primary key (agent_id,event_seq)

transcript_compactions
  agent_id                    uuid not null
  event_seq                   bigint not null
  turn_id                     uuid not null
  compacted_iteration         bigint not null check >= 0
  upto                        bigint not null check > 0
  retained_from_event_seq     bigint not null check > 0
  summary                     jsonb not null
  created_at                  timestamptz not null
  primary key (agent_id,event_seq)
  foreign key (agent_id,event_seq)
    references durable_events on delete restrict
```

`resolved_definition` 是 immutable Agent snapshot，包含解析后的 Agent 名称、system prompt、按序 tools、creation-time effective model config及运行所需定义身份；prompt、tools JSON与创建时有效模型就保存在这里。它不保存原始 TOML、模板路径或 template digest。三处模型数据职责不同：`creation_model_override`只保存客户端幂等请求输入，definition中的effective model是创建历史，`agent_state.default_model_config`是可变的下一Turn默认值；后者变化不得回写前两者。

`agent_state` 只包含 schema 中列出的 identity、current/recent Turn、default model、high-water 与时间字段，不保存 outcome、runtime snapshot、usage、approval、hosting 或 `resume_required`。数据库 CHECK 固定：idle 时 Session/current Turn 为空且 `last_event_seq=0`；running/terminal 时 Session/current Turn 非空。`UNIQUE(session_id) WHERE status='running'` 只实现当前版本的 Agent-only Session 单活，不声称解决 Workflow 或多实例调度。

`durable_events` 的 payload 只保存 event variant 数据，不重复嵌套 `{type,data}`。partial unique index 保证每个 `(agent_id,turn_id)` 只有一个 `LoopStarted`，并且 `LoopFinished|LoopFailed|LoopCancelled` 合计最多一个。snapshot 两列必须同时为空或同时存在，且只允许、也必须出现在 `LoopStarted` row。

Approval Requested/Resolved 的唯一性通过 durable row 的受约束 payload expression index 固定到 exact `hook_invocation_id`/`approval_id`；不为此增加通用列或 projection table。history 通过 `durable_events` 上只覆盖产品可见 event types 的 partial index读取。核心资产没有 delete API，FK 不 cascade。

`transcript_compactions` 是与 `TranscriptCompacted` event discriminator 同事务写入的专用 durable companion，不是可丢失 projection 或 messages snapshot。它只保存一个 summary、kernel `upto` 和第一条保留 message 的 durable pointer；不保存 `messages`、`summary_digest` 或 filesystem `window_start_line`。该event的durable payload固定为空对象，不复制summary、iteration、upto或pointer；store通过同`(agent_id,event_seq)`的companion物化完整typed event。对应discriminator与row必须互相校验，二者都永久保留。

被否决方案：`agent_messages` 和 `tool_approvals` 会制造第二份状态；`session_operation_claims` 提前定义了 scheduler/Workflow 语义；state snapshot/outcome/usage 会复制 ledger；Postgres enum 增加 beta schema 演进成本。

### 3. 所有持久 JSON 与外部 frame 显式版本化

以下版本独立存在并从 v1 开始：

- `agents.definition_schema_version`
- `durable_events.event_version`
- `durable_events.runtime_snapshot_version`
- `AgentStreamFrame.protocol_version`

当前不实现 upcaster。未知的新版本表示数据合法但当前 binary 不支持，映射 `runtime_incompatible`；已知版本无法解码或违反字段不变量，映射 `durable_state_corrupt`。错误必须由 owning library 的独立 `error.rs` 使用 `thiserror::Error` 表达并保留 source chain，不得通过字符串解析分类。

runtime snapshot 仅在 `LoopStarted` row envelope 保存，内容严格为：

```text
agent_version_id
effective_model_config
tool_set_fingerprint
skill_set_version_id
extension_set_version_id
ordered hook_handler_versions
```

snapshot 不保存 prompt、provider reconstruction、credential、secret 或 `base_event_seq`。该 Turn 的 `base_event_seq` 恒为 `LoopStarted.event_seq - 1`，无需冗余字段；terminal 不删除历史 snapshot。

**敏感载荷边界：** API按已确认合同原样保存被接受的user-authored conversation text，并把它按对话级敏感数据处理；系统不得声称能可靠扫描任意自然语言中的secret。与此分开，runtime自己掌握的provider/tool credential value永远不得进入template snapshot、ModelConfig、runtime snapshot、durable event、NATS frame或日志。Tool schema与Handler只能把非敏感业务参数或opaque credential reference放入最终arguments/authorization metadata，executor在approval消费后才从安全credential provider解析真实值；若final Tool call仍含typed secret value，Requested append必须fail closed。raw Tool result必须先经过`AfterToolCall`生成durable-safe表示，无法脱敏时只持久化安全结构化错误，不得落原始result。

因此本change必须同步澄清Constitution §5：绝对禁止的是runtime-managed secret/token/credential value被系统注入持久流；已接受的opaque user conversation按产品历史合同原样持久化，并由输入治理与retention policy作为敏感内容管理。本change不引入不可靠的通用secret scanner，也不允许调用方通过专用credential字段绕过安全引用边界。

### 4. Agent status 只描述 current/recent Turn，hosting 是易失观察

| Durable status | Process registry | 含义与可接受命令 |
|---|---|---|
| `idle` | 无 | Agent 已创建但无 Turn；message 可用 `expected_current_turn_id=null` admission |
| `running` | `starting` | preflight/managed task 安装中；resume 重试 204，cancel 返回 `turn_starting` |
| `running` | `running` | exact Turn 正被本进程推进；approval/cancel 可用，message 返回 `agent_busy` |
| `running` | 无 | durable Turn unhosted；approval 可写，explicit resume 可接管，cancel 返回 `turn_not_hosted` |
| `finished/failed/cancelled` | 无 | 最近 Turn 终态；携带 exact current Turn 的 message 可开始下一 Turn |

`current_turn_id` 在 terminal 后保留，下一次成功 admission 才替换。approval wait 不增加 durable status；Requested 未 Resolved 由 ledger 查询。cancel 202 只表示本机 token 接受信号，直到 terminal event commit 前状态仍是 running。

进程 registry 以 exact `(agent_id,turn_id)` 和唯一 claim identity 管理 `starting/running` handle、managed future 与 `CancellationToken`。旧 task cleanup 只能 compare-and-remove 自己的 exact claim，不能删除后来 Turn/claim。

当前 HTTP `resume_required` 是非持久化 advisory：

```text
status == running
&& 当前进程 registry 不存在 exact (agent_id,current_turn_id) 的 starting/running handle
```

它不参与 PG snapshot barrier，也不是命令授权依据；每个 command 必须重新校验 durable state。浏览器刷新不改变它，服务进程重启后 registry 为空会使遗留 running Turn 返回 true。后续 scheduler 必须替换其判定来源但保留 API 字段。

### 5. Template catalog、immutable Agent 与创建幂等分离

execution storage与template root相关配置收敛为：

```toml
[agent]
templates_root = "./agents"

[postgres]
url = "postgres://..."
```

既有provider/model/tool配置继续按各自能力存在；NATS连接与Agent短tail的age/bytes/message-count上限继续放在`[nats]`能力配置中。本决策删除的是`[storage]` backend selector、`storage_root`与execution目录语义，并非删除NATS或其他真实能力配置。

`templates_root` 是只读热读 catalog。启动时路径缺失、不是目录或不可读必须失败，空目录允许；服务不自动创建 templates/history/definition 目录。`GET /v1/agent-templates` 每次读取当前文件，catalog 中任一模板无效则整个请求失败，并且只返回安全目录信息，不返回 prompt、tools、raw TOML、路径或 digest。

`POST /v1/agents` 接受 `{agent_name, model_config?}` 和必填客户端 UUID `Idempotency-Key`：

1. 先按 key 查询 `agents`，不得先重新读取模板。
2. 命中且 template name/creation override 相同，永久返回原 Agent 的相同 `201 + Location`。
3. 命中但请求不同，返回 `409 idempotency_key_conflict`。
4. 未命中时热读最新模板，完成 definition/model/tool preflight，并在一个事务中写 immutable Agent 与 idle state。
5. 并发相同 key 由 unique constraint 收敛后重读；失败事务不占用 key。

Web 使用 `crypto.randomUUID()` 生成 key，并在请求结果未确定时保留同一 pending key。不同 AgentId 可以来自同一个 template；新 Agent 使用请求时最新模板，既有 Agent 永远使用自己的 resolved snapshot。

### 6. Message admission 保留两个 durable boundary

`POST /v1/agents/{id}/messages` body 必须包含原始 `text`、显式 nullable `expected_current_turn_id`，并可包含 `session_id` 与完整 `model_config` override。JSON body 硬限制 64 KiB；text 只 trim 用于判空，持久化原始内容。模型、provider parameters、tool set 与 runtime preflight 在任何 durable mutation 前完成。

首个 Turn 使用请求 SessionId，省略时服务端生成 UUIDv7；该 UUID 不要求预先存在于 sessions 表，因为当前没有 sessions 表。`LoopStarted` 一旦提交，Session 永久绑定 Agent。后续请求省略即复用，显式不同值返回 `session_mismatch`。

admission 顺序固定为：

1. 生成 TurnId，并在本进程安装 exact `starting` claim/token。
2. `LoopStarted` 事务锁 `agent_state`，比较 `expected_current_turn_id`，检查 Agent-only running Session unique constraint，分配 event_seq，写 row snapshot，绑定 Session/current Turn 并把 status 置 running。
3. kernel 随后通过标准 append 独立提交首条 user `MessageAppended`。
4. 只有 managed future 已安装且首条 user message 已提交，API 才返回 `202` 和 Agent/Session/Turn IDs。

两个 boundary 不合并。若只提交 `LoopStarted`，用户输入不可恢复，形成 started-only Turn；后续 explicit resume 原子追加安全 `LoopFailed`、置 failed，再返回 `409 turn_preamble_incomplete`。刷新后只公开通用 failed marker，不扩大 kernel terminal payload加入 API error code。

model override 是完整替换。`LoopStarted` snapshot 固定 effective config；只有首条 user message commit 且新值与 `default_model_config` 不同时才更新 state，相同值不写。started-only 不改变 default。

`expected_current_turn_id` 同时解决双 tab 和丢失响应：首次为 null，后续为最近 Turn。第一次请求一旦提交新的 current Turn，使用旧 expected value 的重试必定 `stale_turn`，即使该 Turn 已快速 terminal，也不会创建第二个 Turn。create 已成功而 message 失败时 Web 保留 idle AgentId 和原输入，只对同一 Agent 重试，不再 create。

### 7. Agent-wide event_seq 在线性化事务中分配

所有 durable writer——kernel sink、approval requester/resolver、started-only reconciliation——使用同一事务模板：

```text
BEGIN
  SELECT agent_state WHERE agent_id = ? FOR UPDATE
  validate exact Agent / Session / current Turn / status
  event_seq = last_event_seq + 1
  validate event version and variant payload
  INSERT durable_events
  INSERT transcript_compactions when event is TranscriptCompacted
  UPDATE only the state fields owned by this event
COMMIT
```

`agent_state` row lock既是 allocator 也是同 Agent 多 writer 的串行化点；不增加 approval row lock、sink-local counter、PG sequence、per-Turn seq 或 message_seq。commit 成功后 `last_event_seq` 与 rows `1..=last_event_seq` 必须无空洞。历史和 NATS 是过滤视图，因此可见序号允许跳过内部 Hook events。

terminal append 在同一事务插入唯一 terminal row并更新 status；不写 outcome，不清 snapshot。`latest_usage` 在读 AgentView 时从 current Turn 最新携带 usage 的 durable event派生，表示最近一次 provider response，不是 lifetime billing total。

sink 只在 commit 后向 kernel acknowledgement。commit receipt 进入 API-owned per-Agent realtime dispatcher；dispatcher 按 event_seq 从 PG 扫描并发布 product rows，避免 resolver/kernel post-commit 调度乱序。NATS 发布或 notification 失败只记录一次安全错误，不回滚 PG、不使 kernel 重复 append；当前不引入 durable outbox。

### 8. Recovery 使用 compaction base 加 exact current-Turn replay

resume 只接管 durable `running` 且本机 unhosted 的 exact Turn。`stratum-api` 外层runtime编排先安装 `starting` claim，再在固定屏障完成：

1. 读取 `LoopStarted` row并解码 v1 runtime snapshot；计算 `base_event_seq = loop_started.event_seq - 1`。
2. 捕获本次 `through_event_seq = agent_state.last_event_seq`，读取 `(base,through]` 的完整 rows；该区间必须无 event_seq 缺行且全部属于 exact current Turn。
3. 在 base 以内选择 event_seq 最大且有效的 `transcript_compactions` row，以 `[summary] + MessageAppended[retained_from_event_seq..base]` 物化历史基线。
4. base 前没有 compaction 时直接从 ledger 起点 replay；已有 companion summary 但 locator/`retained_from_event_seq` 加速信息无效时，忽略加速信息并在内存中从起点 full replay。若存在 `TranscriptCompacted` durable discriminator 却缺少其必需 companion row、单一 summary 无法解码或二者 identity 不一致，则 durable truth 不完整，返回 `durable_state_corrupt`；不修表、不提供 rebuild API。
5. 将历史基线与 current Turn 的真实 LoopStarted/Hook/message/tool/compaction/iteration events 组成 typed replay window，交给 exact runtime 的唯一 replay validator。

为了在 HTTP 202 前复用 kernel 现有 journal/Tool/compaction 校验而不复制状态机，唯一允许的新 kernel seam 是纯 `prepare_resume`：exact `Arc<AgentLoop>` 产生不可 Clone/Serialize、持有同一 runtime 的 opaque prepared value，后者只允许 consuming `run(token)`。它不得做 I/O、append、模型/Tool/Hook 调用，也不得接收 Postgres/Session/hosting。现有 resume 与该 seam 复用同一 private replay authority；除此之外不改变 fresh run、sink 或 Tool 串行语义。

历史 failed/cancelled Turn 若以未闭合 assistant tool-call group 结束，base materializer 只调整发送给 provider 的虚拟视图：零 result 删除 trailing group，存在精确前缀 results 时裁剪虚拟 assistant calls 并保留该前缀。原 durable/history 永不修改；current running Turn 不做 terminal normalization。

Tool completion 只有一个 truth：`MessageAppended(role=tool, tool_call_id=CallId, content=final JSON)`。Tool error 也是结构化 tool message，`after_tool_call` 可在 durable write 前变换/脱敏。恢复看到 `ToolExecutionStarted` 但没有 result message时 outcome 未知，以相同 CallId 重试；已有 result message则绝不重试，只完成 iteration。runtime 不定义外部服务幂等标准，也不增加 AttemptId。

除 started-only 外，snapshot/version 不兼容、runtime dependency 暂不可用、Hook/Tool history损坏或 PG failure都不猜测 terminal；移除本机 claim并保留 `running + unhosted`。只有 locator/retained pointer 这类加速信息损坏才走 full replay；必需 companion/summary 等 durable truth 损坏返回 `durable_state_corrupt`。

### 9. Approval 生命周期完全从 durable ledger 派生

保留 `DurableAgentEvent::ToolApprovalRequested/Resolved`，但不建 `tool_approvals` 表：

```text
Requested   = 存在 ToolApprovalRequested
Resolved    = 同 ApprovalId 存在 ToolApprovalResolved
Consumed    = exact hook_invocation_id 存在 HookInvocationCompleted
Invalidated = 所属 Turn 已 terminal
```

审批 Handler 仍是 `decide_tool_call` 的普通 Handler，kernel 不理解 HTTP approval。`HookInvocationPending` 必须先 durable；Handler 以 exact Hook invocation确保/reuse server-generated UUIDv7 ApprovalId，并在 Requested payload保存最终复验后的 Tool name、durable-safe arguments、CallId、非敏感授权 identity/reference与 `hook_invocation_id`。真实 credential value只在approval消费后从安全provider注入。映射回 kernel 时可以忽略这些 composition metadata。

resolver 事务只锁 `agent_state`，先验证请求的exact Turn identity并查询Requested/Resolved/terminal ledger：terminal必须优先返回`approval_invalidated`；若尚未terminal，再要求state仍为running，相同决定返回204，相反决定返回`approval_already_resolved`，只有未决定Requested才追加唯一Resolved。state row lock已经串行化同Agent writer，不增加approval row或第二套锁序。

waiter 使用 register-then-read：先按 ApprovalId 注册本机 waiter，再重读 PG；resolver 先 commit再 best-effort notify。通知/NATS 丢失不改变决定。hosted Handler收到 approve 映射 Execute，reject 映射 Block并继续生成模型可见 tool result；unhosted resolve只持久决定，必须显式 resume。没有 TTL。

`AgentView.pending_approvals` 在与 snapshot barrier 相同的 PG MVCC snapshot中查询 current Turn 的 Requested minus Resolved；Consumed/Invalidated不返回。浏览器当时未接收的 Requested在刷新后重新出现，Resolved不会再次要求人决定。

### 10. HTTP cold view、history 与稳定错误直接映射 ledger

`GET /v1/agents/{id}` 返回 API DTO，而不是数据库 row：Agent identity/name、status、default model、nullable Session/current Turn、`snapshot_event_seq`、`telemetry_floor_event_seq`、pending approvals、current Turn latest usage与 advisory `resume_required`。`snapshot_event_seq`不是新状态字段，必须等于同一MVCC snapshot中读取的`agent_state.last_event_seq`；barrier-governed `telemetry_floor_event_seq` 是该 snapshot 内最新一个通过严格解码的 assistant `MessageAppended.event_seq`，不存在时为0。公开 `outcome` 删除。除 registry-derived advisory 外，barrier、telemetry floor、status、usage和approvals来自同一PG snapshot。所有对外event sequence（view barrier、telemetry floor、history cursor/item与durable frame）统一编码为十进制字符串，避免JavaScript number精度改变identity。

`GET /v1/agents/{id}/history` 直接查询 `durable_events` partial index，不使用 message projection。必填 `through_event_seq`，可选 exclusive `before_event_seq` 与 limit；默认 50、最大 256，1 MiB soft page budget，首条自身超限仍完整返回。数据库反向取页，响应按 event_seq 升序并带 `next_before_event_seq/has_more`。可见 history items为 `MessageAppended`、`TranscriptCompacted`、安全的 `LoopFailed/LoopCancelled` marker；Tool result作为 role=tool message。History 与 SSE durable frame复用 API-owned `AgentProductEventV1` typed union，绝不直接暴露 raw durable payload。

成功语义固定：message与new resume返回202及Agent/Session/Turn IDs；cancel signal返回空body的202；already hosted/starting resume、exact already-cancelled、approval first/same retry均返回空body的204；create返回201 + Location。approval resolve与resume是不同endpoint。

所有 JSON request body硬限制 64 KiB，超限返回 413。library errors用 `thiserror`，HTTP统一映射安全 envelope `{"error":{"code":"...","message":"..."}}`。至少区分：

- 400：invalid request/cursor/history query
- 404：agent/template/approval not found
- 409：idempotency conflict、stale turn、agent busy、resume required、session mismatch/busy、turn not running/hosted/starting、preamble incomplete、approval resolved/invalidated、runtime incompatible
- 410：cursor expired
- 413：request too large
- 422：invalid template/model parameters
- 500：durable state corrupt、internal error
- 503：store/runtime/realtime unavailable、service shutting down

错误正文不包含 SQL、NATS subject、filesystem host path、prompt、Tool arguments/result、provider正文或 credential；来源链只在真正处理边界记录一次。

### 11. AgentStreamFrameV1 与 PG snapshot 形成端到端前端恢复

旧 `EventStreamBus`、`StreamEnvelope`、`RuntimeEvent/AgentEvent` transport DTO和公开 Session stream整体删除。新协议由 API拥有：

```text
AgentStreamFrameV1
  protocol_version = 1
  kind = control | durable | telemetry
  agent_id
  session_id? / turn_id?
  created_at

  control: stream_ready | stream_reset { reason: buffer_overflow }
  durable: event_seq, event_version, AgentProductEventV1
  telemetry: durable_before_event_seq, llm_call_id, telemetry_seq, typed LLM event
```

idle Agent可订阅，因此 control frame的 Session/Turn可以为空；Turn durable frame必须完整。任何存在的SSE id都只是不透明 NATS cursor，不能与 event_seq/telemetry_seq比较或持久化为业务状态。API只有在 NATS subscription已经建立并开始buffer后才发 `stream_ready`，客户端收到后才读取 PG cold snapshot。`stream_reset` 是 API 在单条已建立 SSE 上本地产生的控制信号，不写PG、不发NATS，也不携带SSE id。

每条telemetry在进入dispatcher bounded queue时冻结当时已知的PG durable high-water，并以十进制字符串`durable_before_event_seq`公开。dispatcher在发布该telemetry前先flush到该watermark。它只说明“这条telemetry排在这些durable facts之后”，不分配新的durable sequence，也不改变`(llm_call_id,telemetry_seq)` identity。若PG reconcile先应用了event_seq为F的assistant final，则随后到达且`durable_before_event_seq < F`的telemetry属于该final之前的旧tail，必须忽略；`durable_before_event_seq >= F`的下一call telemetry不能仅因此前final而被丢弃。

无 cursor从当前 tail开始；有 cursor只继续仍保留的短 tail。cursor仅在当前页面内存保存；页面刷新不恢复它。cursor过期在建流前返回410；建流后的server buffer overflow发送无id的`stream_reset(reason=buffer_overflow)`并关闭连接。Web收到reset后必须主动关闭原EventSource，丢弃该连接的buffer、draft与cursor，阻止浏览器携旧Last-Event-ID自动短重连，并从无cursor cold bootstrap重新开始。NATS retention使用可配置的短 age/bytes/messages上限，不承担 durable history或跨重启补发。

cold bootstrap 固定为：

1. 建立并 buffer Agent SSE，等待 `stream_ready`。
2. 读取 AgentView和 barrier内最新 history page。
3. 应用 PG snapshot，并用 `AgentView.telemetry_floor_event_seq` 初始化已收敛 assistant final floor，不能只靠最新 history page推导；durable frame `<= barrier` 跳过，`> barrier` 按 event_seq应用。
4. bootstrap期间buffered telemetry全部丢弃，因为没有可证明完整的call prefix。
5. bootstrap成功后才提交最新NATS cursor并进入live mode。

正常 reconcile是增量的：旧 barrier B 到新 barrier T时，从history反向分页直到越过B，只合并 `(B,T]` product events，并替换status/pending approvals等barrier-governed字段，包括用新view的`telemetry_floor_event_seq`推进已收敛assistant final floor；exact Turn仍running时保留已有telemetry draft，若新view已经terminal则执行和terminal frame相同的draft/未完成Tool UI清理。message 202 的exact accepted Turn保留到 AgentView 或同一 Turn 的 exact durable `LoopStarted`/terminal product frame 证明，避免并发cold snapshot仍是旧idle/terminal时停止轮询；accepted Turn存在时只接收该Turn的telemetry，否则只接收running AgentView的current Turn，防止上一Turn的NATS backlog复活draft。进入ready后立即reconcile。running、accepted/cancel待确认、realtime degraded或pending approval期间低频reconcile，窗口聚焦和每个command后立即reconcile。reconcile采用single-flight并把并发timer/focus/command合并成至多一次补跑，不能取消慢分页形成livelock。仅页面刷新、cursor过期、overflow或手动硬重置做cold rebuild。

当前每Turn只有一个active LLM call。统一per-Agent realtime dispatcher保证该call telemetry先于其final durable assistant message，final message关闭draft，下一call之后才开始；无需把 `llm_call_id` 写进kernel message。满队列的 durable wake 只推进单调 high-water；coalesced flush 必须先 snapshot target、再确认 accepted queue 为空，且只 flush 这个旧 snapshot，之后推进的 target 留给下一次 drain/idle 循环并在 idle 退休前追平。正常NATS顺序下按closed call忽略迟到telemetry；PG reconcile抢先看到final时按上述durable watermark区分旧call tail与final之后的新call。terminal event清空未闭合draft，并把无result的实时tool UI标为interrupted，不伪造result。

若NATS无法订阅，SSE返回 `503 realtime_unavailable`，Web显示克制的“实时连接降级”，核心commands继续工作并使用PG reconcile。durable publish失败也不改变command或kernel结果。

History中的`TranscriptCompacted`渲染为可折叠“上下文已压缩”marker，展开显示完整summary；不伪装system chat message、不增加全局banner。原消息只在用户向上滚动且需要时加载。approval 204后可以移除pending；若Turn unhosted则明确显示Resume，不自动resume。cancel 202只显示“取消请求已发送”，不能提前显示cancelled。

### 12. 删除边界与延期边界都必须可搜索验证

完整删除：

- `stratum-store` crate、workspace依赖与文档。
- `stratum-agent-builtin` crate、REPL与default-agent旧composition。
- FilesystemAgentStore、filesystem DurableEventSink、store decorator、state/history/definition执行目录、compact.jsonl和dual-backend tests。
- `stratum-filesystem`中的`cas.rs`、`record.rs`、`Filesystem::get/put`、record version、CAS errors和LocalFilesystem内存version state。
- 旧Session-scoped EventBus、NATS/memory bus、ScopedAgentEventSink、StreamEnvelope、旧AgentEvent/message_seq及其测试。
- `[storage]` backend selector、`storage_root`、兼容alias、自动目录创建、writable agent-data Docker volume和旧配置说明。

保留`stratum-filesystem`的`VirtualPath`、sandboxed `LocalFilesystem`、read/list/write/create/remove/apply-patch等真实业务文件能力；template catalog只以只读方式使用它，Agent tools仍可在自己的sandbox内使用写能力。磁盘上的旧用户文件不由程序自动删除。

独立scheduler PATCH/TODO必须记录：ownership lease/fencing、多实例hosting、rolling deployment、自动takeover/resume、durable cancel、Agent/Workflow Session协调，以及用scheduler ownership/placement替换`resume_required`的process-local来源。独立template-management TODO记录正式template版本、catalog管理和Agent列表。当前change不提前实现这些抽象。

## Risks / Trade-offs

- **[Agent state row成为单Agent写热点]** → 当前kernel串行且事务短；不同Agent互不阻塞，未来并发Tool出现后再测量。
- **[NATS commit后发布会丢失]** → PG/history/AgentView是恢复真相，前端增量reconcile；当前不增加outbox。
- **[cancel intent会在进程崩溃时丢失]** → 这是明确的当前限制，scheduler PATCH负责durable cancel；API不伪造cancelled。
- **[Tool started后结果未知会重复外部副作用]** → 复用同一CallId并明确at-least-once；外部服务自行实现幂等，runtime不定义通用标准。
- **[compaction companion 数据异常]** → locator/retained pointer 这类加速信息异常时忽略并做内存 full replay；缺少必需 companion/summary 或 summary 本身损坏意味着 durable truth 不完整，必须 fail closed；两种情况都不在线 repair。
- **[process-local hosting不能支持多实例]** → 不声明部署/rolling保证；引入第二实例前必须完成scheduler lease/fencing change。
- **[破坏性baseline无法保留beta执行数据]** → 部署前明确备份或丢弃，旧binary回滚必须同时重建旧DB/NATS。
- **[删除transport DTO会触及core crate]** → 只删除无生产消费者的旧transport surface；保留AgentLoop、durable/telemetry sink、approval variants和Tool串行语义。

## Migration Plan

1. 先修订`CONSTITUTION.md` §1/§5/§8、`ARCH.md`、`TODO.md`、`CONTEXT.md`与相关`AGENTS.md`，正式确定concrete `stratum-postgres`所有权、Postgres core readiness/NATS realtime degradation边界和scheduler/template后续PATCH。
2. 将`add-postgres-execution-storage`移入archive并标记superseded，保证其delta specs不再同步为目标架构。
3. 删除旧beta migration，写单一最终baseline；部署/测试显式drop并重建数据库及sqlx migration history，不写数据转换器。
4. 在`stratum-postgres`实现四表、约束、concrete commands/queries、durable append、compaction与ledger-derived reads，并用真实PG integration tests验证竞态。
5. 重写`stratum-api` assembly/runtime orchestration、process registry、approval Handler、resume/cancel与typed HTTP DTO；保持kernel改动只限纯prepare seam和旧transport删除。
6. 切换NATS到Agent-scoped短tail与API-owned frame，更新Web cold bootstrap、增量reconcile、审批、压缩marker和按需history。
7. 删除`stratum-store`、`stratum-agent-builtin`、filesystem execution/CAS、旧bus/sequence/config/tests/docs；用`rg`证明无生产残留。
8. 更新OpenAPI、Docker/config examples和运维说明；运行Rust/Web/PG/NATS测试、OpenSpec strict validation，再派发独立constitution-review子代理逐条审完整diff。

Rollback不承诺保留新beta数据；回到旧binary必须同时恢复旧schema/config/NATS，不能只回滚应用。

## Open Questions

无。scheduler、完整Session和template管理均已明确延期，不阻塞本change。
