# runtime-event-protocol Specification

## ADDED Requirements

### Requirement: AgentStreamFrameV1 是唯一公开 Agent Realtime Frame
公开 Agent realtime 协议必须（SHALL）由 `stratum-api` 拥有并使用 `AgentStreamFrameV1`。每个 frame 必须（SHALL）包含 `protocol_version=1`、`kind`、`agent_id` 与 `created_at`，并可以按 variant 包含 `session_id` 与 `turn_id`。`kind` 必须（SHALL）是以下封闭集合之一：

- `control`：事件为 `stream_ready`，或仅由当前 SSE 连接本地产生的 `stream_reset { reason: "buffer_overflow" }`；
- `durable`：包含 `event_seq`、`event_version` 与 typed `AgentProductEventV1`；
- `telemetry`：包含 `llm_call_id`、`telemetry_seq` 与 typed LLM telemetry event。

Turn-scoped durable 与 telemetry frame 必须（SHALL）携带完整 Session/Turn identity；idle Agent 的 `stream_ready` 与连接级 `stream_reset` 可以（SHALL）省略这两个 identity。`stream_reset` 不得（SHALL NOT）写入 Postgres、发布到 NATS 或携带 SSE `id`。协议不得（SHALL NOT）直接序列化 kernel event、Postgres raw payload 或旧 transport DTO。未知 `protocol_version` 或未知 frame variant 必须（SHALL）被拒绝，不得静默按 v1 猜测。

#### Scenario: Idle Agent 建立 Control Frame
- **WHEN** 已创建但尚未绑定 Session 的 Agent 建立 realtime subscription
- **THEN** API 发送 `protocol_version=1`、正确 AgentId 且 Session/Turn 可空的 `stream_ready` frame

#### Scenario: 连接级 Reset 不污染 Tail
- **WHEN** API 必须终止一个已建立但本地 buffer 已溢出的 SSE
- **THEN** API 直接在该连接发送无 SSE id 的 `stream_reset { reason: "buffer_overflow" }`，且该 control frame 不进入 NATS 或 durable ledger

#### Scenario: Durable Turn Frame 身份完整
- **WHEN** current Turn 的 committed product event 被发布
- **THEN** durable frame 包含 AgentId、SessionId、TurnId、event_seq、event_version 与 typed product event

#### Scenario: Telemetry Frame 与 Durable Frame 可判别
- **WHEN** consumer 解码同一 Agent stream 上的 LLM delta 与 committed message
- **THEN** `kind` 分别明确选择 telemetry 与 durable shape，consumer 不读取 arbitrary metadata 来猜测语义

#### Scenario: 不支持的 Frame Version
- **WHEN** consumer 收到非 v1 `protocol_version`
- **THEN** consumer 拒绝该 frame并停止按 v1 应用，不猜测未知版本字段

### Requirement: Durable Product Event 使用 Agent-wide Event Sequence
每个公开 durable frame 必须（SHALL）使用对应 Postgres durable row 的 Agent-wide `event_seq`，其稳定 identity 与顺序 key 必须（SHALL）为 `(AgentId,event_seq)`。JSON 必须（SHALL）把 `event_seq` 编码为无符号十进制字符串，避免 JavaScript number 精度改变 identity。`event_version` 必须（SHALL）来自 durable row；v1 API mapper 必须（SHALL）把支持的 row转换为 typed `AgentProductEventV1`，不得把 variant-only raw JSON 直接透传。

公开 product union 必须（SHALL）只包含安全映射后的 `LoopStarted`、`MessageAppended`、`ToolApprovalRequested`、`ToolApprovalResolved`、`TranscriptCompacted`、`IterationCompleted`、`LoopFinished`、`LoopFailed` 与 `LoopCancelled`。`ToolExecutionStarted`、Hook invocation journal 与其他 internal durable facts不得（SHALL NOT）发布，因此相邻公开 frame 的 event_seq 可以存在数值间隔；该间隔不是丢帧证据。History 必须（SHALL）复用同一 typed union/version，但可以只选择其中公开 history 所允许的 variants。

#### Scenario: Committed Message 使用原 Event Sequence
- **WHEN** `MessageAppended` 的 Postgres 事务已提交
- **THEN** 后续 durable frame 使用该 row 的 event_seq与event_version，且 publish 不得先于 commit

#### Scenario: Durable Frame 去重
- **WHEN** client 重复收到同一 `(AgentId,event_seq)`
- **THEN** client 忽略后到 frame，不创建第二份 durable UI state

#### Scenario: Internal Hook Event 形成可见序号间隔
- **WHEN** 两个 product events 之间提交了 Hook journal rows
- **THEN** 后一个 durable frame 保留其真实 event_seq，client 不因公开序号不连续而报告 durable corruption

#### Scenario: Tool Result 不需要第二套序号
- **WHEN** Tool 成功或失败并提交最终 `MessageAppended(role=tool)`
- **THEN** realtime 与 history 都使用该 message 的 event_seq，不生成 `ToolExecutionCompleted` 或 message_seq

#### Scenario: Raw Durable Payload 不对外暴露
- **WHEN** durable row 含 runtime snapshot、internal error source或其他非公开字段
- **THEN** API mapper只构造安全的 typed product variant，frame 不直接包含 raw payload

### Requirement: LLM Telemetry 使用 Call-local Sequence
LLM lifecycle 与 delta 必须（SHALL）使用 `(LlmCallId,telemetry_seq)` 作为 volatile identity。API-side `TelemetryEventSink` 必须（SHALL）在进入 bounded realtime queue 前，为同一 call 的 `LlmStarted`、每条 content/reasoning/tool delta 与 `LlmFinished` 从 0 开始单调分配 telemetry_seq。telemetry 不得（SHALL NOT）分配 durable event_seq、写入 Postgres 或取代最终完整 assistant message。

NATS Agent tail 必须（SHALL）保留这些 delta frame，从而在连接健康时提供低延迟 token streaming。consumer 必须（SHALL）按 llm_call_id 隔离 sequence：低于下一期待值的 frame 是重复并被忽略，高于下一期待值表示 draft prefix 不完整；transport cursor不得（SHALL NOT）被解释为缺失的 telemetry sequence。

#### Scenario: Delta 正常到达
- **WHEN** client 收到 active LLM call 下一期待的 telemetry_seq
- **THEN** client应用 typed delta并推进该 call 的 volatile frontier

#### Scenario: Delta 重复
- **WHEN** telemetry_seq 小于该 call 的下一期待值
- **THEN** client忽略重复 frame，不重复拼接文本

#### Scenario: Delta 出现间隔
- **WHEN** telemetry_seq 大于下一期待值
- **THEN** client将该 draft 标为 incomplete并等待 durable final message收敛，不从 NATS cursor 推导缺失内容

#### Scenario: Telemetry 不进入 Durable History
- **WHEN** LLM 产生大量 reasoning 或 content delta
- **THEN** Postgres durable frontier不因这些 delta增长，刷新后的完整内容只来自 committed message

### Requirement: Per-Agent Dispatcher 串行化 Product 与 Telemetry Tail
`stratum-api` 必须（SHALL）为每个本机活跃 Agent 使用统一的有序 realtime dispatcher。durable commit receipt 必须（SHALL）只在 Postgres commit 后进入 dispatcher；dispatcher 必须（SHALL）按 Agent-wide event_seq 从 Postgres读取并跨过 internal rows，再按递增顺序发布 product frames，不得直接以多个 writer 的 task wake order决定 NATS 顺序。

当前一个 Turn 同时只能（SHALL）有一个 active LLM call。dispatcher 必须（SHALL）保证该 call 的已接收 telemetry 先于对应 final durable assistant `MessageAppended` 发布，final message 之后才可发布下一 LLM call 的 start。NATS publish failure不得（SHALL NOT）回滚 Postgres、改变 kernel acknowledgement 或让 durable sink重复 append；本版本不得（SHALL NOT）增加 durable outbox。

#### Scenario: 两个 Durable Writer 的 Receipt 乱序
- **WHEN** approval resolver 与 kernel 的相邻 durable rows以相反 task wake order到达 dispatcher
- **THEN** dispatcher按 Postgres event_seq 发布 product frames，NATS 不把较大 event_seq 先交付

#### Scenario: Final Assistant 收敛当前 Draft
- **WHEN** active call 的 final assistant message已 durable commit
- **THEN** dispatcher先交付已排队 telemetry再交付 final durable frame，client用完整 message替换 draft并关闭该 call

#### Scenario: 下一 Call 不与上一 Draft 交错
- **WHEN**一个 iteration 的 final assistant message关闭当前 call，Agent 随后启动下一次 LLM call
- **THEN** 下一 `LlmStarted` 不会先于上一 call 的 final durable frame发布

#### Scenario: Publish 失败不改变 Durable 结果
- **WHEN** Postgres commit 成功后 NATS publish 失败
- **THEN** sink/kernel/HTTP 仍确认 durable结果，client以后通过 AgentView/history reconcile恢复

### Requirement: NATS 只保留 Agent-scoped 短期 Tail
NATS 必须（SHALL）按 AgentId 分区 durable product 与 telemetry frames，并通过 `stratum-infra` 的窄 Agent-tail transport API提供 publish/subscribe；系统不得（SHALL NOT）恢复旧的通用 Session `EventBus` abstraction。retention 必须（SHALL）同时使用可配置且有限的 age、bytes 与 message-count 短期上限，并淘汰旧数据；NATS 不得（SHALL NOT）作为 durable history、approval truth、resume replay、跨重启补发或 Session 多 Agent 聚合来源。

无 cursor 的 subscription 必须（SHALL）从当前新 tail开始。服务启动时 volatile publish frontier 必须（SHALL）初始化到已存在的 Postgres high-water，不能因重启把全部旧 product events重新发布；启动后的遗漏由 PG cold recovery处理。旧 `replay=all`、`replay=new` 与公开 Session stream参数不得（SHALL NOT）继续受支持。

#### Scenario: 无 Cursor 只订阅新 Tail
- **WHEN** client为一个 Agent建立无 cursor subscription
- **THEN** NATS只交付 subscription 建立后的新 frames，不重放该 Agent全部 retained或durable history

#### Scenario: Delta 保留在短窗口
- **WHEN**已连接页面暂时落后但 cursor仍在 retention内
- **THEN** subscription可以继续得到保留的 product与LLM delta frames

#### Scenario: Publisher 重启不补发旧历史
- **WHEN** API 重启且 Postgres已有大量旧 event rows
- **THEN** volatile frontier从启动 high-water开始，旧 rows由 AgentView/history提供而不重新灌入 NATS

#### Scenario: 旧 Replay 参数被拒绝
- **WHEN** client提供 `replay=all`、`replay=new` 或请求公开 Session event stream
- **THEN** API拒绝旧协议参数或路由，不把短 retention 暴露为 durable replay API

#### Scenario: 同一 Session 的 Agent 分开订阅
- **WHEN**两个 Agent关联同一 Session并分别产生 events
- **THEN** consumer按各自 AgentId订阅，两者不会被公开 Session stream混合

### Requirement: Stream Ready 建立明确的 Subscribe-before-Snapshot 屏障
Agent SSE 必须（SHALL）先建立 NATS subscription并开始服务端 buffering，再发送 `stream_ready` control frame。client 必须（SHALL）在收到该 frame后才读取 Postgres `AgentView` 与 history。任何存在的SSE `id`必须（SHALL）是Agent tail的不透明transport cursor；cursor只能（SHALL）保存在当前页面内存，不能与 event_seq/telemetry_seq 比较，也不得跨页面刷新持久化。

仍在 retention内的 cursor只能（SHALL）继续其后的短 tail；过期 cursor或 server buffer overflow必须（SHALL）终止当前增量路径并触发新的 cold bootstrap，不能静默跳过中间数据。cursor 过期必须（SHALL）在 SSE 建立前使用 HTTP 410 表达；SSE 已建立后的 buffer overflow 必须（SHALL）发送无 SSE `id` 的 `stream_reset { reason: "buffer_overflow" }` 后主动关闭连接。无 cursor时必须（SHALL）从当前 tail开始，而不是从 NATS history起点开始。

#### Scenario: Snapshot 前 Subscription 已可收事件
- **WHEN** client收到 `stream_ready` 并随后发起 AgentView/history请求
- **THEN**从 subscription 建立到 PG snapshot返回期间产生的 frames已进入 buffer，不存在 subscribe-after-snapshot race

#### Scenario: 页面刷新不复用旧 Cursor
- **WHEN**浏览器刷新或重新进入 Agent页面
- **THEN** client丢弃旧页面 cursor，建立无 cursor新 tail并执行 cold bootstrap

#### Scenario: Cursor 只决定 Transport 位置
- **WHEN** client用 retention内 cursor重连同一页面
- **THEN** NATS只用 cursor选择 tail位置，durable去重仍用 event_seq，telemetry去重仍用 `(llm_call_id,telemetry_seq)`

#### Scenario: Cursor 过期
- **WHEN**NATS已淘汰 cursor位置
- **THEN** SSE返回 `410 cursor_expired`，client清除 transient state并重新执行无 cursor cold bootstrap

#### Scenario: Server Buffer Overflow
- **WHEN** cold bootstrap期间 Agent tail超过服务端有界 buffer
- **THEN** server发送无 SSE id 的 `stream_reset { reason: "buffer_overflow" }` 后关闭连接
- **AND** client关闭原 EventSource、丢弃该次 buffer、transient draft 与 page-memory cursor，不允许浏览器携旧 Last-Event-ID 自动短重连
- **AND** client建立无 cursor 的新 subscription并重新执行 cold bootstrap

### Requirement: Web Cold Bootstrap 以 PG Snapshot Barrier 收敛
Web 必须（SHALL）按以下顺序执行 cold bootstrap：

1. 建立并 buffer Agent SSE，等待 `stream_ready`；
2. 读取 `AgentView`，再以其 `snapshot_event_seq` 作为固定 `through_event_seq`读取最新 history page；
3. 应用 PG view与history，对 buffered durable frame跳过 `event_seq <= barrier`，仅按 event_seq应用 `event_seq > barrier`；
4. 丢弃 bootstrap期间所有 buffered telemetry，因为 client无法证明拥有完整 call prefix；
5. 只有 view、history 与 buffered durable merge全部成功后，才提交最新 NATS cursor并进入 live mode。

Cold bootstrap不得（SHALL NOT）要求加载全部历史。原始旧消息只能（SHALL）在用户向上滚动且需要时继续以同一 through barrier分页。

#### Scenario: Snapshot 请求期间产生 Durable Event
- **WHEN** subscription已 ready而PG snapshot返回前提交了新 product event
- **THEN** event要么已包含在 barrier内并从 buffer去重，要么高于 barrier并从 buffer应用，不丢失也不重复

#### Scenario: Bootstrap Telemetry 没有完整 Prefix
- **WHEN** buffer包含一个已进行中 LLM call 的部分 delta
- **THEN** client丢弃这些 telemetry，不用残缺 draft覆盖 PG committed history

#### Scenario: Bootstrap 失败不提交 Cursor
- **WHEN** AgentView、history或buffer merge失败
- **THEN** client不保存最新 cursor，修复连接后重新执行 cold bootstrap

#### Scenario: 首屏只加载最新一页
- **WHEN**对话包含大量永久历史
- **THEN** cold bootstrap只加载固定 barrier内最新一页，旧页在向上滚动时按需请求

### Requirement: Web Reconcile 只增量补齐 Durable Gap
进入 live mode 后，Web 必须（SHALL）保存已应用的 durable barrier B。下一次 reconcile读取新 `AgentView` 得到 barrier T 时，client 必须（SHALL）用 `through_event_seq=T` 从最新 history反向分页，直到页面越过或到达 B，只应用 `(B,T]` 的公开 product items，并用新 view替换 status、pending approvals、latest usage与其他 barrier-governed fields。只要exact current Turn仍为running，已有telemetry draft必须（SHALL）保留，不能因普通reconcile清空；若新view证明该Turn已经terminal，则必须（SHALL）执行与terminal frame相同的draft和未完成Tool UI清理。

running 或存在 pending approval期间必须（SHALL）进行低频 PG reconcile；窗口重新获得焦点以及每个 command 返回后必须（SHALL）立即 reconcile。只有页面刷新、cursor expired、buffer overflow或用户明确 hard reset才可（SHALL）做完整 cold rebuild。

#### Scenario: NATS 漏掉 Durable Frame
- **WHEN** durable event已提交但publish失败，旧 barrier为B且新AgentView barrier为T
- **THEN** reconcile从history补入 `(B,T]` 可见items并推进barrier，不重建已加载的全部对话

#### Scenario: Running Reconcile 保留 Active Draft
- **WHEN** live telemetry draft存在且reconcile后exact Turn仍running，只发现approval或其他PG字段变化
- **THEN** client更新PG-backed fields但保留active draft，等待final durable message收敛

#### Scenario: Reconcile 发现 Terminal
- **WHEN** terminal realtime frame丢失，但新AgentView证明exact current Turn已finished、failed或cancelled
- **THEN** client清除未闭合draft并把无result Tool UI标为interrupted，不等待NATS补发

#### Scenario: Approval Resolve 后立即 Reconcile
- **WHEN** approval endpoint返回204
- **THEN** client可先移除已决 pending item并立即读取 AgentView确认；若 Turn unhosted则显示 Resume，不自动调用resume

#### Scenario: Cancel Accepted 不提前写 Terminal UI
- **WHEN** cancel endpoint返回202但PG尚无terminal event
- **THEN** client只显示取消请求已发送，直到 reconcile或durable frame确认真实terminal status

#### Scenario: Compaction 作为可折叠 Marker
- **WHEN** history/reconcile收到 `TranscriptCompacted`
- **THEN** Web渲染可折叠“上下文已压缩”marker并可展开完整summary，不伪装system message或显示全局banner

### Requirement: Final Durable Message 与 Terminal Event 收敛 Volatile UI
Web 必须（SHALL）把当前唯一 active LLM call 的 final durable assistant `MessageAppended` 视为 draft truth：完整 message替换 volatile draft并关闭该 llm_call_id。closed call的迟到 telemetry必须（SHALL）忽略，不得重新创建 draft。任一 current Turn terminal product event或同barrier AgentView中的terminal status必须（SHALL）清除未闭合 draft，并把没有 final result的实时 Tool UI标为 interrupted；client不得（SHALL NOT）伪造 tool result。

#### Scenario: Final Message 替换 Incomplete Draft
- **WHEN** telemetry出现gap后收到对应完整 assistant `MessageAppended`
- **THEN** client以 durable内容整体替换draft、清除incomplete标记并关闭当前call

#### Scenario: Final 后迟到 Delta
- **WHEN** call已由final durable message关闭后又收到其delta或`LlmFinished`
- **THEN** client忽略迟到 frame，不改写已提交 message

#### Scenario: Terminal 清理未完成 UI
- **WHEN** Turn提交finished、failed或cancelled terminal event时仍有draft或无result Tool card
- **THEN** client清除draft并把无result Tool标为interrupted，不创造assistant文本或tool result

### Requirement: Realtime Degraded 不改变 Postgres Core Availability
Postgres 必须（SHALL）是 Agent create/read/history/message/resume/cancel/approval 的核心 readiness依赖；NATS 只决定 realtime tail availability。NATS无法连接、subscription失败或publish失败时，系统必须（SHALL）把 realtime标为 degraded，SSE建立返回 `503 realtime_unavailable`，但只要Postgres及对应runtime依赖可用，核心commands必须（SHALL）继续工作。Web必须（SHALL）显示克制的 realtime degraded状态并使用 AgentView/history reconcile，不能把NATS故障显示为 durable command失败。

#### Scenario: NATS Down 但 Message 可提交
- **WHEN**NATS不可用而Postgres和runtime可用，client提交新message
- **THEN**API仍按durable合同返回202；Web通过PG reconcile看到结果并显示realtime degraded

#### Scenario: SSE 无法建立
- **WHEN**Agent SSE subscription无法连接NATS
- **THEN**SSE返回`503 realtime_unavailable`，read/history与其他PG-backed endpoints保持可用

#### Scenario: Post-commit Publish Failure
- **WHEN**一个已确认HTTP或kernel操作的durable frame发布失败
- **THEN**操作结果不回滚、不改写为失败，后续PG reconcile收敛client

#### Scenario: Postgres Down 是 Core Unavailable
- **WHEN**Postgres无法提供要求的durable read或transaction
- **THEN**对应核心endpoint返回`503 store_unavailable`，不得从NATS猜测durable状态

## MODIFIED Requirements

### Requirement: 删除 Run 事件语义
公开 Agent realtime协议不得（SHALL NOT）暴露 `RunId`、run lifecycle variant或把长期 Agent/Session解释为一次性run。`LoopFinished`、`LoopFailed`与`LoopCancelled`必须（SHALL）只终结其 exact Turn，并更新Agent current/recent Turn status；terminal后同一Agent必须（SHALL）仍可在相同绑定Session中通过新message开始新Turn。

#### Scenario: Agent Turn 结束
- **WHEN**Agent在仍可复用的Session中完成、失败或取消一个Turn
- **THEN**durable product event标识Agent/Session/Turn，且不会发出Agent或Session永久结束的run event

#### Scenario: Terminal 后开始新 Turn
- **WHEN**客户端以exact current-Turn CAS向terminal Agent发送后续message
- **THEN**系统创建新Turn并复用Agent/Session identity，不需要新的RunId

### Requirement: Event cursor 仅用于传输
SSE `id`/`EventCursor` 必须（SHALL）是不透明、短期且Agent-scoped的NATS transport position。runtime、history、Hook、approval、resume与Web durable state不得（SHALL NOT）把cursor和event_seq或telemetry_seq比较，也不得持久化cursor作为业务状态。cursor只可（SHALL）在当前页面内继续仍被保留的tail；过期必须（SHALL）显式返回410并转为PG cold bootstrap。

#### Scenario: SSE 短期重连
- **WHEN**当前页面使用仍在retention内的cursor重连同一Agent stream
- **THEN**transport从该position之后继续tail，业务去重分别使用event_seq或call-local telemetry_seq

#### Scenario: SSE Cursor 过期
- **WHEN**NATS已淘汰cursor对应position
- **THEN**API返回410，consumer不从cursor推导durable history，而是清除cursor并执行PG cold bootstrap

#### Scenario: 页面刷新
- **WHEN**浏览器刷新或稍后重新进入Agent
- **THEN**client不恢复旧cursor，使用新的subscription加PG snapshot/history恢复

### Requirement: Metadata 不携带主要语义
`AgentStreamFrameV1` 不得（SHALL NOT）包含任意扩展的通用 metadata map。Agent、Session、Turn、event/call sequence与variant必须（SHALL）由typed frame字段直接表达；consumer无需读取metadata即可确定identity、ordering与event kind。frame不得（SHALL NOT）记录secret、credential、host path、raw prompt、未经公共API映射的Tool arguments/result、provider body或内部error source。

#### Scenario: 核心 UI 投影
- **WHEN**client投影durable或telemetry frame
- **THEN**只使用typed frame和event variant即可确定归属、顺序与UI行为

#### Scenario: 敏感内部字段被过滤
- **WHEN**durable row或runtime error含非公开内部字段
- **THEN**API mapper在构造typed product event时省略或安全转换这些字段，不通过metadata旁路泄露

### Requirement: Beta 协议变更必须明确
runtime必须（SHALL）拒绝旧run-oriented、Session-scoped公开Agent stream、`StreamEnvelope`、旧`RuntimeEvent`/`AgentEvent`、`EventRecord`或message_seq payload，不得（SHALL NOT）实现双读、双写、自动转换或回滚兼容路径。部署必须（SHALL）删除旧NATS stream/consumer并以新Agent-scoped v1协议重新初始化；旧beta realtime数据由丢弃和PG cold recovery处理，不进行transport migration。

#### Scenario: 收到旧 Session Envelope
- **WHEN**入站payload包含`run_id`、公开Session stream identity或message_seq，但缺少v1 Agent frame字段
- **THEN**runtime以unsupported protocol拒绝，不合成AgentId、event_seq或telemetry_seq

#### Scenario: 部署发现旧 NATS Stream
- **WHEN**新版本部署环境仍存在旧 Session-scoped beta stream/consumer
- **THEN**部署流程重建realtime资源，不尝试把旧cursor或records转换到Agent tail

## REMOVED Requirements

### Requirement: Runtime envelope 以 Session 为作用域
**Reason**: 旧 `StreamEnvelope` 让公开订阅以 Session 聚合多个 Agent，并把API transport语义放进通用core DTO；新协议严格按AgentId订阅。
**Migration**: 删除 `StreamEnvelope` 及公开Session SSE，使用API-owned `AgentStreamFrameV1`和Agent-scoped endpoint。

#### Scenario: 旧 Session Envelope 不再可用
- **WHEN**client请求或发送旧Session-scoped `StreamEnvelope`
- **THEN**系统拒绝旧协议，client必须按AgentId建立v1 stream

### Requirement: 事件归属由事件变体编码
**Reason**: 旧 `RuntimeEvent` 的Session/Node/Agent transport union随旧EventBus整体删除；公开Agent协议不再承载Workflow/Node聚合事件。
**Migration**: Agent归属由`AgentStreamFrameV1`顶层typed identity和`AgentProductEventV1`/telemetry variant表达；Workflow/Node未来协议不通过此旧union兼容。

#### Scenario: 旧 RuntimeEvent Union 被拒绝
- **WHEN**public Agent endpoint收到旧Session/Node/Agent `RuntimeEvent` variant
- **THEN**endpoint不解码该union，要求v1 Agent frame

### Requirement: 完整消息序号表示 Agent 历史顺序
**Reason**: `message_seq` 与durable journal形成第二套frontier，无法统一message、approval、compaction和terminal顺序。
**Migration**: 所有committed Agent facts使用`(AgentId,event_seq)`；公开history筛选对应event rows，LLM delta使用`(LlmCallId,telemetry_seq)`。

#### Scenario: 旧 Message Sequence 不再生成
- **WHEN**提交user、assistant或tool message
- **THEN**系统只分配Agent-wide event_seq，不生成、存储或发布message_seq

### Requirement: EventBus 以 Session 为作用域
**Reason**: 通用Session EventBus和memory/NATS双实现扩大了composition surface，并与Agent-scoped恢复和短tail语义冲突。
**Migration**: 删除EventBus abstraction、Session partition和公开Session subscriber；`stratum-infra`只提供窄Agent-tail transport，Postgres承担durable recovery。

#### Scenario: 同 Session 多 Agent 不再聚合
- **WHEN**同一Session内两个Agent产生product或telemetry event
- **THEN**它们只进入各自Agent tail，不存在一个公开Session EventBus订阅同时接收两者
