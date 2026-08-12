# agent-runtime-api Specification

## Purpose
定义 Agent template catalog、AgentRuntime 创建与运行命令、Postgres 冷视图、历史查询和实时短 tail 的公共 HTTP 合同。
## Requirements
### Requirement: Template Catalog 是只读热目录
系统必须（SHALL）从 `[agent].templates_root` 暴露 `GET /v1/agent-templates`。服务启动时必须（SHALL）验证该路径存在、是目录且可读；空目录合法，服务不得（SHALL NOT）自动创建 template、definition 或 history 目录。每次 catalog 请求与 idempotency key 未命中的 runtime create 都必须（SHALL）读取当时 filesystem 中的 template TOML；catalog 中任一 template 无法读取、解析或校验时，整个 catalog 请求必须（SHALL）失败，不得返回部分结果。

每份 template TOML 必须（SHALL）由作者提供顶层 `version` 字符串 tag。tag 必须（SHALL）通过 UTF-8 长度 `1..=128` bytes、无控制字符且无首尾空白的校验；它大小写敏感、没有排序或 SemVer 语义。catalog DTO 必须（SHALL）返回创建界面需要的公开 name、version 与安全模型信息，不得（SHALL NOT）返回 prompt、tools JSON、raw TOML、host path、内容 digest 或其他 resolved definition 内容。缺失或无效 tag 必须（SHALL）使 catalog 返回 `422 invalid_agent_version`；其他 template shape 错误返回 `422 invalid_agent_template`。

filesystem template 只表示当前最新源。runtime create request 不得（SHALL NOT）接收或覆盖 version；既有 AgentRuntime 必须（SHALL）继续使用 `agent_states.agent_id` pinned 的 immutable definition，不得因 template 文件后续变化而改变。

#### Scenario: 空 Template 目录
- **WHEN** 已配置的 `templates_root` 可读但不包含任何 template 文件
- **THEN** 服务正常启动，`GET /v1/agent-templates` 返回 `200 OK` 与空目录

#### Scenario: Catalog 全有或全无
- **WHEN** catalog 中一个 template 有效而另一个 template 无法解析
- **THEN** `GET /v1/agent-templates` 返回 typed 422，且不泄露部分 catalog 或 host path

#### Scenario: Template 缺少 Version Tag
- **WHEN** 任一 TOML 缺少顶层 version、tag为空、过长、含控制字符或首尾空白
- **THEN** 整个 catalog 请求返回 `422 invalid_agent_version`，不把文件顺序或时间戳推断为版本

#### Scenario: Template 热更新使用新 Tag
- **WHEN** 运维者修改 template 定义并提供新 tag 后再次读取 catalog并创建 runtime
- **THEN** catalog反映当前文件，新 runtime pin新 `AgentId`，修改前创建的 runtime仍pin原定义

#### Scenario: Template Root 无效时拒绝启动
- **WHEN** `templates_root` 缺失、不是目录或启动时不可读
- **THEN** 服务启动失败，且不创建替代目录或回退到内置、filesystem execution 定义

### Requirement: 模型目录与完整覆盖具有稳定合同
系统必须（SHALL）通过 `GET /v1/models` 返回当前已配置且可选择的模型目录及其公开 `parameters_schema`。`POST /v1/agent-runtimes` 与 `POST /v1/agent-runtimes/{agent_runtime_id}/messages` 可以（SHALL）接受完整 `model_config` 覆盖；覆盖必须（SHALL）整体替换 provider、model 与 provider-specific parameters，不得（SHALL NOT）与原配置做字段 merge。系统必须（SHALL）在任何 durable mutation 前校验覆盖。resume 请求不得（SHALL NOT）接受模型覆盖，并必须（SHALL）使用目标 Turn 的固定 runtime snapshot。

create override 只必须（SHALL）初始化新 `agent_states.model_config`；未提供时使用 template 默认模型。template 默认模型保留在 immutable `resolved_definition` 中供 definition equality 与初始值使用，但 create override 不得（SHALL NOT）进入 definition equality。state 不得（SHALL NOT）保存第二个 creation/default model 字段。

后续 Turn override 只有在首条 user `MessageAppended` 成功提交且值与当前 `agent_states.model_config` 不同时，才可（SHALL）在同一事务更新该唯一字段。`LoopStarted` 必须（SHALL）始终固定本 Turn 的 effective model config；started-only Turn不得（SHALL NOT）改变 state model。

#### Scenario: 前端按 Schema 构造模型参数
- **WHEN** 客户端调用 `GET /v1/models`
- **THEN** API 返回 `200 OK` 与已配置模型的公开描述和 `parameters_schema`，客户端无需硬编码 provider 参数枚举

#### Scenario: 创建时完整覆盖模型
- **WHEN** runtime create 请求携带有效 `model_config`
- **THEN** 新 `agent_states.model_config` 使用该完整覆盖，而 immutable definition仍保存template默认模型

#### Scenario: 同一 Definition 创建不同模型的 Runtime
- **WHEN** 两个 create key基于同一 exact template 版本但提供不同有效model override
- **THEN** 两个 AgentRuntime复用同一 `AgentId`，各自在自己的`agent_states.model_config`保存初始配置

#### Scenario: 后续 Turn 覆盖模型
- **WHEN** terminal AgentRuntime 的 message 请求携带有效且不同的 `model_config`
- **THEN** `LoopStarted` snapshot固定完整覆盖，首条user message提交时才把它保存为后续Turn配置

#### Scenario: 相同模型不重复写入
- **WHEN** 新 Turn 的 effective model config 与 AgentRuntime 当前 `model_config` 完全相同
- **THEN** 首条 user message事务不执行冗余 model update，但 Turn snapshot仍固定该配置

#### Scenario: Started-only 不改变 State Model
- **WHEN** `LoopStarted` 使用不同覆盖，但首条 user message没有提交
- **THEN** `agent_states.model_config`保持旧值，started-only reconciliation不传播未接受的覆盖

#### Scenario: 无效模型覆盖
- **WHEN** create 或 message 请求引用未配置模型，或参数不符合该模型的schema
- **THEN** API返回`422 model_not_configured`或`422 invalid_model_parameters`，且不产生durable mutation

### Requirement: AgentRuntime 创建是 Key-only 幂等的纯持久化操作
系统必须（SHALL）通过 `POST /v1/agent-runtimes` 创建长期运行聚合。请求体必须（SHALL）且只能包含 `agent_name` 与可选完整 `model_config`，请求必须（SHALL）携带由客户端生成的 UUID `Idempotency-Key`。请求不得（SHALL NOT）包含 version、`AgentId`、`AgentRuntimeId`、user message、`SessionId` 或 `TurnId`；创建不得（SHALL NOT）调用模型、启动 `AgentLoop` 或生成 Turn event。

在完成请求大小、JSON语法、strict DTO shape与key格式等边界校验后，系统必须（SHALL）先按idempotency key查询`agent_states`，再执行create业务语义校验或读取template。key命中时必须（SHALL）无条件返回首次创建的同一runtime，不比较此次`agent_name`或model override，也不重新校验新的model；key是command identity，不是请求指纹。系统必须（SHALL）从`agent_states + agents`重构语义相同的`201 Created` body与同一`Location`。

key未命中时，系统必须（SHALL）热读并校验 template name与作者tag，完成definition/model/tool preflight，并构造不含create override的canonical `resolved_definition`。创建事务必须（SHALL）按 exact `(name, version)` 获取transaction-scoped advisory lock，再次检查key，并执行以下唯一规则：

- pair不存在时插入新的immutable `agents` row与新`AgentId`；
- pair存在且`definition_schema_version + resolved_definition`严格相等时复用原`AgentId`；
- pair存在但定义不同时返回`409 agent_version_conflict`并回滚；
- tag不同即使定义相同也插入新的`agents` row。

事务必须（SHALL）生成新`AgentRuntimeId`，原子提交可能的新definition row与引用它的idle `agent_states` row，并以完整create override或template默认值初始化唯一`model_config`。失败不得（SHALL NOT）消费key或留下孤立版本；并发相同key必须（SHALL）由unique constraint收敛后回滚输家并按key-only规则重读winner。

成功响应必须（SHALL）使用固定`AgentRuntimeCreated` DTO，且只能包含`agent_runtime_id`、pinned `agent_id`、`agent_name`、`agent_version`与runtime `created_at`。响应必须（SHALL）为`201 Created`并携带`Location: /v1/agent-runtimes/{agent_runtime_id}`；不得（SHALL NOT）包含随后可变的model、status、Session、Turn、usage、approval或barrier。

#### Scenario: 纯创建 AgentRuntime
- **WHEN** 客户端用有效UUID key和有效template调用`POST /v1/agent-runtimes`
- **THEN** API返回固定`AgentRuntimeCreated`、201与runtime Location；state为idle、Session/current Turn为空且没有模型调用或durable Turn event

#### Scenario: 相同 Key 与不同 Body 重试
- **WHEN** 第一次create已提交但响应丢失，客户端用同一key和不同`agent_name`或model override重试
- **THEN** API不重读template或比较请求，返回原runtime相同语义的201 body与Location

#### Scenario: Template 已变化但 Create 重试命中 Key
- **WHEN** 原create成功后template被修改、删除或改tag，客户端以同一key重试
- **THEN** API在读取template前命中原state并返回原runtime与pinned definition metadata

#### Scenario: Exact Tag 相同定义被不同 Key 复用
- **WHEN** 不同key基于same name、same tag与相同canonical definition创建runtime
- **THEN** API创建不同`AgentRuntimeId`但复用同一`AgentId`

#### Scenario: Exact Tag 不同定义冲突
- **WHEN** key未命中且当前template复用已存在name/tag却改变canonical definition
- **THEN** API返回`409 agent_version_conflict`，不覆盖历史definition、不创建runtime且不消费key

#### Scenario: 不同 Tag 相同定义创建新版本
- **WHEN** 作者为相同canonical definition提供不同tag后创建runtime
- **THEN** API插入新`agents` row与新`AgentId`，新runtime pin它

#### Scenario: Create Key 缺失或无效
- **WHEN** create请求缺少`Idempotency-Key`或其值不是合法UUID
- **THEN** API返回`400 invalid_request`，不读取template且不产生durable mutation

#### Scenario: Web 保留未决 Create Key
- **WHEN** Web使用`crypto.randomUUID()`发起create但无法确定请求是否成功
- **THEN** Web为该pending create保留同一key并重试；只有形成新的create intent时才生成新key

### Requirement: AgentRuntimeView 是固定 Postgres 屏障上的冷视图
系统必须（SHALL）通过 `GET /v1/agent-runtimes/{agent_runtime_id}` 返回API-owned `AgentRuntimeView`，而不是暴露数据库row。view必须（SHALL）包含`agent_runtime_id`、pinned `agent_id`、join得到的`agent_name`与`agent_version`、`status`、唯一`model_config`、nullable `session_id`、nullable `current_turn_id`、以无符号十进制字符串编码的`snapshot_event_seq`与`telemetry_floor_event_seq`、`pending_approvals`、current Turn可空`latest_usage`与布尔`resume_required`；不得（SHALL NOT）包含公开outcome、runtime snapshot、prompt、tools或raw durable payload。

除process registry派生的`resume_required`外，definition pin、status、barrier、telemetry floor、latest usage与pending approvals必须（SHALL）来自同一Postgres MVCC snapshot。`snapshot_event_seq`必须（SHALL）直接等于该snapshot中的`agent_states.last_event_seq`；`telemetry_floor_event_seq`必须（SHALL）等于barrier内最后一个严格解码为assistant `MessageAppended`的event_seq，不存在时为`"0"`。两者不得（SHALL NOT）另存为state字段或第二个high-water。`pending_approvals`必须（SHALL）从current Turn在barrier内的Requested减Resolved facts派生，Consumed或Invalidated approval不得（SHALL NOT）返回；`latest_usage`必须（SHALL）从current Turn最大event_seq上的provider usage派生。

`resume_required`只在durable status为running且registry没有exact `(AgentRuntimeId, current TurnId)`的starting或running handle时为true；它不得（SHALL NOT）写入Postgres。若runtime row不存在返回`404 agent_runtime_not_found`；若state存在但pinned `agents` row缺失、metadata不一致或definition无法严格解码，必须（SHALL）返回`500 durable_state_corrupt`，不得伪装成runtime/template不存在或重读filesystem。

#### Scenario: 读取尚未开始 Turn 的 Runtime
- **WHEN** 客户端读取纯创建后仍为idle的AgentRuntime
- **THEN** API返回200，Session/current Turn与latest usage为空，snapshot frontier为0、telemetry floor为`"0"`，且view不依赖hosting registry

#### Scenario: Cold 首屏之外的 Final 仍建立 Telemetry Floor
- **WHEN** barrier内最新assistant `MessageAppended`已被更多product rows推出最新history page
- **THEN** view仍返回该assistant event_seq作为telemetry floor，Web无需NATS重放即可拒绝更旧telemetry

#### Scenario: 读取未托管 Running Runtime
- **WHEN** Postgres中runtime为running且本进程没有exact-Turn handle
- **THEN** API保留durable running status并返回`resume_required=true`，不把advisory写回Postgres

#### Scenario: 刷新恢复 Pending Approval
- **WHEN** current Turn在snapshot barrier内存在Requested但未Resolved的approval
- **THEN** `pending_approvals`返回该审批，浏览器即使错过realtime request也能重新显示

#### Scenario: Usage 取 Current Turn 最新 Provider 响应
- **WHEN** current Turn有多个携带usage的iteration或terminal event
- **THEN** `latest_usage`等于barrier内event_seq最大的usage，且不表示lifetime累计账单

#### Scenario: 共享 AgentId 的 View 相互隔离
- **WHEN** 两个AgentRuntime pin同一`AgentId`
- **THEN** 每个view返回自己的status、model、Session/Turn、barrier、approval与usage，不从另一个runtime聚合

#### Scenario: Runtime 不存在
- **WHEN** 客户端读取不存在的`AgentRuntimeId`
- **THEN** API返回`404 agent_runtime_not_found`

### Requirement: Message 命令以 Exact Current-Turn CAS 创建新 Turn
系统必须（SHALL）通过`POST /v1/agent-runtimes/{agent_runtime_id}/messages`接受user message。请求体必须（SHALL）包含原始`text`与显式nullable `expected_current_turn_id`，并且可以包含可选`session_id`与完整`model_config`；缺少expected字段或包含未知字段必须（SHALL）返回`400 invalid_request`，首次message必须（SHALL）传null。系统只能（SHALL）trim text来判断是否为空，必须（SHALL）持久化已接受的原始text。

preflight必须（SHALL）先按`AgentRuntimeId`加载state及其pinned `AgentId`，再从`agents`加载immutable prompt、ordered tools与template definition identity；不得（SHALL NOT）在Turn admission期间重读filesystem template。provider、model、parameters、tools与runtime必须（SHALL）在任何durable mutation前完成preflight。

只有durable status为`idle | finished | failed | cancelled`且expected current Turn与Postgres完全相等时，系统才可（SHALL）admit新Turn。外层orchestration必须（SHALL）先生成`TurnId`、在本进程安装带唯一claim identity和cancellation token的exact `(AgentRuntimeId, TurnId)` starting entry，并在开启`LoopStarted`写事务前通过无caller-frontier参数的dispatcher hub ensure取得live handle。若generation不存在，hub必须（SHALL）在per-runtime gate内读取当前committed PG high-water并安装generation。随后`LoopStarted`事务锁exact `agent_states` row、执行CAS、绑定或校验Session、分配AgentRuntime-wide event_seq、写含pinned `agent_id`的snapshot并把status置为running。该handle跨commit持有且commit后接收receipt。kernel的bound sink复用该live handle并独立提交首条user `MessageAppended`；API只有在managed future已安装且该message已提交后，才可（SHALL）返回`202 Accepted`与`agent_runtime_id`、`agent_id`、`session_id`、`turn_id`。

首Turn使用请求提供的SessionId；省略时服务端必须（SHALL）生成UUIDv7。Session不要求在独立表中预先存在。一旦随`LoopStarted`绑定，后续Turn省略session时复用它，显式不同值返回`409 session_mismatch`。同一Session已被另一个AgentRuntime的running state占用时返回`409 session_busy`；当前change不得（SHALL NOT）增加Session claim或scheduler语义。

message model override是完整替换。snapshot必须（SHALL）固定effective config；只有首条user append提交且值发生变化时才更新`agent_states.model_config`。override不得（SHALL NOT）改变pinned `AgentId`或template默认模型。

#### Scenario: 首条消息创建 Session 和 Turn
- **WHEN** idle runtime收到非空text、`expected_current_turn_id=null`且未提供SessionId
- **THEN** 服务生成UUIDv7 SessionId，在两个有序durable boundary提交`LoopStarted`与首条user message后返回202及两类Agent identity

#### Scenario: 终态 Runtime 接受后续消息
- **WHEN** runtime为finished、failed或cancelled且请求携带exact recent Turn
- **THEN** API复用既有Session与pinned Agent、创建新Turn并切换为running

#### Scenario: 丢失 Message 成功响应后的重试
- **WHEN** 第一次message已提交但客户端未收到响应，并以旧expected Turn重试
- **THEN** CAS返回`409 stale_turn`且不创建第二个Turn，即使第一次Turn已快速terminal

#### Scenario: Hosted Running Runtime 拒绝新消息
- **WHEN** durable status为running且exact Turn由当前进程托管
- **THEN** API返回`409 agent_runtime_busy`，不修改Turn、Session或model

#### Scenario: Unhosted Running Runtime 要求显式 Resume
- **WHEN** durable status为running且当前进程没有exact-Turn handle
- **THEN** API返回`409 resume_required`，不隐式resume或创建新Turn

#### Scenario: LoopStarted 前失败
- **WHEN** preflight、CAS、Session constraint或`LoopStarted`事务失败
- **THEN** API不返回成功，durable state保持原值，registry只按本请求claim identity清理自己的entry

#### Scenario: LoopStarted 后首条 Message 未提交
- **WHEN** `LoopStarted`已commit但首条user message未commit
- **THEN** API不返回202，Postgres保留started-only running Turn，state model不变，后续只由显式resume执行preamble reconciliation

#### Scenario: Text 只按 Trim 判空
- **WHEN** message text前后有空白但trim后非空
- **THEN** API接受并持久化原始text；trim后为空时返回`400 invalid_request`且不写durable event

#### Scenario: 共享 AgentId 的并发 Runtime
- **WHEN** 两个runtime pin同一`AgentId`并同时admit message
- **THEN** 它们使用各自`AgentRuntimeId`、registry handle、state lock与event sequence，不互相产生busy或history混流，除非显式竞争同一Session

#### Scenario: Message API 不接受 Credential 字段
- **WHEN** message request试图携带API key、token或其他专用credential字段
- **THEN** strict schema返回`400 invalid_request`；API不把该值复制到conversation、definition或model

### Requirement: AgentRuntime 历史直接读取 Durable Ledger
系统必须（SHALL）通过`GET /v1/agent-runtimes/{agent_runtime_id}/history`直接查询该runtime的Postgres `durable_events`公开history视图，不得（SHALL NOT）依赖message projection、NATS、filesystem或读时kernel replay。查询必须（SHALL）携带固定inclusive `through_event_seq`，可以携带exclusive `before_event_seq`与`limit`；所有对外event-sequence参数和响应字段必须（SHALL）使用无符号十进制字符串。limit默认50、最大256。响应必须（SHALL）包含按event_seq升序排列的typed `AgentRuntimeProductEventV1` items、固定`through_event_seq`、nullable `next_before_event_seq`与`has_more`。

History必须（SHALL）返回完整、安全映射的`AgentRuntimeProductEventV1` union：`LoopStarted`、`MessageAppended`、`ToolApprovalRequested`、`ToolApprovalResolved`、`TranscriptCompacted`、`IterationCompleted`、`LoopFinished`、`LoopFailed`与`LoopCancelled`，从而使PG reconcile可读取任意固定`(B,T]` public product window。它不得（SHALL NOT）返回`ToolExecutionStarted`、Hook journal或其他internal facts。Tool result必须（SHALL）作为`MessageAppended(role=tool, tool_call_id=CallId, content=final JSON)`返回。`TranscriptCompacted`必须（SHALL）在自己的event_seq位置返回完整summary与公开marker数据；原始message永久保留并可继续向上分页。History与SSE durable frame必须（SHALL）复用同一API-owned typed union与版本，不得直接序列化raw durable payload。Web必须严格解码完整product page：reconcile对固定`(B,T]`按序应用全部product；cold bootstrap与向上分页则以barrier view作为current-state真相，只把允许的message、compaction和安全terminal marker并入conversation timeline，历史LoopStarted/approval/iteration/finished不得回写当前status、pending、draft或PG-confirmed barrier。显示过滤不得改变pagination cursor。

服务端必须（SHALL）使用1 MiB soft page budget：加入下一条会超出预算时结束页面；若当前页第一条自身超限，必须（SHALL）完整返回该单条并保证cursor推进。公开history sequence可以因内部Hook、`ToolExecutionStarted`等非product event出现数字间隔，客户端不得把间隔视为损坏。

#### Scenario: 首屏读取最新 History Page
- **WHEN** 客户端以`AgentRuntimeView.snapshot_event_seq`为through barrier且不带before cursor
- **THEN** API从该runtime barrier内反向取最近一页，并把响应翻为event_seq严格升序

#### Scenario: Reconcile 读取完整 Product Window
- **WHEN** fixed `(B,T]` 中包含LoopStarted、审批、IterationCompleted、terminal与message等public product，并夹有internal Hook/ToolExecutionStarted rows
- **THEN** history分页最终返回全部public product及其原event_seq、排除internal rows；reducer可完整补齐窗口而conversation timeline只渲染允许的可见项

#### Scenario: 向上滚动加载旧历史
- **WHEN** 用户确实向上滚动且客户端用原through barrier与`next_before_event_seq`请求下一页
- **THEN** API只返回固定snapshot中小于exclusive before的更旧items，新提交event不进入窗口；Web验证全部product但只扩展历史timeline，不把旧control facts应用为当前runtime状态

#### Scenario: 共享 AgentId 不共享 History
- **WHEN** 两个runtime pin同一`AgentId`
- **THEN** history route只读取URL中`AgentRuntimeId`的ledger，另一runtime的event不会出现

#### Scenario: Tool Result 与普通消息共享序列
- **WHEN** Tool完成或失败并提交最终tool message
- **THEN** history在该AgentRuntime-wide event_seq返回role=tool item，不需要第二种完成event或message序号

#### Scenario: Compaction Marker 不删除原消息
- **WHEN** snapshot中存在`TranscriptCompacted`
- **THEN** history返回可展开完整summary的typed marker，旧消息仍能通过更早页面读取

#### Scenario: History 查询无效
- **WHEN** through缺失、limit越界、cursor无法解析、before超出through或through超出runtime frontier
- **THEN** API返回`400 invalid_history_query`，不猜测新的barrier

#### Scenario: 单条超过 Soft Budget
- **WHEN** 当前页第一条完整JSON大于1 MiB
- **THEN** API完整返回该单条并结束页面，不截断内容或形成无法推进的cursor

### Requirement: Resume 只托管 Exact Unhosted Running Turn
系统必须（SHALL）通过`POST /v1/agent-runtimes/{agent_runtime_id}/resume`显式恢复Turn，请求体必须（SHALL）且只能包含`turn_id`。只有Postgres status为running、current Turn与请求完全相等且当前进程没有exact `(AgentRuntimeId, TurnId)`时，API才可（SHALL）安装starting claim。API必须（SHALL）按state pin加载immutable definition，验证snapshot `agent_id`、`agent_states.agent_id`与`agents.id`一致，并在返回成功前捕获固定durable barrier，完成started-only、runtime snapshot、extension version、Hook journal、compaction与Tool result preflight。

fixed durable slice、definition/provider/tool fingerprint、lineage与typed replay window等不依赖bound sink的preflight成功后、安装exact managed future前，API必须（SHALL）通过无caller-frontier参数的dispatcher hub ensure取得live handle；missing generation的initial frontier由hub在per-runtime ensure/retirement gate内读取当前committed PG high-water。API随后才可用该handle组装API-owned bound sinks与exact AgentLoop并执行纯`prepare_resume`；prepare失败必须释放handle与claim，且此前不得发生durable write或模型/Tool/Hook外部动作。prepare成功后，API必须（SHALL）用短state-row-lock事务重新验证definition pin、running/current Turn；失败时释放handle与claim，成功时让API-owned bound sinks与managed task持有该handle到Turn退出。这样并发durable writer只能共享该generation，resume后即使下一动作直接产生telemetry而尚无新durable append也已有安全frontier；resume不得（SHALL NOT）从0重发旧history或让Telemetry sink在首个delta时隐式建generation。initial frontier可以高于replay through barrier，但不能改变prepared replay window；期间新增的approval facts仍由其ledger读取合同消费。

只有exact managed future与dispatcher handle均已安装且claim转为running时，API才可（SHALL）返回`202 Accepted`与`agent_runtime_id`、`agent_id`、`session_id`、`turn_id`。相同exact Turn已有starting或running claim时必须（SHALL）幂等返回`204 No Content`。`AgentRuntimeId`只存在于API-owned恢复编排与sink scope，kernel replay不得（SHALL NOT）接收它。

Resume不得（SHALL NOT）创建Session、Turn、model override、definition版本、repair row或通用rebuild。除started-only外，preflight失败必须（SHALL）只释放本请求exact claim，并保持Turn `running + unhosted`。

#### Scenario: 恢复未托管 Running Turn
- **WHEN** 请求Turn等于Postgres current Turn、status为running且registry不含exact Turn
- **THEN** API加载pinned definition、完成固定屏障preflight、通过hub ensure取得live handle、在短row-lock事务中重验Turn、安装managed future并返回202

#### Scenario: 并发 Resume 收敛
- **WHEN** 两个请求并发resume同一unhosted running Turn
- **THEN** 一个安装exact claim并返回202，另一个观察同一starting/running claim后返回204

#### Scenario: Snapshot Definition Pin 不一致
- **WHEN** runtime snapshot的`agent_id`与state pin或加载definition不一致
- **THEN** API返回`500 durable_state_corrupt`，不重读filesystem也不开始外部动作

#### Scenario: Resume 使用陈旧 TurnId
- **WHEN** 请求Turn不等于current Turn
- **THEN** API返回`409 stale_turn`，不托管或修改任何Turn

#### Scenario: Resume 非 Running Turn
- **WHEN** 请求Turn等于current Turn但status不是running
- **THEN** API返回`409 turn_not_running`

#### Scenario: Started-only Turn 原子失败
- **WHEN** 固定barrier内current Turn只有`LoopStarted`，没有首条user message或其他current-Turn activity
- **THEN** resume不进入AgentLoop，而通过标准append原子提交`LoopFailed`与failed state，释放claim并返回`409 turn_preamble_incomplete`

#### Scenario: Runtime 版本不兼容
- **WHEN** snapshot或extension版本结构有效但当前binary不支持
- **THEN** API返回`409 runtime_incompatible`，释放claim并保持Turn running/unhosted

#### Scenario: Durable Truth 损坏
- **WHEN** snapshot缺失或畸形，或ledger、Hook journal、Tool result、compaction core fact无法通过一致性校验
- **THEN** API返回`500 durable_state_corrupt`，释放claim并保持Turn running/unhosted

#### Scenario: Runtime 依赖暂不可用
- **WHEN** snapshot有效兼容但固定provider、model、tool、skill、extension或Hook implementation不可用
- **THEN** API返回`503 runtime_unavailable`，释放claim并保持Turn running/unhosted

#### Scenario: Postgres 暂不可用
- **WHEN** Postgres无法完成preflight read、lock或started-only terminal事务，且重读不能确认结果
- **THEN** API返回`503 store_unavailable`，只清理本请求exact claim，不猜测durable state

### Requirement: Cancel 只向 Exact Hosted Turn 发出内存信号
系统必须（SHALL）通过`POST /v1/agent-runtimes/{agent_runtime_id}/cancel`接受取消，请求体必须（SHALL）且只能包含`turn_id`。只有Postgres status为running、current Turn与请求相等且registry持有同一exact `(AgentRuntimeId, TurnId)`的running handle、managed future与token时，API才可（SHALL）signal其`CancellationToken`并返回空body的`202 Accepted`。cancel不得（SHALL NOT）持久化intent、隐式resume、abort/drop `AgentLoop` future或承诺最终一定cancelled。

命中同一Turn的starting claim返回`409 turn_starting`；running但未托管返回`409 turn_not_hosted`；Turn不匹配返回`409 stale_turn`。Tool或协作方可以暂时忽略cancellation，最终status只由唯一durable terminal event决定。

#### Scenario: 取消当前托管 Turn
- **WHEN** 请求Turn与durable current Turn和registry running handle完全一致
- **THEN** API signal token并返回空body的202，UI只显示取消请求已发送

#### Scenario: 取消未托管 Running Turn
- **WHEN** Postgres Turn为running但本进程没有exact running handle
- **THEN** API返回`409 turn_not_hosted`，不写cancel intent且不自动resume

#### Scenario: Cancel 与 Starting 竞态
- **WHEN** exact Turn已有starting claim但managed future尚未安装
- **THEN** API返回`409 turn_starting`，不signal尚未成立的运行

#### Scenario: 陈旧 Cancel 不影响新 Turn
- **WHEN** 请求Turn不等于current Turn
- **THEN** API返回`409 stale_turn`，新Turn token与durable state均不改变

#### Scenario: 重复取消已取消 Turn
- **WHEN** runtime的同一current Turn已为cancelled
- **THEN** API幂等返回空body的204；同一finished或failed Turn返回`409 turn_not_running`

#### Scenario: 正常完成先于取消生效
- **WHEN** API已返回202但AgentLoop在观察cancellation前提交`LoopFinished`
- **THEN** 最终status为finished，系统不补写第二个terminal event

### Requirement: Approval Resolve 与 Resume 是独立命令
系统必须（SHALL）通过`POST /v1/agent-runtimes/{agent_runtime_id}/approvals/{approval_id}` resolve approval，请求体必须（SHALL）且只能是`{turn_id, decision}`，其中decision只能为`approve | reject`。resolver必须（SHALL）锁定exact AgentRuntime state，先校验请求Turn与approval所属/current Turn identity，并从该runtime durable Requested、Resolved与terminal facts判定结果；terminal判定必须（SHALL）先于running-status要求，只有追加新Resolved时才要求Turn仍running。resolver不得（SHALL NOT）读取或写入approval projection table。

若所属Turn已terminal，API必须（SHALL）返回`409 approval_invalidated`。若同一approval已以相同decision resolve，必须（SHALL）幂等返回`204 No Content`；相反decision返回`409 approval_already_resolved`。只有Requested尚未resolve时才可追加唯一`ToolApprovalResolved`；commit后API返回204，再best-effort通知以`(AgentRuntimeId, ApprovalId)`定位的本机waiter。通知或NATS失败不得改变已提交决定。

resolve不得（SHALL NOT）隐式resume。unhosted Turn上的决定只持久化，客户端随后显式resume；hosted Turn的reject必须映射为blocked Tool result并让Agent继续，而不是取消Turn。

#### Scenario: 首次批准 Hosted Turn
- **WHEN** exact current running Turn的unresolved approval收到approve
- **THEN** API提交`ToolApprovalResolved`、返回204，再通知该runtime waiter执行Tool

#### Scenario: 同决定重试
- **WHEN** 客户端因响应丢失再次提交相同decision
- **THEN** API从ledger识别相同resolution并返回204，不追加第二条event

#### Scenario: 相反决定冲突
- **WHEN** approval已approve后收到reject或相反
- **THEN** API返回`409 approval_already_resolved`，原决定不变

#### Scenario: Turn Terminal 后审批失效
- **WHEN** approval所属Turn已提交任一terminal event
- **THEN** API返回`409 approval_invalidated`

#### Scenario: Approval 不存在
- **WHEN** exact AgentRuntime/Turn ledger中不存在该ApprovalId的Requested fact
- **THEN** API返回`404 approval_not_found`

#### Scenario: Unhosted Turn 只保存决定
- **WHEN** unresolved approval属于exact running Turn但本进程未托管
- **THEN** API持久化决定并返回204，Web移除pending approval并显示需要Resume，不自动恢复

#### Scenario: Reject 继续 Agent
- **WHEN** hosted或恢复后的waiter观察到reject
- **THEN** Handler产生模型可见的blocked Tool result并继续运行，API不把reject解释为cancel

#### Scenario: 共享 AgentId 的 Approval 不混流
- **WHEN** 两个runtime pin同一`AgentId`且恰有相同ApprovalId值
- **THEN** resolver只读取URL指定`AgentRuntimeId`的ledger与waiter，不影响另一个runtime

### Requirement: AgentRuntime SSE 只提供短期实时 Tail
系统必须（SHALL）通过`GET /v1/agent-runtimes/{agent_runtime_id}/events`提供严格按AgentRuntimeId过滤、跨该runtime多个Turn的SSE tail；尚未绑定Session的idle runtime也必须（SHALL）能够订阅。SSE data必须（SHALL）使用API-owned `AgentRuntimeStreamFrameV1`，每个frame必须携带exact `agent_runtime_id`与state pinned `agent_id`，并按kind提供：

- control：`stream_ready`或`stream_reset { reason: "buffer_overflow" }`；
- durable：十进制字符串`event_seq`、`event_version`与`AgentRuntimeProductEventV1`；
- telemetry：十进制字符串`durable_before_event_seq`、`llm_call_id`、`telemetry_seq`与typed LLM event。

runtime与Agent identity必须（SHALL）承担不同职责：durable排序/去重键只能是`(AgentRuntimeId, event_seq)`，telemetry fence至少是`(AgentRuntimeId, llm_call_id, telemetry_seq)`；frame中的`agent_id`只用于校验immutable definition pin，不得（SHALL NOT）用于NATS subject、dispatcher map、history barrier或event去重。Web应用frame前必须（SHALL）同时验证两类identity与当前view一致，不一致时关闭stream并执行无cursor cold bootstrap。

任何SSE `id`必须（SHALL）是不透明NATS cursor。客户端可以使用`Last-Event-ID`或`after_cursor`二选一继续当前页面内仍被保留的short tail；无cursor时只订阅新事件。cursor不得（SHALL NOT）充当durable sequence、history barrier或跨页面恢复状态。NATS subject、dispatcher generation与cursor validity必须（SHALL）绑定exact AgentRuntimeId；同一`AgentId`下多个runtime不得混入同一tail。NATS retention必须（SHALL）使用短age/bytes/messages上限，不承担durable history或跨重启补发。

API必须（SHALL）在exact runtime的NATS subscription建立并开始buffering后才发送`stream_ready`。cursor过期在建流前返回`410 cursor_expired`；NATS无法订阅返回`503 realtime_unavailable`。建流后bounded buffer溢出时，API必须（SHALL）在该连接直接发送无SSE id的`stream_reset`并关闭；该control不得写PG或发NATS。Postgres commit必须（SHALL）永远先于realtime publish，publish失败不得回滚durable结果。

每条telemetry进入dispatcher bounded queue时必须（SHALL）冻结当时已知的PG durable high-water为`durable_before_event_seq`，dispatcher发布它前必须flush到该watermark。该fence只表达ordering，不分配durable sequence。若PG reconcile已应用assistant final event F，随后到达且fence小于F的旧telemetry必须丢弃；fence大于等于F的下一call telemetry不得仅因此前final被丢弃。

#### Scenario: Idle Runtime 先建立 SSE
- **WHEN** 客户端在首条message前订阅已存在的idle runtime
- **THEN** API建立runtime-scoped stream并在subscription可接收tail后发送Session/Turn可空的`stream_ready`

#### Scenario: Cursor 后继续 Tail
- **WHEN** 客户端提供仍在retention内的单一cursor
- **THEN** API从该transport position后交付该runtime frame，不交付共享AgentId的其他runtime或公开Session聚合流

#### Scenario: 同时提供两个 Cursor 来源
- **WHEN** 请求同时携带`Last-Event-ID`与`after_cursor`或cursor无法解析
- **THEN** API返回`400 invalid_cursor`

#### Scenario: Cursor 已过期
- **WHEN** cursor对应NATS position已淘汰
- **THEN** API返回`410 cursor_expired`，客户端清除page-memory cursor并执行PG cold bootstrap

#### Scenario: 已建立 Stream 的 Buffer 溢出
- **WHEN** SSE headers已发送后该连接bounded buffer溢出
- **THEN** API发送无SSE id的`stream_reset { reason: "buffer_overflow" }`并关闭连接
- **AND** Web丢弃该连接buffer、transient draft与cursor，以无cursor subscription重新cold bootstrap

#### Scenario: NATS 不可用时核心命令继续工作
- **WHEN** SSE subscription因NATS故障返回`503 realtime_unavailable`
- **THEN** Web显示realtime degraded并通过view/history reconcile，Postgres-backed command合同保持可用

#### Scenario: Durable Commit 先于 Realtime Publish
- **WHEN** message、approval、compaction或terminal event已在Postgres commit
- **THEN** 系统之后才可best-effort发布对应frame；publish失败不得回滚或否认durable HTTP结果

### Requirement: HTTP 错误使用稳定且安全的类型
所有`/v1` Agent runtime error response必须（SHALL）使用`{"error":{"code":"...","message":"..."}}`。`code`必须（SHALL）是稳定snake_case discriminant，客户端不得依赖message文本分支。library error必须（SHALL）在独立`error.rs`中使用`thiserror`定义并保留source chain；HTTP边界必须（SHALL）显式映射status/code，且只在真正处理错误的边界记录一次详细来源。

所有JSON request body必须（SHALL）限制为64 KiB，超限返回`413 request_too_large`。至少必须（SHALL）固定以下映射：

- `404 agent_runtime_not_found`用于runtime route，`404 agent_template_not_found`用于create catalog lookup，`404 approval_not_found`用于approval lookup；
- `409 agent_version_conflict`用于exact name/tag定义冲突；
- `409 stale_turn`、`agent_runtime_busy`、`resume_required`、`session_mismatch`、`session_busy`、`turn_not_running`、`turn_not_hosted`、`turn_starting`、`turn_preamble_incomplete`、`approval_already_resolved`、`approval_invalidated`与`runtime_incompatible`用于各自durable冲突；
- `410 cursor_expired`、`413 request_too_large`；
- `422 invalid_agent_version`、`invalid_agent_template`、`model_not_configured`与`invalid_model_parameters`；
- `500 durable_state_corrupt`与`internal_error`；
- `503 store_unavailable`、`runtime_unavailable`、`realtime_unavailable`与`service_shutting_down`。

error body不得（SHALL NOT）暴露filesystem host path、SQL、NATS subject、prompt、Tool arguments/result、provider body、credential、stack trace或底层错误文本。

#### Scenario: Template Tag 错误有稳定类型
- **WHEN** catalog或key-miss create遇到缺失、过长或非法version tag
- **THEN** API返回`422 invalid_agent_version`，不把它折叠为template not found或internal error

#### Scenario: Exact Tag Definition 冲突有稳定类型
- **WHEN** create发现exact name/tag已存在但canonical definition不同
- **THEN** API返回`409 agent_version_conflict`，不覆盖definition且不消费create key

#### Scenario: Runtime 与 Template Not Found 不混淆
- **WHEN** runtime route找不到`AgentRuntimeId`，或key-miss create找不到template name
- **THEN** API分别返回`404 agent_runtime_not_found`与`404 agent_template_not_found`

#### Scenario: Resume Failure 不被折叠
- **WHEN** resume分别遇到unsupported version、损坏durable truth、暂不可用runtime dependency或Postgres failure
- **THEN** API分别返回`409 runtime_incompatible`、`500 durable_state_corrupt`、`503 runtime_unavailable`或`503 store_unavailable`

#### Scenario: Realtime Failure 不伪装成 Store Failure
- **WHEN** NATS subscription不可用而Postgres正常
- **THEN** SSE返回`503 realtime_unavailable`，核心HTTP命令不返回虚假的`store_unavailable`

#### Scenario: Payload 超限
- **WHEN** 任一JSON command body超过64 KiB
- **THEN** API在反序列化业务payload前返回`413 request_too_large`，不产生durable mutation

#### Scenario: 未分类错误不泄漏内部细节
- **WHEN** handler遇到不能安全映射的内部失败
- **THEN** API返回`500 internal_error`与固定安全message，只在服务端tracing记录source chain
