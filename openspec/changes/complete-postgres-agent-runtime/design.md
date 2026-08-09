## Context

当前 workspace 已有 `PostgresDurableEventSink`、初版 `PostgresAgentStore`、filesystem execution backend、Session-scoped EventBus 和两套 Agent composition。它们分别维护 durable event、message sequence、Agent state、history、approval、NATS cursor 与进程内 hosting，导致同一个 Turn 在崩溃、重试和浏览器刷新后可能从不同来源得到不同答案。

已实现的 beta baseline 还把 `agents` 与单数 `agent_state` 做成一对一：每次创建运行实例都重复写入一份 resolved template definition 和唯一 `AgentVersionId`，实际上退化为一个 Session/对话一个 definition row。这与“filesystem template 是最新源，Postgres `agents` 是可复用的不可变版本历史”的边界相反，必须在归档前纠正。

本设计面向当前 `/chat` 产品路径，允许破坏 beta API、schema 和配置。最高约束是保持 kernel 克制：`AgentLoop` 继续只理解 typed durable events、`DurableEventSink`、`TelemetryEventSink`、Hook runtime、Tool executor 与 cancellation token；Postgres、HTTP、Session、history、hosting、NATS 和 scheduler 不进入 kernel。Postgres 是唯一执行真相，NATS 只是低延迟、短保留且可丢失的观察通道。

当前 `CONSTITUTION.md` §1/§5 强制要求状态/定义经过 `stratum-store` 并保留 `stratum-agent-builtin` 装配层，§8 又把 NATS 不可用视为整体 readiness 失败，均与已确认目标冲突。实现前必须先做明确的最小宪法修订：删除 mandatory `stratum-store` 与 `stratum-agent-builtin` 层，由 concrete `stratum-postgres` 暴露执行存储接口、类型和 `thiserror` 错误；业务 crate 仍不得直接调用 `sqlx`；Postgres 决定核心 readiness，NATS 只决定 realtime capability 是否 degraded。

## Goals / Non-Goals

**Goals:**

- 以四张复数命名的 Postgres 表分离可复用的 immutable Agent template 版本与 `AgentRuntimeId`-wide 执行事实。
- 固定 `AgentId` 与 `AgentRuntimeId` 的一对多关系：`agents.id` 标识一个不可变 template 版本，多个 `agent_states.id` 可 pin 同一 `AgentId`；template 作者用字符串 `version` tag 声明版本身份，storage 只物化或校验 exact `(name,version)`，不分配、不排序 tag。
- 固定 AgentRuntime/current-Turn 状态机、Session 绑定、message CAS、恢复、审批、取消和崩溃窗口。
- 彻底删除 filesystem execution、旧 store/bus/sequence/projection，而不是保留 adapter、fallback 或双写。
- 在 Postgres commit 之后继续提供 NATS delta，并让 Web 在丢事件、刷新或 NATS 故障时确定性收敛。
- 永久保留原始历史和 compaction summary，同时允许恢复从有效 compaction base 快速开始。
- 固定版本、类型化错误和安全 HTTP code，使前端无需解析错误字符串。

**Non-Goals:**

- 不实现 scheduler、lease/fencing、多实例 placement、rolling takeover、自动 resume 或 durable cancel。
- 不实现完整 Session 资源、Session 表或 Agent/Workflow 跨 owner 协调；当前仅约束 AgentRuntime rows。
- 不实现 kernel 内并发 Tool；当前仍只有一个 active LLM call，schema 只避免把审批限制成 Turn 单例。
- 不实现 template CRUD/catalog administration、显式版本列表、发布/提升/回滚、`GET /v1/agents`、`GET /v1/agents/{agent_id}` 或既有 runtime 的 template upgrade；create 时自动物化并复用 immutable `AgentId` 版本属于本 change。
- 不迁移旧 filesystem/PG beta 执行数据，不提供双读、双写、upcaster 或 runtime rollback compatibility。
- 不改变 `/chat` 之外的产品范围，也不引入 Workflow/canvas UI。

## Architecture Comparison

| 维度 | 旧实现 | 本 change 收敛后 |
|---|---|---|
| 执行真相 | filesystem、`AgentStore`、PG 初版表并存 | concrete `stratum-postgres` 四表是唯一持久化真相 |
| Agent 定义 | 运行时可能重新读取当前模板，或每个对话重复一份 snapshot | filesystem catalog 是最新源；`agents(id,name,version)` 永久保留不可变历史，多个 `AgentRuntimeId` 可 pin 同一 `AgentId` |
| 状态 | state 复制 snapshot、outcome、usage、approval/claim | `agent_states` 每个 `AgentRuntimeId` 一行，只保留 `AgentId` pin、create key、单一 `model_config`、current/recent Turn 与 high-water |
| 历史与压缩 | message projection、filesystem index、messages snapshot | 直接读 durable ledger；companion 只保存一个 summary 与 retained pointer |
| 审批 | durable events 加审批 projection 双份状态 | Requested/Resolved/Consumed/Invalidated 全从 ledger 派生 |
| 顺序 | per-Turn/event/message/cursor 多套前沿 | durable 统一 AgentRuntime-wide `event_seq`，delta 只用 call-local `telemetry_seq` |
| 实时 | Session EventBus 同时承担观察与补发期待 | AgentRuntime-scoped NATS 短 tail；PG snapshot/history 负责恢复与收敛 |
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
- `stratum-infra` 不再承载 AgentStore 或旧通用 EventBus，但保留窄的 concrete AgentRuntime tail API 作为 NATS 唯一访问边界；业务代码不得直接使用 `async-nats`。
- `CONSTITUTION.md` §5 删除“必须经 generic event bus abstraction”的要求，改为“AgentRuntime realtime只经`stratum-infra`窄concrete tail boundary”；禁止业务crate直连`async-nats`的约束保持不变。
- `CONSTITUTION.md` §8 将 Postgres 定为核心 readiness 依赖；NATS 订阅或发布不可用只使 realtime capability degraded，create/message/resume/cancel/approval/history 等核心 PG command/query 继续可用。
- Constitution 的技术栈说明、crate DAG、§1/§5/§8 与 red flags，以及 `ARCH.md`、`TODO.md`、根 `CONTEXT.md` 和相关 crate `AGENTS.md`，必须同步所有权与 readiness 变化。

被否决方案：保留薄 `stratum-store` 会留下唯一实现 trait 和 pass-through 类型；业务 crate 直接使用 `sqlx` 会扩散事务不变量；把 Postgres 状态加入 kernel 会破坏可复用边界。

### 2. 四张表分别承载 immutable identity、薄状态、ledger 与 compaction

最终 baseline 的逻辑 schema 固定如下，实际 migration 使用 `TEXT + CHECK` 而不是 Postgres enum，所有核心外键使用 `RESTRICT`：

```text
agents
  id                          uuid primary key
  name                        text not null
  version                     text collate "C" not null check valid tag
  definition_schema_version   integer not null check > 0
  resolved_definition         jsonb not null
  created_at                  timestamptz not null
  unique (name,version)

agent_states
  id                          uuid primary key
  agent_id                    uuid not null references agents(id) on delete restrict
  idempotency_key             uuid not null unique
  status                      text not null check in
                              (idle,running,finished,failed,cancelled)
  session_id                  uuid null
  current_turn_id             uuid null
  model_config                jsonb not null
  last_event_seq              bigint not null default 0 check >= 0
  created_at                  timestamptz not null
  updated_at                  timestamptz not null

durable_events
  agent_runtime_id            uuid not null references agent_states(id) on delete restrict
  event_seq                   bigint not null check > 0
  session_id                  uuid not null
  turn_id                     uuid not null
  event_type                  text not null check known type
  event_version               integer not null check > 0
  payload                     jsonb not null
  runtime_snapshot_version    integer null check > 0
  runtime_snapshot            jsonb null
  created_at                  timestamptz not null
  primary key (agent_runtime_id,event_seq)

transcript_compactions
  agent_runtime_id            uuid not null
  event_seq                   bigint not null
  turn_id                     uuid not null
  compacted_iteration         bigint not null check >= 0
  upto                        bigint not null check > 0
  retained_from_event_seq     bigint not null check > 0
  summary                     jsonb not null
  created_at                  timestamptz not null
  primary key (agent_runtime_id,event_seq)
  foreign key (agent_runtime_id,event_seq)
    references durable_events on delete restrict
```

领域身份固定如下，禁止同一个 UUID newtype 同时表达 definition 与 runtime：

| 领域概念 | SQL identity | 生命周期 |
|---|---|---|
| `AgentId` | `agents.id` | 一个不可变 template 版本，永久保留且可被复用 |
| `AgentRuntimeId` | `agent_states.id` | 一个长期运行聚合，可跨多个 Turn，永久 pin 一个 `AgentId` |
| `SessionId` | `agent_states.session_id` | 首个 `LoopStarted` 后绑定到 runtime |
| `TurnId` | durable/current Turn identity | runtime 内的一次可恢复执行 |

`AgentVersionId` 完全删除；`agents.version` 不是 UUID 或数值顺序，只是 template 作者命名、由 storage 持久化并校验的字符串 tag。`AgentId` 与 `AgentRuntimeId` 都是服务端生成的 UUIDv7 newtype，客户端不得指定。

HTTP、storage、dispatcher 与 tracing 的运行上下文统一记录 `agent_runtime_id`；需要定义来源时再附加 `agent_id`、`agent_name`、`agent_version`，不能继续让日志字段 `agent_id` 同时表示两个资源。`AgentRuntimeId` 可作为跨 API/storage/infra 的 shared newtype 放在 `stratum-core`，但 `AgentLoop`、kernel durable event variant 与 `prepare_resume` 均不持有它。

`agents` 只是 immutable template-version registry。`id: AgentId` 唯一固定一行定义；`name` 是 template provenance，`version` 是 template TOML 作者必填的字符串 tag。tag 校验固定为：UTF-8 编码后 `1..=128` bytes、不得包含控制字符、不得有首尾空白；比较使用原始UTF-8 bytes、大小写敏感，不做 trim、case folding、Unicode normalization、SemVer 解析或排序。application 使用 validated string newtype，数据库以 `TEXT COLLATE "C"`、`UNIQUE(name,version)` 及等价 CHECK 作最终 backstop。

`version` 与 `definition_schema_version` 完全独立：前者回答“作者给这份定义贴了什么 tag”，后者回答“JSON 用哪个 codec 解码”。精确 `(name,version)` 已存在且 canonical definition 相同时复用其 `AgentId`；已存在但 definition 不同返回 typed `AgentVersionConflict` / `409 agent_version_conflict`；不同 tag 即使 definition 相同也创建新的 `agents` row。storage 永不修改已有 row，也不计算 latest/max/next version。

`resolved_definition` 是 template-derived canonical v1 定义，包含 system prompt、按序 tools、template 默认模型与运行所需非敏感定义身份；`name` 只存在于列中，不在 JSON 内复制。它不保存原始 TOML、模板路径、`AgentRuntimeId`、创建请求 override、effective runtime model、时间戳或 definition fingerprint。对象键使用唯一 canonical 编码，tools 数组顺序是定义语义的一部分。

`agent_states` 每个 `AgentRuntimeId` 一行，并在整个生命期内通过 `agent_id` 不可变地 pin 一个 `AgentId`。它只包含 schema 中列出的 create key、唯一可变 `model_config`、current/recent Turn、high-water 与时间字段，不保存 outcome、runtime snapshot、usage、approval、hosting 或 `resume_required`。数据库 CHECK 固定：idle 时 Session/current Turn 为空且 `last_event_seq=0`；running/terminal 时 Session/current Turn 非空。`UNIQUE(session_id) WHERE status='running'` 只实现当前版本的 AgentRuntime-only Session 单活，不声称解决 Workflow 或多实例调度。

关系固定为 `agents 1 -> N agent_states`：相同 template 版本可被多个相互独立的 `AgentRuntimeId` 复用，各自拥有独立 status、Session/Turn、`model_config`、event allocator 和 ledger。`durable_events.agent_runtime_id` 因此引用 `agent_states.id`，不引用 template-version row。

`durable_events` 的 payload 只保存 event variant 数据，不重复嵌套 `{type,data}`。partial unique index 保证每个 `(agent_runtime_id,turn_id)` 只有一个 `LoopStarted`，并且 `LoopFinished|LoopFailed|LoopCancelled` 合计最多一个。snapshot 两列必须同时为空或同时存在，且只允许、也必须出现在 `LoopStarted` row。

Approval Requested/Resolved 的唯一性通过 durable row 的受约束 payload expression index 固定到 exact `hook_invocation_id`/`approval_id`；不为此增加通用列或 projection table。history 通过 `durable_events` 上覆盖完整安全 `AgentRuntimeProductEventV1` event types、排除internal facts的partial index读取。核心资产没有 delete API，FK 不 cascade。

`transcript_compactions` 是与 `TranscriptCompacted` event discriminator 同事务写入的专用 durable companion，不是可丢失 projection 或 messages snapshot。它只保存一个 summary、kernel `upto` 和第一条保留 message 的 durable pointer；不保存 `messages`、`summary_digest` 或 filesystem `window_start_line`。该event的durable payload固定为空对象，不复制summary、iteration、upto或pointer；store通过同`(agent_runtime_id,event_seq)`的companion物化完整typed event。对应discriminator与row必须互相校验，二者都永久保留。

被否决方案：`agents` 与 `agent_states` 一对一会把 Session/对话数量误当成 template 版本数量；额外 `AgentVersionId` 会与 `agents.id` 重复表达同一 identity；`creation_model_override` 与可变 runtime model 并存会制造双份模型真相；definition fingerprint 对当前严格 typed canonical 比较没有额外价值。`agent_messages` 和 `tool_approvals` 会制造第二份状态；`session_operation_claims` 提前定义了 scheduler/Workflow 语义；state snapshot/outcome/usage 会复制 ledger；Postgres enum 增加 beta schema 演进成本。

### 3. 所有持久 JSON 与外部 frame 显式版本化

以下版本独立存在并从 v1 开始：

- `agents.definition_schema_version`
- `durable_events.event_version`
- `durable_events.runtime_snapshot_version`
- `AgentRuntimeStreamFrameV1.protocol_version`

当前不实现 upcaster。未知的新版本表示数据合法但当前 binary 不支持，映射 `runtime_incompatible`；已知版本无法解码或违反字段不变量，映射 `durable_state_corrupt`。错误必须由 owning library 的独立 `error.rs` 使用 `thiserror::Error` 表达并保留 source chain，不得通过字符串解析分类。

runtime snapshot 仅在 `LoopStarted` row envelope 保存，内容严格为：

```text
agent_id
effective_model_config
tool_set_fingerprint
skill_set_version_id
extension_set_version_id
ordered hook_handler_versions
```

`agent_states.agent_id`、`LoopStarted.runtime_snapshot.agent_id` 与加载的 `agents.id` 必须三者一致。fresh Turn 只能从 state 已 pin 的 `AgentId` 构造 snapshot；resume 先通过 state 加载 immutable definition，再校验 snapshot。definition row 缺失、identity 不一致或 definition v1 无法严格解码都是 `durable_state_corrupt`，不得重读 filesystem template 尝试修复。`AgentRuntimeId` 由 API-owned sink 与恢复编排绑定，不进入 kernel snapshot 或 `AgentLoop`。

Hook journal 继续保持 kernel-minimal：现有 Pending/Completed/Failed durable variant 只保存 invocation identity、Hook point、iteration、CallId、digest 与 decision/failure，不新增 `AgentId`、`AgentRuntimeId`、Session 或 Turn 字段，也不把进程内 `HookInvocationAddress` 整体持久化。API-owned sink 必须把这些事实写入绑定 exact AgentRuntime/Session/Turn 的外层 ledger；storage append、strict decode 与 resume replay 必须校验外层 row identity，并以 current `LoopStarted` snapshot 的 `AgentId` 作为唯一 definition pin。共享一个 `AgentId` 的多个 runtime 依靠各自 ledger 与 Turn identity 隔离，不把 runtime identity 塞回 kernel journal。

snapshot 不保存 prompt、provider reconstruction、credential、secret 或 `base_event_seq`。该 Turn 的 `base_event_seq` 恒为 `LoopStarted.event_seq - 1`，无需冗余字段；terminal 不删除历史 snapshot。

**敏感载荷边界：** API按已确认合同原样保存被接受的user-authored conversation text，并把它按对话级敏感数据处理；系统不得声称能可靠扫描任意自然语言中的secret。当前closed composition只注册Echo：它的参数与结果是同一份schema-validated、user-authored opaque JSON，authorization只有typed `ToolKind`/`DangerLevel`，不存在runtime-managed credential/reference/provider通道；Echo result仍必须经过`AfterToolCall`，但`Keep`只表示这个closed composition允许把该conversation data持久化，不是通用secret扫描或脱敏证明。HTTP strict DTO拒绝专用credential字段。未来credential-aware Tool必须由独立PATCH同时引入opaque reference、批准消费后的安全provider注入与fail-closed result transform；在该边界完成前不得注册。

因此本change必须同步澄清Constitution §5：绝对禁止的是runtime-managed secret/token/credential value被系统注入持久流；已接受的opaque user conversation按产品历史合同原样持久化，并由输入治理与retention policy作为敏感内容管理。本change不引入不可靠的通用secret scanner，也不允许调用方通过专用credential字段绕过安全引用边界。

### 4. AgentRuntime status 只描述 current/recent Turn，hosting 是易失观察

| Durable status | Process registry | 含义与可接受命令 |
|---|---|---|
| `idle` | 无 | AgentRuntime 已创建但无 Turn；message 可用 `expected_current_turn_id=null` admission |
| `running` | `starting` | preflight/managed task 安装中；resume 重试 204，cancel 返回 `turn_starting` |
| `running` | `running` | exact Turn 正被本进程推进；approval/cancel 可用，message 返回 `agent_runtime_busy` |
| `running` | 无 | durable Turn unhosted；approval 可写，explicit resume 可接管，cancel 返回 `turn_not_hosted` |
| `finished/failed/cancelled` | 无 | 最近 Turn 终态；携带 exact current Turn 的 message 可开始下一 Turn |

`current_turn_id` 在 terminal 后保留，下一次成功 admission 才替换。approval wait 不增加 durable status；Requested 未 Resolved 由 ledger 查询。cancel 202 只表示本机 token 接受信号，直到 terminal event commit 前状态仍是 running。

进程 registry 以 exact `(agent_runtime_id,turn_id)` 和唯一 claim identity 管理 `starting/running` handle、managed future 与 `CancellationToken`。旧 task cleanup 只能 compare-and-remove 自己的 exact claim，不能删除后来 Turn/claim。

当前 HTTP `resume_required` 是非持久化 advisory：

```text
status == running
&& 当前进程 registry 不存在 exact (agent_runtime_id,current_turn_id) 的 starting/running handle
```

它不参与 PG snapshot barrier，也不是命令授权依据；每个 command 必须重新校验 durable state。浏览器刷新不改变它，服务进程重启后 registry 为空会使遗留 running Turn 返回 true。后续 scheduler 必须替换其判定来源但保留 API 字段。

### 5. Template catalog、user-authored version tag 与 runtime create 幂等分离

execution storage与template root相关配置收敛为：

```toml
[agent]
templates_root = "./agents"

[postgres]
url = "postgres://..."
```

既有provider/model/tool配置继续按各自能力存在；NATS连接与AgentRuntime短tail的age/bytes/message-count上限继续放在`[nats]`能力配置中。本决策删除的是`[storage]` backend selector、`storage_root`与execution目录语义，并非删除NATS或其他真实能力配置。

`templates_root` 是只读热读 catalog。启动时路径缺失、不是目录或不可读必须失败，空目录允许；服务不自动创建 templates/history/definition 目录。每份 TOML 必须包含顶层 `version = "..."`；`name` 继续来自已验证的 catalog identity，version 由模板作者维护。`GET /v1/agent-templates` 每次读取当前文件，catalog 中任一模板缺 tag 或 tag 无效则整个请求失败；响应只返回安全的 name、version 与模型目录信息，不返回 prompt、tools、raw TOML、路径或 digest。

`POST /v1/agent-runtimes` 接受 `{agent_name, model_config?}` 和必填客户端 UUID `Idempotency-Key`。request 不含 `AgentId`、`AgentRuntimeId` 或 `version`；前两个 identity 由服务端生成，version 只能来自当前 template TOML：

1. 先按 key 查询 `agent_states`，不得先重新读取 template、验证新 model override 或重新解释创建意图。
2. 命中 key 时无条件返回首次创建的原 `AgentRuntimeId`、pinned `AgentId`、name/version metadata 和同一 `201 + Location`。key 是 command identity，不是可变 `model_config` 的请求指纹；调用方不得用同一 key 表示第二个创建意图。
3. 未命中时热读当前 template，校验作者提供的 version tag，完成 definition/model/tool preflight，并构造 template-only canonical `resolved_definition`。create override 不参与 definition equality。
4. 在 Postgres 事务中按 exact `(name,version)` 获取 transaction-scoped advisory lock，再次查询 key，然后读取同一 pair 的 `agents` row。不存在时生成新 `AgentId` 并插入；存在且 `definition_schema_version + resolved_definition` 严格相等时复用；存在但任一值不同则返回 `AgentVersionConflict` 并回滚。`UNIQUE(name,version)` 是最终并发 backstop；不增加 fingerprint 列或大 JSON unique index。
5. 生成新 `AgentRuntimeId`，原子写入可能的 `agents` row 与 idle `agent_states` row。state 的 `agent_id` pin 选定的 `AgentId`，持有 unique key，并以“完整 create override，否则 template 默认模型”初始化唯一 `model_config`。版本插入与 state 插入必须在同一事务，失败不得留下无引用版本或消耗 key。
6. 并发相同 key 由 `agent_states.idempotency_key` unique constraint 收敛；输家回滚自己的全部 mutation 后重读 winner，按 key-only 规则返回原 runtime。

canonical definition 不包含 `AgentId`、`AgentRuntimeId`、name、version、created time、create override、runtime `model_config`、raw path 或任何非确定顺序。版本相等性只在 exact `(name,version)` 内判断：不同 name 或不同 tag 即使 canonical JSON 相同也不共享 `AgentId`；同一 pair 再次出现且定义相同则复用，无论期间 catalog 曾指向什么其他 tag。tag 没有“最新”“更大”或回退语义，当前 filesystem 文件本身才是 create 时的最新源。

Web 使用 `crypto.randomUUID()` 生成 key，并在请求结果未确定时保留同一 pending key。同一 `(name,version)` 与相同定义下的不同 key 创建不同 `AgentRuntimeId`，但共享同一 `AgentId`；作者改用新 tag 时新 runtime pin 新 `AgentId`；作者复用旧 tag 却修改定义时 create 明确冲突；既有 runtime 永远通过 state pin 使用原定义。

### 6. Message admission 保留两个 durable boundary

`POST /v1/agent-runtimes/{agent_runtime_id}/messages` body 必须包含原始 `text`、显式 nullable `expected_current_turn_id`，并可包含 `session_id` 与完整 `model_config` override。JSON body 硬限制 64 KiB；text 只 trim 用于判空，持久化原始内容。模型、provider parameters、tool set 与 runtime preflight 在任何 durable mutation 前完成。

preflight 先按 `AgentRuntimeId` 从 `agent_states` 取得 pinned `AgentId` 和当前 `model_config`，再从 `agents` 加载 prompt、tools 与 template 定义身份；永不在 Turn admission 期间重读 filesystem template。

首个 Turn 使用请求 SessionId，省略时服务端生成 UUIDv7；该 UUID 不要求预先存在于 sessions 表，因为当前没有 sessions 表。`LoopStarted` 一旦提交，Session 永久绑定 AgentRuntime。后续请求省略即复用，显式不同值返回 `session_mismatch`。

admission 顺序固定为：

1. 生成 TurnId，并在本进程安装 exact `starting` claim/token。
2. 在开启`LoopStarted`写事务前调用无caller-frontier参数的`DispatcherHub::ensure(AgentRuntimeId)`；hub在per-runtime ensure/retirement gate内读取当前committed PG high-water、安装或复用generation并返回live handle。
3. `LoopStarted`事务按`AgentRuntimeId`锁`agent_states`，比较`expected_current_turn_id`、检查AgentRuntime-only running Session unique constraint，分配event_seq、写row snapshot、绑定Session/current Turn并把status置running；handle跨commit持有，commit后只通过该同一handle提交receipt。
4. kernel 随后通过标准 append 独立提交首条 user `MessageAppended`。
5. 只有 managed future 已安装且首条 user message 已提交，API 才返回 `202` 和 AgentRuntime/Session/Turn IDs。

两个 boundary 不合并。若只提交 `LoopStarted`，用户输入不可恢复，形成 started-only Turn；后续 explicit resume 原子追加安全 `LoopFailed`、置 failed，再返回 `409 turn_preamble_incomplete`。刷新后只公开通用 failed marker，不扩大 kernel terminal payload加入 API error code。

model override 是完整替换。`LoopStarted` snapshot 固定 pinned `AgentId` 与该 Turn effective config；只有首条 user message commit 且新值与 `agent_states.model_config` 不同时才更新这一字段，相同值不写。started-only 不改变 `model_config`。Turn override 不得创建或切换 `AgentId`，也不得回写 template 默认模型。

`expected_current_turn_id` 同时解决双 tab 和丢失响应：首次为 null，后续为最近 Turn。第一次请求一旦提交新的 current Turn，使用旧 expected value 的重试必定 `stale_turn`，即使该 Turn 已快速 terminal，也不会创建第二个 Turn。create 已成功而 message 失败时 Web 保留 idle `AgentRuntimeId` 和原输入，只对同一 runtime 重试，不再 create。

### 7. AgentRuntime-wide event_seq 在线性化事务中分配

所有 durable writer——fresh `LoopStarted` admission、kernel sink（包括 compaction 与 terminal）、approval requester/resolver、started-only reconciliation，以及未来任何新增 writer——使用同一事务模板：

```text
BEGIN
  SELECT agent_states WHERE id = ? FOR UPDATE
  validate exact AgentRuntime / Session / current Turn / status
  event_seq = last_event_seq + 1
  validate event version and variant payload
  INSERT durable_events
  INSERT transcript_compactions when event is TranscriptCompacted
  UPDATE only the state fields owned by this event
COMMIT
```

`agent_states` row lock既是 allocator 也是同一 AgentRuntime 多 writer 的串行化点；不增加 approval row lock、sink-local counter、PG sequence、per-Turn seq 或 message_seq。commit 成功后 `last_event_seq` 与 rows `1..=last_event_seq` 必须无空洞。同一 `AgentId` 下的不同 `AgentRuntimeId` 锁不同 state rows、各自从 seq=1 开始，不能互相串行化。历史和 NATS 是过滤视图，因此可见序号允许跳过内部 Hook events。

terminal append 在同一事务插入唯一 terminal row并更新 status；不写 outcome，不清 snapshot。`latest_usage` 在读 AgentRuntimeView 时从 current Turn 最新携带 usage 的 durable event派生，表示最近一次 provider response，不是 lifetime billing total。

sink 只在 commit 后向 kernel acknowledgement。commit receipt 进入 API-owned per-AgentRuntime realtime dispatcher；dispatcher 按 event_seq 从 PG 扫描并发布 product rows，避免 resolver/kernel post-commit 调度乱序。NATS 发布或 notification 失败只记录一次安全错误，不回滚 PG、不使 kernel 重复 append；当前不引入 durable outbox。

dispatcher generation 的初始 publish frontier 必须来自 exact runtime 的已提交PG high-water。`DispatcherHub::ensure(AgentRuntimeId)`不接收caller frontier；当generation不存在或正在退休时，hub在同一per-runtime gate内保留初始化位置、读取`agent_states.last_event_seq`、以该值安装generation并取得首个live handle，并发ensure等待且共享该初始化。PG read、generation install、handle acquire与retirement在同一gate上线性化；ensure只做PG read和本机注册，不等待NATS publish。每个可能追加durable row的路径——fresh `LoopStarted` admission、kernel sink、approval requester/resolver、started-only reconciliation及未来writer——都必须在开启写事务前取得或复用live handle，跨过mutation/commit持有，并在commit后只通过同一handle提交receipt；失败事务不发receipt。

resume先完成fixed durable slice、definition/provider/tool fingerprint、lineage与typed replay window等不依赖bound sink的preflight，再ensure并用handle组装API-owned sinks与exact loop执行纯`prepare_resume`；prepare失败不得有durable write或外部动作，且释放handle/claim。prepare成功后再用短state-row-lock事务重验definition pin与running/current Turn；失败释放handle与claim，成功则API-owned bound sinks/managed task持有handle到Turn退出。initial frontier可高于replay through barrier但不改变prepared replay window，期间新增approval facts仍按ledger消费。

正常generation只有在frontier追平target、accepted queue为空且所有durable writer/hosted Turn producer handles均释放时才退休；任一live handle阻止retirement。若NATS持续失败且handles已归零，内部有界retry budget耗尽后hub在线性化gate内标记degraded、丢弃未发布volatile queue/target并退休，避免每个历史runtime永久残留task；这不修改PG，下一次ensure从最新committed high-water开始，缺口由PG reconcile补。retiring/degraded-abandonment与ensure竞争只能线性化为加入旧generation或关闭后建立新generation，不能并存两个publisher。进程启动不扫描或预建所有runtime frontier；receipt不能隐式建generation，也不得从0、NATS cursor、caller缓存或当前caller mutation提交后的high-water猜起点。dispatcher只扫描`(frontier,target]`，旧历史始终由AgentRuntimeView/history恢复。

### 8. Recovery 使用 compaction base 加 exact current-Turn replay

resume 只接管 durable `running` 且本机 unhosted 的 exact Turn。`stratum-api` 外层runtime编排先安装 `starting` claim，再在固定屏障完成：

1. 在同一固定读取中按 `AgentRuntimeId` 取得 exact `agent_states` row，再按其 immutable `agent_id` 加载 `agents.resolved_definition`；missing/unsupported/malformed definition 立即 fail closed，不读 filesystem。
2. 读取 `LoopStarted` row 并解码 v1 runtime snapshot，要求 snapshot `agent_id` 与 state pin 严格相等；计算 `base_event_seq = loop_started.event_seq - 1`。
3. 捕获本次 `through_event_seq = agent_states.last_event_seq`，读取 `(base,through]` 的完整 rows；该区间必须无 event_seq 缺行且全部属于 exact current Turn。
4. 在 base 以内选择 event_seq 最大且有效的 `transcript_compactions` row，以 `[summary] + MessageAppended[retained_from_event_seq..base]` 物化历史基线。
5. base 前没有 compaction 时直接从 ledger 起点 replay；已有 companion summary 但 locator/`retained_from_event_seq` 加速信息无效时，忽略加速信息并在内存中从起点 full replay。若存在 `TranscriptCompacted` durable discriminator 却缺少其必需 companion row、单一 summary 无法解码或二者 identity 不一致，则 durable truth 不完整，返回 `durable_state_corrupt`；不修表、不提供 rebuild API。
6. 将历史基线与 current Turn 的真实 LoopStarted/Hook/message/tool/compaction/iteration events 组成 typed replay window，交给 exact pinned runtime 的唯一 replay validator。

为了在 HTTP 202 前复用 kernel 现有 journal/Tool/compaction 校验而不复制状态机，唯一允许的新 kernel seam 是纯 `prepare_resume`：exact `Arc<AgentLoop>` 产生不可 Clone/Serialize、持有同一 definition runtime 的 opaque prepared value，后者只允许 consuming `run(token)`。它不得做 I/O、append、模型/Tool/Hook 调用，也不得接收 `AgentRuntimeId`、Postgres、Session或hosting。现有 resume 与该 seam 复用同一 private replay authority；除此之外不改变 fresh run、sink 或 Tool 串行语义。

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

审批 Handler 仍是 `decide_tool_call` 的普通 Handler，kernel 不理解 HTTP approval。`HookInvocationPending` 必须先 durable；Handler 以 exact Hook invocation确保/reuse server-generated UUIDv7 ApprovalId，并在 Requested payload保存最终复验后的 Tool name、CallId、Echo 的user-authored opaque arguments、typed `ToolKind`/`DangerLevel` 与 `hook_invocation_id`。当前composition没有credential reference/provider字段，也不在审批后注入credential；未来credential-aware Tool必须先完成独立安全PATCH才能注册。映射回 kernel 时可以忽略这些 composition metadata。

resolver 事务只按 `AgentRuntimeId` 锁 `agent_states`，先验证请求的exact Turn identity并查询Requested/Resolved/terminal ledger：terminal必须优先返回`approval_invalidated`；若尚未terminal，再要求state仍为running，相同决定返回204，相反决定返回`approval_already_resolved`，只有未决定Requested才追加唯一Resolved。state row lock已经串行化同一runtime writer，不增加approval row或第二套锁序。

waiter 使用 register-then-read-and-poll：先按 `(AgentRuntimeId,ApprovalId)` 注册本机 waiter，再重读PG；未Resolved时在同一cancellation-safe loop中等待Turn cancellation、本机notify或有固定内部上限的退避tick，notify与tick都触发ledger重读。resolver先commit再best-effort notify；通知/NATS丢失只增加最多一个bounded polling interval的延迟，不会永久挂起hosted Handler。waiter在决定、取消、shutdown或typed fail-closed错误后注销。hosted Handler收到approve映射Execute，reject映射Block并继续生成模型可见tool result；unhosted resolve只持久决定，必须显式resume。没有TTL，也不增加用户可配置的poll interval。

`AgentRuntimeView.pending_approvals` 在与 snapshot barrier 相同的 PG MVCC snapshot中查询 current Turn 的 Requested minus Resolved；Consumed/Invalidated不返回。浏览器当时未接收的 Requested在刷新后重新出现，Resolved不会再次要求人决定。

### 10. HTTP cold view、history 与稳定错误直接映射 ledger

运行态 resource 统一使用 `AgentRuntimeId`，不再把 template `AgentId` 伪装成对话身份：

```text
POST /v1/agent-runtimes
GET  /v1/agent-runtimes/{agent_runtime_id}
POST /v1/agent-runtimes/{agent_runtime_id}/messages
GET  /v1/agent-runtimes/{agent_runtime_id}/history
GET  /v1/agent-runtimes/{agent_runtime_id}/events
POST /v1/agent-runtimes/{agent_runtime_id}/resume
POST /v1/agent-runtimes/{agent_runtime_id}/cancel
POST /v1/agent-runtimes/{agent_runtime_id}/approvals/{approval_id}
```

`GET /v1/agent-runtimes/{agent_runtime_id}` 返回 `AgentRuntimeView` DTO，而不是数据库 row：必须显式包含 `agent_runtime_id`、pinned `agent_id`、通过 definition join 得到的 `agent_name`/`agent_version`、status、`model_config`、nullable Session/current Turn、`snapshot_event_seq`、`telemetry_floor_event_seq`、pending approvals、current Turn latest usage与 advisory `resume_required`。`/v1/agents/{agent_id}` 保留给 immutable definition resource，但本 change 不实现该读取或管理 endpoint。

create 使用独立的不可变 `AgentRuntimeCreated` DTO，字段只允许 `agent_runtime_id`、`agent_id`、`agent_name`、`agent_version` 与 runtime `created_at`；不得包含随后可变的 `model_config`、status、Session、Turn、usage、approval或barrier。相同 idempotency key 因此可以从 `agent_states + agents` 重新构造字节语义相同的 `201` body 与同一 `Location: /v1/agent-runtimes/{agent_runtime_id}`，而无需保存 creation snapshot。

`snapshot_event_seq`不是新状态字段，必须等于同一MVCC snapshot中读取的`agent_states.last_event_seq`；barrier-governed `telemetry_floor_event_seq` 是该 snapshot 内最新一个通过严格解码的 assistant `MessageAppended.event_seq`，不存在时为0。公开 `outcome` 删除。除 registry-derived advisory 外，definition pin、barrier、telemetry floor、status、usage和approvals来自同一PG snapshot。所有对外event sequence（view barrier、telemetry floor、history cursor/item与durable frame）统一编码为十进制字符串，避免JavaScript number精度改变identity。

`GET /v1/agent-runtimes/{agent_runtime_id}/history` 直接查询该 runtime 的 `durable_events` public-product partial index，不使用 message projection。必填 `through_event_seq`，可选 exclusive `before_event_seq` 与 limit；默认 50、最大 256，1 MiB soft page budget，首条自身超限仍完整返回。数据库反向取页，响应按 event_seq 升序并带 `next_before_event_seq/has_more`。History必须完整返回与SSE durable frame相同的安全`AgentRuntimeProductEventV1` union，排除ToolExecutionStarted/Hook journal等internal facts，从而为reconcile提供任意`(B,T]` product window；Tool result仍作为role=tool message。Web严格解码所有product：reconcile按序应用完整window；cold snapshot/向上分页则以view为current-state真相，只渲染message、compaction与安全terminal marker，旧control facts不得回写当前status/pending/draft/barrier，pagination cursor也不因渲染过滤改变。API绝不直接暴露raw durable payload。

成功语义固定：message与new resume返回202及AgentRuntime/Session/Turn IDs；cancel signal返回空body的202；already hosted/starting resume、exact already-cancelled、approval first/same retry均返回空body的204；create返回201 + Location。approval resolve与resume是不同endpoint。

所有 JSON request body硬限制 64 KiB，超限返回 413。library errors用 `thiserror`，HTTP统一映射安全 envelope `{"error":{"code":"...","message":"..."}}`。至少区分：

- 400：invalid request/cursor/history query
- 404：runtime route 使用 `agent_runtime_not_found`；create catalog lookup 使用 `agent_template_not_found`；approval lookup 使用 `approval_not_found`
- 409：stale turn、`agent_runtime_busy`、`agent_version_conflict`、resume required、session mismatch/busy、turn not running/hosted/starting、preamble incomplete、approval resolved/invalidated、runtime incompatible
- 410：cursor expired
- 413：request too large
- 422：`invalid_agent_version`、invalid template/model parameters
- 500：durable state corrupt、internal error
- 503：store/runtime/realtime unavailable、service shutting down

错误正文不包含 SQL、NATS subject、filesystem host path、prompt、Tool arguments/result、provider正文或 credential；来源链只在真正处理边界记录一次。

若 `agent_states` 存在但 pinned `agents` row 缺失、version metadata不一致或definition无法严格解码，必须返回 `500 durable_state_corrupt`，不能伪装成 `agent_runtime_not_found`、`agent_template_not_found` 或尝试重读filesystem。

### 11. AgentRuntimeStreamFrameV1 与 PG snapshot 形成端到端前端恢复

旧 `EventStreamBus`、`StreamEnvelope`、`RuntimeEvent/AgentEvent` transport DTO和公开 Session stream整体删除。新协议由 API拥有：

```text
AgentRuntimeStreamFrameV1
  protocol_version = 1
  kind = control | durable | telemetry
  agent_runtime_id
  agent_id
  session_id? / turn_id?
  created_at

  control: stream_ready | stream_reset { reason: buffer_overflow }
  durable: event_seq, event_version, AgentRuntimeProductEventV1
  telemetry: durable_before_event_seq, llm_call_id, telemetry_seq, typed LLM event
```

idle AgentRuntime可订阅，因此 control frame的 Session/Turn可以为空；所有 frame 必须携 exact `agent_runtime_id` 与该 state pinned `agent_id`，Turn durable frame还必须携完整 Session/Turn identity。任何存在的SSE id都只是不透明 NATS cursor，不能与 event_seq/telemetry_seq比较或持久化为业务状态。API只有在 exact AgentRuntime 的 NATS subscription已经建立并开始buffer后才发 `stream_ready`，客户端收到后才读取 PG cold snapshot。`stream_reset` 是 API 在单条已建立 SSE 上本地产生的控制信号，不写PG、不发NATS，也不携带SSE id。

durable frame 的稳定 identity、排序和去重键只能是 `(AgentRuntimeId,event_seq)`；telemetry fence 至少是 `(AgentRuntimeId,llm_call_id,telemetry_seq)`。frame中的 `agent_id` 只是 immutable definition pin，绝不能用于subject、dispatcher map、history barrier或事件去重。Web在应用任何frame前必须同时验证`agent_runtime_id`等于当前resource且`agent_id`等于AgentRuntimeView pin；任一不匹配都不得应用该frame，应关闭stream并以protocol identity error触发无cursor cold bootstrap，若新view仍不匹配则显示错误而不是循环重连。

每条telemetry在进入dispatcher bounded queue时冻结当时已知的PG durable high-water，并以十进制字符串`durable_before_event_seq`公开。dispatcher在发布该telemetry前先flush到该watermark。它只说明“这条telemetry排在这些durable facts之后”，不分配新的durable sequence，也不改变`(llm_call_id,telemetry_seq)` identity。若PG reconcile先应用了event_seq为F的assistant final，则随后到达且`durable_before_event_seq < F`的telemetry属于该final之前的旧tail，必须忽略；`durable_before_event_seq >= F`的下一call telemetry不能仅因此前final而被丢弃。

NATS subject、dispatcher generation 与 cursor validity 全部绑定 exact `AgentRuntimeId`；同一个 `AgentId` 下两个 runtime 的 frame 绝不能进入同一 tail。无 cursor从当前 tail开始；有 cursor只继续仍保留的短 tail。cursor仅在当前页面内存保存；页面刷新不恢复它。cursor过期在建流前返回410；建流后的server buffer overflow发送无id的`stream_reset(reason=buffer_overflow)`并关闭连接。Web收到reset后必须主动关闭原EventSource，丢弃该连接的buffer、draft与cursor，阻止浏览器携旧Last-Event-ID自动短重连，并从无cursor cold bootstrap重新开始。NATS retention使用可配置的短 age/bytes/messages上限，不承担 durable history或跨重启补发。

cold bootstrap 固定为：

1. 建立并 buffer AgentRuntime SSE，等待 `stream_ready`。
2. 读取 AgentRuntimeView和 barrier内最新 history page。
3. 应用 PG snapshot，把`snapshot_event_seq`保存为只由成功PG snapshot/reconcile推进的`pg_confirmed_event_seq`，并用 `AgentRuntimeView.telemetry_floor_event_seq` 初始化已收敛 assistant final floor，不能只靠最新 history page推导；durable frame `<= barrier` 跳过，`> barrier` 按 event_seq应用但不推进PG-confirmed barrier。
4. bootstrap期间buffered telemetry全部丢弃，因为没有可证明完整的call prefix。
5. bootstrap成功后才提交最新NATS cursor并进入live mode。

正常 reconcile是增量的：Web维护只由成功PG snapshot/reconcile推进的`pg_confirmed_event_seq=B`，并保留B之后尚未PG确认的realtime durable product map；NATS product可以先投影但不得推进B，因为较大event_seq不能证明较小公开product没有publish失败。新view给出T时，从history反向分页直到越过B，读取完整`(B,T]` product window。reducer提交时读取当前未确认map，而不是请求发起时的旧副本：先把PG window与`<=T`的realtime frame按`(AgentRuntimeId,event_seq)`去重成base，再在view@T之上按event_seq重放所有`>T` frame，最后按rebase后的exact Turn/status/final floor处理transient telemetry；原子成功后才把B推进到T、删除`<=T` entries并保留`>T` entries。若期间snapshot/recovery generation已替换，旧action直接作废。这样较大的frame不会掩盖先前丢失的较小product，慢view也不会回滚期间到达的terminal、approval或next-Turn状态。随后替换status/pending approvals等barrier-governed字段，包括用new view与重放frames推进已收敛assistant final floor；rebase后exact Turn仍running时保留同Turn telemetry draft，最终terminal才执行draft/未完成Tool UI清理。message 202 的exact accepted Turn保留到 AgentRuntimeView 或同一 Turn 的 exact durable `LoopStarted`/terminal product frame 证明，避免并发cold snapshot仍是旧idle/terminal时停止轮询；accepted Turn存在时只接收该Turn的telemetry，否则只接收running AgentRuntimeView的current Turn，防止上一Turn的NATS backlog复活draft。进入ready后立即reconcile。running、accepted/cancel待确认、realtime degraded或pending approval期间低频reconcile，窗口聚焦和每个command后立即reconcile。reconcile采用single-flight并把并发timer/focus/command合并成至多一次补跑，不能取消慢分页形成livelock。仅页面刷新、cursor过期、overflow或手动硬重置做cold rebuild。

当前每Turn只有一个active LLM call。统一per-AgentRuntime realtime dispatcher保证该call telemetry先于其final durable assistant message，final message关闭draft，下一call之后才开始；无需把 `llm_call_id` 写进kernel message。满队列的 durable wake 只推进单调 high-water；coalesced flush 必须先 snapshot target、再确认 accepted queue 为空，且只 flush 这个旧 snapshot，之后推进的 target 留给下一次 drain/idle 循环并在 idle 退休前追平。正常NATS顺序下按closed call忽略迟到telemetry；PG reconcile抢先看到final时按上述durable watermark区分旧call tail与final之后的新call。terminal event清空未闭合draft，并把无result的实时tool UI标为interrupted，不伪造result。

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

独立scheduler PATCH/TODO必须记录：ownership lease/fencing、多实例hosting、rolling deployment、自动takeover/resume、durable cancel、Agent/Workflow Session协调，以及用scheduler ownership/placement替换`resume_required`的process-local来源。独立template-management TODO只记录catalog CRUD、显式版本浏览/发布/提升/回滚和既有 AgentRuntime upgrade；create 时自动物化不可变 version 与多 runtime 复用已是当前 change 的必备语义。

## Risks / Trade-offs

- **[同一 version tag 被并发物化或被作者复用于不同定义]** → `stratum-postgres` 在创建事务内按 exact `(name,version)` 使用 transaction-scoped advisory lock，并严格比较已有 canonical definition；相同定义复用、不同定义返回 `agent_version_conflict`，`UNIQUE(name,version)` 兜底，不建立大 JSON unique index。该锁只串行化同一 tag 的 create，不阻塞其他 tag 或既有 AgentRuntime 命令。
- **[Idempotency-Key 被误用于不同创建意图]** → key-only 契约一律返回首次创建结果，Web 对每个新意图生成 UUID 并只在结果不确定时复用；不为了侦测调用方误用而保存第二份 immutable model request。
- **[immutable template versions 与 runtime states 持续增长]** → 两者与原始 durable messages 都是当前明确永久保留的核心资产，不增加 delete/cascade/retention API；未来若需要资产治理，必须以独立数据生命周期 change 定义。
- **[AgentRuntime state row成为单runtime写热点]** → 当前kernel串行且事务短；不同AgentRuntime互不阻塞，未来并发Tool出现后再测量。
- **[NATS commit后发布会丢失或持续失败]** → PG/history/AgentRuntimeView是恢复真相，前端增量reconcile；当前不增加outbox。无live producer的失败generation只做有界retry，随后显式丢弃volatile tail并退休，不让历史AgentRuntime task无界增长。
- **[cancel intent会在进程崩溃时丢失]** → 这是明确的当前限制，scheduler PATCH负责durable cancel；API不伪造cancelled。
- **[Tool started后结果未知会重复外部副作用]** → 复用同一CallId并明确at-least-once；外部服务自行实现幂等，runtime不定义通用标准。
- **[compaction companion 数据异常]** → locator/retained pointer 这类加速信息异常时忽略并做内存 full replay；缺少必需 companion/summary 或 summary 本身损坏意味着 durable truth 不完整，必须 fail closed；两种情况都不在线 repair。
- **[process-local hosting不能支持多实例]** → 不声明部署/rolling保证；引入第二实例前必须完成scheduler lease/fencing change。
- **[破坏性baseline无法保留beta执行数据]** → 部署前明确备份或丢弃，旧binary回滚必须同时重建旧DB/NATS。
- **[删除 `AgentVersionId` 会触及 core crate]** → runtime snapshot 以 `agent_id: AgentId` 机械取代旧 definition identity；`AgentRuntimeId` 只存在于 API/storage/infra 组合边界，不进入AgentLoop。除此之外只删除无生产消费者的旧transport surface，保留durable/telemetry sink、approval variants和Tool串行语义。

## Migration Plan

1. 先修订`CONSTITUTION.md` §1/§5/§8、`ARCH.md`、`TODO.md`、`CONTEXT.md`与相关`AGENTS.md`，正式确定concrete `stratum-postgres`所有权、Postgres core readiness/NATS realtime degradation边界和scheduler/template后续PATCH。
2. 将`add-postgres-execution-storage`移入archive并标记superseded，保证其delta specs不再同步为目标架构。
3. 删除旧beta migration，写单一最终baseline；部署/测试显式drop并重建数据库及sqlx migration history，不写数据转换器。
4. 在`stratum-config`与template catalog加入必填validated string version tag，更新catalog DTO、全部template fixtures/examples与invalid-tag边界测试；runtime create request仍不接收version。
5. 在`stratum-postgres`实现四张复数表、`agents 1 -> N agent_states`、exact `(name,string version tag)` 的原子物化/复用/冲突判定、state-owned key/`model_config`、concrete commands/queries、AgentRuntime-wide durable append、compaction与ledger-derived reads，并用真实PG integration tests验证同tag同定义复用、同tag异定义冲突、异tag同定义独立、并发 create、version pin 和独立runtime ledger。
6. 删除`AgentVersionId`，新增`AgentRuntimeId`；重写`stratum-api` assembly/runtime orchestration、create/view DTO、definition-aware preflight、process registry、approval Handler、resume/cancel与typed HTTP DTO。kernel只把snapshot definition pin机械改为`AgentId`并保留纯prepare seam，不接收`AgentRuntimeId`。
7. 切换NATS到AgentRuntime-scoped短tail与API-owned frame，更新Web identity、cold bootstrap、增量reconcile、审批、压缩marker和按需history。
8. 删除`stratum-store`、`stratum-agent-builtin`、filesystem execution/CAS、旧bus/sequence/config/tests/docs；用`rg`证明无生产残留。
9. 更新OpenAPI、Docker/config examples和运维说明；运行Rust/Web/PG/NATS测试、OpenSpec strict validation，再派发独立constitution-review子代理逐条审完整diff。

Rollback不承诺保留新beta数据；回到旧binary必须同时恢复旧schema/config/NATS，不能只回滚应用。

## Open Questions

无。`AgentId`、`AgentRuntimeId`、template-authored string `version` tag、exact tag 冲突语义、运行态 route 与四张复数表均已定死；scheduler、完整Session和template管理已明确延期，不阻塞本change。
