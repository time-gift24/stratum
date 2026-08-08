# agent-runtime-api Specification

## ADDED Requirements

### Requirement: Template catalog 是只读热目录
系统必须（SHALL）从 `[agent].templates_root` 暴露 `GET /v1/agent-templates`。服务启动时必须（SHALL）验证该路径存在、是目录且可读；空目录合法，服务不得（SHALL NOT）自动创建 template、definition 或 history 目录。每次 catalog 请求与首次创建 Agent 时都必须（SHALL）读取当时最新的 template TOML；catalog 中任一 template 无法读取、解析或校验时，整个 catalog 请求必须（SHALL）失败，不得返回部分结果。

Template catalog 只能（SHALL）返回创建界面需要的公开标识、展示信息和公开模型默认值，不得（SHALL NOT）返回 prompt、tools JSON、raw TOML、host path、内容 digest 或其他 resolved definition 内容。既有 Agent 必须（SHALL）继续使用创建时固化的 immutable resolved definition，不得因 template 文件后续变化而改变。

#### Scenario: 空 Template 目录
- **WHEN** 已配置的 `templates_root` 可读但不包含任何有效 template 文件
- **THEN** 服务正常启动，`GET /v1/agent-templates` 返回 `200 OK` 与空目录

#### Scenario: Catalog 全有或全无
- **WHEN** catalog 中一个 template 有效而另一个 template 无法解析
- **THEN** `GET /v1/agent-templates` 返回 `422 invalid_agent_template`，且不泄露部分 catalog 或 host path

#### Scenario: Template 热更新只影响新 Agent
- **WHEN** 运维者修改一个 template 后再次读取 catalog 并创建新 Agent
- **THEN** catalog 与新 Agent 使用修改后的最新定义，修改前创建的 Agent 仍使用自己的 immutable resolved definition

#### Scenario: Template Root 无效时拒绝启动
- **WHEN** `templates_root` 缺失、不是目录或启动时不可读
- **THEN** 服务启动失败，且不创建替代目录或回退到内置、filesystem execution 定义

### Requirement: 模型目录与完整覆盖具有稳定合同
系统必须（SHALL）通过 `GET /v1/models` 返回当前已配置且可选择的模型目录及其公开 `parameters_schema`。`POST /v1/agents` 与 `POST /v1/agents/{agent_id}/messages` 可以（SHALL）接受完整 `model_config` 覆盖；覆盖必须（SHALL）整体替换 provider、model 与 provider-specific parameters，不得（SHALL NOT）与原配置做字段 merge。系统必须（SHALL）在任何 durable mutation 前校验覆盖。resume 请求不得（SHALL NOT）接受模型覆盖，并必须（SHALL）使用目标 Turn 的固定 runtime snapshot。

#### Scenario: 前端按 Schema 构造模型参数
- **WHEN** 客户端调用 `GET /v1/models`
- **THEN** API 返回 `200 OK` 与已配置模型的公开描述和 `parameters_schema`，客户端无需硬编码 provider 参数枚举

#### Scenario: 创建时完整覆盖模型
- **WHEN** create 请求携带有效 `model_config`
- **THEN** 新 Agent 的 creation model 与 mutable default model 都使用该完整覆盖，template 的 prompt 与 tools 保持不变

#### Scenario: 后续 Turn 覆盖模型
- **WHEN** terminal Agent 的 message 请求携带有效且不同的 `model_config`
- **THEN** `LoopStarted` runtime snapshot 固定该完整覆盖，并且只有首条 user `MessageAppended` 成功提交时才把它写为后续 Turn 的默认配置

#### Scenario: 相同模型不重复写入
- **WHEN** 新 Turn 的 effective model config 与 Agent 当前 default model config 完全相同
- **THEN** 首条 user message 事务不执行冗余 default model 更新，但 Turn runtime snapshot 仍固定该配置

#### Scenario: Started-only 不改变默认模型
- **WHEN** `LoopStarted` 使用不同覆盖，但首条 user message 没有提交
- **THEN** Agent 的 default model config 保持旧值，started-only reconciliation 不传播未接受的覆盖

#### Scenario: 无效模型覆盖
- **WHEN** create 或 message 请求引用未配置模型，或参数不符合该模型的 schema
- **THEN** API 返回 `422 model_not_configured` 或 `422 invalid_model_parameters`，且不产生 durable mutation

### Requirement: Agent 创建是幂等的纯持久化操作
系统必须（SHALL）通过 `POST /v1/agents` 创建不可变 Agent 实例。请求体必须（SHALL）且只能包含 `agent_name` 与可选的完整 `model_config`，请求必须（SHALL）携带由客户端生成的 UUID `Idempotency-Key`。创建不得（SHALL NOT）接受 user message、`SessionId` 或 `TurnId`，不得（SHALL NOT）调用模型、启动 AgentLoop 或生成 Turn event。

系统必须（SHALL）先按 idempotency key 查询已创建 Agent，再读取 template。key 命中且 canonical create request 的 `agent_name` 与可选 model override 相同时，系统必须（SHALL）永久返回原 Agent 的相同 `201 Created` response 与 `Location`；key 命中但请求不同时必须（SHALL）返回 `409 idempotency_key_conflict`。key 未命中时，系统必须（SHALL）热读并校验最新 template，在一个 Postgres 事务中写入包含prompt、tools与creation-time effective model config的immutable resolved definition、唯一internal `agent_version_id`与`idle` state；失败事务不得（SHALL NOT）占用 key。并发相同 key 必须（SHALL）由唯一约束收敛后重读。

#### Scenario: 纯创建 Agent
- **WHEN** 客户端用有效 UUID key 和有效 template 调用 `POST /v1/agents`
- **THEN** API 返回 `201 Created`、`Location` 与新 `AgentId`，Agent 为 `idle`，Session/current Turn 为空，且没有模型调用或 durable Turn event

#### Scenario: 相同 Key 与相同请求重试
- **WHEN** 第一次 create 已提交但响应丢失，客户端用同一 key 和相同 create request 重试
- **THEN** API 不重读 template，返回原 Agent 的相同 `201 Created` body 与 `Location`，不创建第二个 Agent

#### Scenario: 相同 Key 与不同请求冲突
- **WHEN** 已使用的 key 被用于不同 `agent_name` 或不同 model override
- **THEN** API 返回 `409 idempotency_key_conflict`，原 Agent 与最新 template 均不被修改

#### Scenario: Template 已变化但 Create 重试命中 Key
- **WHEN** 原 create 成功后 template 被修改或删除，客户端以同一 key 和相同请求重试
- **THEN** API 在读取 template 前命中原记录并返回原 Agent，不把最新 template 内容混入既有 Agent

#### Scenario: Create 失败不消费 Key
- **WHEN** template 或 model 校验失败，或创建事务回滚
- **THEN** API 返回类型化非成功错误，且客户端修正原因后可以用同一 key 再次创建

#### Scenario: Create Key 缺失或无效
- **WHEN** create 请求缺少 `Idempotency-Key` 或其值不是合法 UUID
- **THEN** API 返回 `400 invalid_request`，不读取 template且不产生 durable mutation

#### Scenario: Web 保留未决 Create Key
- **WHEN** Web 使用 `crypto.randomUUID()` 发起 create，但无法确定请求是否成功
- **THEN** Web 为该 pending create 保留同一 key并重试；只有明确失败并形成新 create intent 时才生成新 key

### Requirement: AgentView 是固定 Postgres 屏障上的冷视图
系统必须（SHALL）通过 `GET /v1/agents/{agent_id}` 返回 API-owned `AgentView`，而不是暴露数据库 row。view 必须（SHALL）包含 `agent_id`、Agent 名称、`status`、default `model_config`、可空 `session_id`、可空 `current_turn_id`、以无符号十进制字符串编码的 `snapshot_event_seq`、`pending_approvals`、current Turn 的可空 `latest_usage` 与布尔 `resume_required`；不得（SHALL NOT）包含公开 `outcome`、runtime snapshot、prompt、tools 或 raw durable payload。

除 `resume_required` 外，status、identity、barrier、latest usage 与 pending approvals 必须（SHALL）来自同一 Postgres MVCC snapshot。`snapshot_event_seq`必须（SHALL）直接等于该snapshot中读取的`agent_state.last_event_seq`，不得（SHALL NOT）另存snapshot cursor或第二个high-water。`pending_approvals` 必须（SHALL）由 current Turn 在 barrier 内的 Requested minus Resolved/Consumed/Invalidated ledger facts 派生。`latest_usage` 必须（SHALL）从 current Turn 在 barrier 内最后一个携带 provider usage 的 durable event 派生，不得从 `agent_state` 复制。`resume_required` 只是进程内 advisory：仅当 durable status 为 `running` 且 registry 没有 exact `(agent_id,current_turn_id)` 的 `starting` 或 `running` handle 时为 true；每个 command 仍必须重新校验 durable state。

#### Scenario: 读取尚未开始 Turn 的 Agent
- **WHEN** 客户端读取纯创建后仍为 `idle` 的 Agent
- **THEN** API 返回 `200 OK`，Session/current Turn 与 latest usage 为空，`snapshot_event_seq` 为当前 durable frontier，且 view 不依赖 hosting registry

#### Scenario: 读取未托管 Running Agent
- **WHEN** Postgres 中 Agent 为 `running` 且本进程没有 exact-Turn handle
- **THEN** API 保留 durable `running` status 并返回 `resume_required=true`，不把 advisory 写回 Postgres

#### Scenario: Snapshot 原子恢复审批
- **WHEN** current Turn 在 snapshot barrier 内存在未 Resolved、未 Consumed 且未 Invalidated 的 approval request
- **THEN** `pending_approvals` 返回该审批；浏览器即使曾错过 realtime request，也能在刷新后重新显示审批

#### Scenario: Usage 取 Current Turn 最新 Provider 响应
- **WHEN** current Turn 有多个携带 usage 的 iteration 或 terminal event
- **THEN** `latest_usage` 等于 barrier 内 event_seq 最大的 usage，且不表示 lifetime 累计账单

#### Scenario: Agent 不存在
- **WHEN** 客户端读取不存在的 `AgentId`
- **THEN** API 返回 `404 agent_not_found`

### Requirement: Message 命令以 exact current-Turn CAS 创建新 Turn
系统必须（SHALL）通过 `POST /v1/agents/{agent_id}/messages` 接受 user message。请求体必须（SHALL）包含原始 `text` 与显式可空 `expected_current_turn_id`，并且可以包含可选 `session_id` 与完整 `model_config`；缺少 `expected_current_turn_id` 或包含未知字段必须（SHALL）返回 `400 invalid_request`，首次 message 必须（SHALL）传 `null`。系统只能（SHALL）trim text 来判断是否为空，必须（SHALL）持久化已接受的原始 text；API不得（SHALL NOT）接受专用credential/token字段，也不得（SHALL NOT）把runtime-managed credential注入text或ModelConfig。任意自然语言不能被可靠证明不含用户粘贴的secret，因此系统不得（SHALL NOT）用猜测性scanner静默改写原文；该payload必须（SHALL）按对话级敏感数据治理。provider、model、parameters、tools 与 runtime 必须（SHALL）在任何 durable mutation 前完成 preflight。

只有 durable status 为 `idle | finished | failed | cancelled` 且请求的 expected current Turn 与 Postgres 完全相等时，系统才可（SHALL）admit 新 Turn。系统必须（SHALL）先生成 `TurnId` 并在本进程安装带唯一 claim identity 和 cancellation token 的 exact-Turn `starting` entry，再由 `LoopStarted` 事务锁 state、执行 CAS、绑定或校验 Session、分配 Agent-wide event_seq、写 runtime snapshot 并把 status 置为 `running`。kernel 随后必须（SHALL）通过标准 append 独立提交首条 user `MessageAppended`；API 只有在 managed future 已安装且该 message 已提交后，才可（SHALL）返回 `202 Accepted` 与 `agent_id`、`session_id`、`turn_id`。

首个 Turn 使用请求提供的 SessionId；省略时服务端必须（SHALL）生成 UUIDv7。系统不要求（SHALL NOT）SessionId 预先存在于 sessions table。Session 一旦随 `LoopStarted` 绑定，后续 Turn 省略 session 时复用它，显式不同值必须（SHALL）返回 `409 session_mismatch`。同一 Session 当前已被另一个 Agent 的 running state 占用时必须（SHALL）返回 `409 session_busy`。本版本不得（SHALL NOT）为此引入 Session claim table 或 Workflow/scheduler 幂等语义。

#### Scenario: 首条消息创建 Session 和 Turn
- **WHEN** `idle` Agent 收到非空 text，`expected_current_turn_id=null` 且未提供 SessionId
- **THEN** 服务生成 UUIDv7 SessionId，并在两个有序 durable boundary 提交 `LoopStarted` 与首条 user `MessageAppended` 后返回 `202 Accepted`

#### Scenario: 终态 Agent 接受后续消息
- **WHEN** Agent 为 `finished`、`failed` 或 `cancelled`，请求携带与 Postgres 相同的 expected current Turn
- **THEN** API 复用既有 Session、创建新 Turn、切换为 `running` 并在接受完成后返回 `202 Accepted`

#### Scenario: 丢失 Message 成功响应后的重试
- **WHEN** 第一次 message 已提交但客户端未收到响应，并使用旧 expected current Turn 重试，即使第一次 Turn 已快速 terminal
- **THEN** CAS 返回 `409 stale_turn`，不创建第二个 Turn；message endpoint 不定义额外通用 idempotency key

#### Scenario: Hosted Running Agent 拒绝新消息
- **WHEN** durable status 为 `running` 且 exact Turn 由当前进程托管
- **THEN** API 返回 `409 agent_busy`，不修改 Turn、Session 或模型配置

#### Scenario: Unhosted Running Agent 要求显式 Resume
- **WHEN** durable status 为 `running` 且当前进程没有 exact-Turn handle
- **THEN** API 返回 `409 resume_required`，不隐式 resume 或创建新 Turn

#### Scenario: LoopStarted 前失败
- **WHEN** preflight、CAS、Session constraint 或 `LoopStarted` 事务失败
- **THEN** API 不返回成功，durable state 保持原值，并且 registry 只按本请求 claim identity 清理自己的 entry

#### Scenario: LoopStarted 后首条 Message 未提交
- **WHEN** `LoopStarted` 已 commit，但首条 user `MessageAppended` 未 commit
- **THEN** API 不返回 202，Postgres 保留可诊断的 started-only running Turn，default model 不改变，后续只由显式 resume 执行 preamble reconciliation

#### Scenario: Text 只按 Trim 判空
- **WHEN** message text 前后有空白但 trim 后非空
- **THEN** API 接受并持久化原始 text；trim 后为空时返回 `400 invalid_request` 且不写 durable event

#### Scenario: Message API 不接受 Credential 字段
- **WHEN**message request试图携带API key、token或其他专用credential字段
- **THEN**strict request schema返回`400 invalid_request`；调用方必须使用外部credential配置/引用，API不把该值复制到conversation

### Requirement: Agent 历史直接读取 Durable Ledger
系统必须（SHALL）通过 `GET /v1/agents/{agent_id}/history` 直接查询 Postgres `durable_events` 的公开 history 视图，不得（SHALL NOT）依赖 `agent_messages`、NATS、filesystem 或读时 kernel replay。查询必须（SHALL）携带固定的 inclusive `through_event_seq`，可以携带 exclusive `before_event_seq` 与 `limit`；所有对外 event-sequence 参数和响应字段必须（SHALL）使用无符号十进制字符串，避免 JavaScript number 改变 identity。limit 默认 50、最大 256。响应必须（SHALL）包含按 event_seq 升序排列的 typed `AgentProductEventV1` items、固定 `through_event_seq`、可空 `next_before_event_seq` 与 `has_more`。

History 只可（SHALL）返回 `MessageAppended`、`TranscriptCompacted` 以及安全的 `LoopFailed`/`LoopCancelled` marker。Tool result 必须（SHALL）作为 `MessageAppended(role=tool, tool_call_id=CallId, content=final JSON)` 返回。`TranscriptCompacted` 必须（SHALL）在自己的 event_seq 位置返回完整 summary 与 compacted iteration 等公开 marker 数据；原始 message 永久保留并可继续向上分页。History 和 SSE durable frame 必须（SHALL）复用同一 API-owned typed union 与版本，不得把 raw durable payload 直接序列化给客户端。

服务端必须（SHALL）使用 1 MiB soft page budget：加入下一条会超出预算时结束页面；若当前页第一条自身超限，必须（SHALL）完整返回该单条并保证 cursor 推进。公开 history event_seq 可以（SHALL）因内部 Hook、approval 或其他过滤 event 而出现数字间隔，客户端不得（SHALL NOT）把该间隔视为数据损坏。

#### Scenario: 首屏读取最新 History Page
- **WHEN** 客户端以 `AgentView.snapshot_event_seq` 作为 through barrier 且不带 before cursor
- **THEN** API 从 barrier 内反向取最近一页，并把响应翻为 event_seq 严格升序

#### Scenario: 向上滚动加载旧历史
- **WHEN** 用户确实向上滚动且客户端用原 through barrier 与 `next_before_event_seq` 请求下一页
- **THEN** API 只返回该 snapshot 内 event_seq 小于 exclusive before 的更旧 items，新提交 event 不进入固定窗口

#### Scenario: Tool Result 与普通消息共享序列
- **WHEN** Tool 完成或失败并提交最终 tool message
- **THEN** history 在该 `MessageAppended` 的 Agent-wide event_seq 返回 role=tool item，不需要 `ToolExecutionCompleted` 或 message_seq

#### Scenario: Compaction Marker 不删除原消息
- **WHEN** snapshot 中存在 `TranscriptCompacted`
- **THEN** history 返回可展开完整 summary 的 typed marker，而旧消息仍能通过更早页面读取，不伪装成 system message

#### Scenario: History 查询无效
- **WHEN** through 缺失、limit 越界、cursor 无法解析、before 超出 through 或 through 超出 Agent durable frontier
- **THEN** API 返回 `400 invalid_history_query`，不猜测新的 barrier

#### Scenario: 单条超过 Soft Budget
- **WHEN** 当前页第一条完整 JSON 大于 1 MiB
- **THEN** API 完整返回该单条并结束页面，不截断内容或形成无法推进的 cursor

### Requirement: Resume 只托管 Exact Unhosted Running Turn
系统必须（SHALL）通过 `POST /v1/agents/{agent_id}/resume` 显式恢复 Turn，请求体必须（SHALL）且只能包含 `turn_id`。只有 Postgres status 为 `running`、current Turn 与请求完全相等且当前进程没有该 exact Turn 时，API 才可（SHALL）安装 `starting` claim。API 必须（SHALL）在返回成功前捕获固定 durable barrier，完成 started-only、runtime snapshot、extension version、Hook journal、compaction 与 Tool result preflight；只有 exact managed future 已安装且 claim 转为 `running` 时才可（SHALL）返回 `202 Accepted` 与 Agent/Session/Turn IDs。相同 exact Turn 已有 `starting` 或 `running` claim 时必须（SHALL）幂等返回 `204 No Content`。

Resume 不得（SHALL NOT）创建 Session、Turn、model override、repair row 或通用 rebuild。除 started-only 外，preflight 失败必须（SHALL）只释放本请求 exact claim，并保持 Turn `running + unhosted`。

#### Scenario: 恢复未托管 Running Turn
- **WHEN** 请求 Turn 等于 Postgres current Turn，status 为 `running` 且 registry 不含 exact Turn
- **THEN** API 完成固定屏障 preflight、安装 managed future 并返回 `202 Accepted`

#### Scenario: 并发 Resume 收敛
- **WHEN** 两个请求并发 resume 同一 unhosted running Turn
- **THEN** 一个请求安装 exact claim 并返回 202，另一个观察到同一 starting/running claim 后返回 204

#### Scenario: Resume 使用陈旧 TurnId
- **WHEN** 请求 Turn 不等于 current Turn
- **THEN** API 返回 `409 stale_turn`，不托管或修改任何 Turn

#### Scenario: Resume 非 Running Turn
- **WHEN** 请求 Turn 等于 current Turn但 status 不是 `running`
- **THEN** API 返回 `409 turn_not_running`

#### Scenario: Started-only Turn 原子失败
- **WHEN** 固定 barrier 内 current Turn 只有 `LoopStarted`，没有首条 `MessageAppended` 或其他 current-Turn activity
- **THEN** resume 不进入 AgentLoop，而是通过标准 durable append 原子提交 `LoopFailed` 与 `failed` state，释放 claim并返回 `409 turn_preamble_incomplete`

#### Scenario: Runtime 版本不兼容
- **WHEN** snapshot 或 extension 版本结构有效但当前 binary 不支持
- **THEN** API 返回 `409 runtime_incompatible`，释放 claim并保持 Turn running/unhosted

#### Scenario: Durable Truth 损坏
- **WHEN** runtime snapshot 缺失或畸形，或 durable ledger、Hook journal、Tool result、compaction core fact 无法通过一致性校验
- **THEN** API 返回 `500 durable_state_corrupt`，释放 claim并保持 Turn running/unhosted

#### Scenario: Runtime 依赖暂不可用
- **WHEN** snapshot 有效且兼容，但固定 provider、model、tool、skill、extension 或 Hook implementation 当前不可用
- **THEN** API 返回 `503 runtime_unavailable`，释放 claim并保持 Turn running/unhosted

#### Scenario: Postgres 暂不可用
- **WHEN** Postgres 无法完成 preflight read、lock 或 started-only terminal transaction，且重读不能确认结果
- **THEN** API 返回 `503 store_unavailable`，只清理本请求 exact claim，不猜测 durable state

### Requirement: Cancel 只向 Exact Hosted Turn 发出内存信号
系统必须（SHALL）通过 `POST /v1/agents/{agent_id}/cancel` 接受取消，请求体必须（SHALL）且只能包含 `turn_id`。只有 Postgres status 为 `running`、current Turn 与请求相等且 registry 持有同一 exact Turn 的 `running` handle、managed future 与 token 时，API 才可（SHALL）signal 其 `CancellationToken` 并返回空body的`202 Accepted`。cancel 不得（SHALL NOT）持久化 intent、隐式 resume、abort/drop AgentLoop future 或承诺最终一定 `cancelled`。

命中同一 Turn 的 `starting` claim 时必须（SHALL）返回 `409 turn_starting`；running 但未托管时必须（SHALL）返回 `409 turn_not_hosted`；Turn identity 不匹配必须（SHALL）返回 `409 stale_turn`。Tool 或其他协作方可以（SHALL）暂时忽略 cancellation，Turn 可继续 running 或抢先正常完成；最终 status 只由唯一 durable terminal event 决定。

#### Scenario: 取消当前托管 Turn
- **WHEN** 请求 Turn 与 durable current Turn 和 registry running handle 完全一致
- **THEN** API signal token并返回空body的`202 Accepted`，UI只能显示取消请求已发送

#### Scenario: 取消未托管 Running Turn
- **WHEN** Postgres Turn 为 running 但本进程没有 exact running handle
- **THEN** API 返回 `409 turn_not_hosted`，不写 cancel intent且不自动 resume

#### Scenario: Cancel 与 Starting 竞态
- **WHEN** exact Turn 已有 starting claim但 managed future 尚未安装
- **THEN** API 返回 `409 turn_starting`，不 signal 尚未成立的运行

#### Scenario: 陈旧 Cancel 不影响新 Turn
- **WHEN** 请求 Turn 不等于 current Turn
- **THEN** API 返回 `409 stale_turn`，新 Turn 的 token 与 durable state 均不改变

#### Scenario: 重复取消已取消 Turn
- **WHEN** Agent 的同一 current Turn 已处于 `cancelled`
- **THEN** API幂等返回空body的`204 No Content`；同一`finished`或`failed` Turn返回`409 turn_not_running`

#### Scenario: 正常完成先于取消生效
- **WHEN** API 已返回 202，但 AgentLoop 在观察 cancellation 前提交 `LoopFinished`
- **THEN** 最终 status 为 `finished`，系统不补写第二个 terminal event

### Requirement: Approval Resolve 与 Resume 是独立命令
系统必须（SHALL）通过 `POST /v1/agents/{agent_id}/approvals/{approval_id}` resolve approval，请求体必须（SHALL）且只能是 `{turn_id, decision}`，其中 decision 只能（SHALL）为 `approve | reject`。resolver 必须（SHALL）锁定 Agent state，先校验请求Turn与审批所属/current Turn的exact identity，并从durable Requested、Resolved、Consumed与terminal facts判定结果；terminal判定必须（SHALL）先于running-status要求，只有追加新Resolved时才要求Turn仍running。resolver不得（SHALL NOT）读取或写入approval projection table。

若所属 Turn 已 terminal，API 必须（SHALL）返回 `409 approval_invalidated`。若同一 approval 已以相同 decision resolve，必须（SHALL）幂等返回 `204 No Content`；若已以相反 decision resolve，必须（SHALL）返回 `409 approval_already_resolved`。只有 Requested 尚未 resolve 时才可（SHALL）追加唯一 `ToolApprovalResolved`；事务 commit 后 API 返回 204，再 best-effort 通知本机 waiter。通知或 NATS 失败不得（SHALL NOT）改变已提交决定。

Approval resolve 不得（SHALL NOT）隐式调用 resume。unhosted Turn 上的决定只持久化，客户端必须（SHALL）随后显式 resume；hosted Turn 的 reject 必须（SHALL）映射为 blocked Tool result并让 Agent继续，而不是取消 Turn。

#### Scenario: 首次批准 Hosted Turn
- **WHEN** exact current running Turn 的 unresolved approval 收到 `approve`
- **THEN** API 提交 `ToolApprovalResolved`、返回 204，再通知 waiter执行该 Tool

#### Scenario: 同决定重试
- **WHEN** 客户端因响应丢失再次提交相同 decision
- **THEN** API 从 ledger 识别相同 resolution并返回 204，不追加第二条 resolved event

#### Scenario: 相反决定冲突
- **WHEN** approval 已 approve 后收到 reject，或已 reject 后收到 approve
- **THEN** API 返回 `409 approval_already_resolved`，原决定不变

#### Scenario: Turn Terminal 后审批失效
- **WHEN** approval 所属 Turn 已提交任一 terminal event
- **THEN** API 返回 `409 approval_invalidated`，即使 resolver request 携带旧页面保存的 exact TurnId

#### Scenario: Approval 不存在
- **WHEN** exact Agent/Turn ledger 中不存在该 ApprovalId 的 Requested fact
- **THEN** API 返回 `404 approval_not_found`

#### Scenario: Unhosted Turn 只保存决定
- **WHEN** unresolved approval 属于 exact running Turn但本进程未托管该 Turn
- **THEN** API 持久化决定并返回 204，Web 移除 pending approval并明确显示需要 Resume，不自动恢复

#### Scenario: Reject 继续 Agent
- **WHEN** hosted 或恢复后的 waiter 观察到 reject
- **THEN** Handler 产生模型可见的 blocked tool message并继续运行，API 不把 reject解释为 cancel

### Requirement: Agent SSE 只提供短期实时 Tail
系统必须（SHALL）通过 `GET /v1/agents/{agent_id}/events` 提供严格按 AgentId 过滤、跨该 Agent 多个 Turn 的 SSE tail；已创建但尚未绑定 Session 的 idle Agent 也必须（SHALL）能够订阅。SSE data 必须（SHALL）使用 `AgentStreamFrameV1`，任何存在的SSE `id`必须（SHALL）是不透明NATS cursor。客户端可以（SHALL）使用 `Last-Event-ID` 或 `after_cursor` 二选一继续当前页面内仍被保留的短 tail；无 cursor 时必须（SHALL）只订阅新事件。cursor 不得（SHALL NOT）充当 durable sequence、history barrier 或跨页面恢复状态。

API 必须（SHALL）在 NATS subscription 已建立且服务端开始 buffering 后才发送 `stream_ready` control frame。若 cursor 过期，API 必须（SHALL）在 stream 建立前返回 `410 cursor_expired`；若 NATS 无法订阅，必须（SHALL）返回 `503 realtime_unavailable`。若 SSE 建立后服务端有界 buffer 溢出，API 必须（SHALL）在该连接直接发送无 SSE `id` 的 `stream_reset { reason: "buffer_overflow" }` control frame后关闭连接；该 frame不得写入Postgres或发布到NATS。这些 realtime failure 不得（SHALL NOT）禁用由 Postgres 支撑的 create/read/history/message/resume/cancel/approval commands。

#### Scenario: Idle Agent 先建立 SSE
- **WHEN** 客户端在首条 message 前订阅已存在的 idle Agent
- **THEN** API 建立 Agent-scoped stream并在 subscription 可接收 tail 后发送 Session/Turn 可空的 `stream_ready`

#### Scenario: Cursor 后继续 Tail
- **WHEN** 客户端提供仍在 retention 内的单一 cursor
- **THEN** API 从该 transport position 之后交付该 Agent 的 frame，不交付其他 Agent 或公开 Session 聚合流

#### Scenario: 同时提供两个 Cursor 来源
- **WHEN** 请求同时携带 `Last-Event-ID` 与 `after_cursor`，或 cursor 无法解析
- **THEN** API 返回 `400 invalid_cursor`

#### Scenario: Cursor 已过期
- **WHEN** cursor 对应的 NATS position 已淘汰
- **THEN** API 返回 `410 cursor_expired`，客户端清除 page-memory cursor并执行 PG cold bootstrap

#### Scenario: 已建立 Stream 的 Buffer 溢出
- **WHEN** SSE headers 已发送后该连接的 bounded buffer 溢出
- **THEN** API 发送无 SSE id 的 `stream_reset { reason: "buffer_overflow" }` 后关闭连接
- **AND** Web 丢弃该连接的 buffer、transient draft与cursor，并以无 cursor subscription执行cold bootstrap

#### Scenario: NATS 不可用时核心命令继续工作
- **WHEN** SSE subscription 因 NATS 故障返回 `503 realtime_unavailable`
- **THEN** Web 显示 realtime degraded 状态并通过 AgentView/history reconcile，Postgres-backed command 合同保持可用

#### Scenario: Durable Commit 先于 Realtime Publish
- **WHEN** message、approval、compaction 或 terminal event 已在 Postgres commit
- **THEN** 系统之后才可 best-effort 发布对应 frame；publish 失败不得回滚或否认 durable HTTP 结果

### Requirement: HTTP 错误使用稳定且安全的类型
所有 `/v1` Agent runtime error response 必须（SHALL）使用 `{"error":{"code":"...","message":"..."}}`。`code` 必须（SHALL）是稳定 snake_case discriminant，客户端不得（SHALL NOT）依赖 message 文本分支。library error 必须（SHALL）在独立 `error.rs` 中使用 `thiserror` 定义并保留 source chain；HTTP 边界必须（SHALL）显式映射 status/code，且只在真正处理错误的边界记录一次详细来源。

所有 JSON request body 必须（SHALL）限制为 64 KiB，超限返回 `413 request_too_large`。边界格式错误使用 400；资源不存在使用 404；durable/current-Turn 冲突使用 409；过期 cursor 使用 410；有效结构但不可用的 template/model 使用 422；durable corruption 与未分类错误使用 500；Postgres、runtime、realtime 或 shutdown 暂不可用使用 503。error body 不得（SHALL NOT）暴露 filesystem host path、SQL、NATS subject、prompt、Tool arguments/result、provider body、credential、stack trace 或底层错误文本。

#### Scenario: Stable Conflict Codes
- **WHEN** API 遇到 idempotency key 冲突、stale Turn、busy Agent、resume required、Session mismatch/busy、Turn not hosted/starting、approval conflict/invalidation 或 incompatible runtime
- **THEN** 分别返回对应稳定的 `idempotency_key_conflict`、`stale_turn`、`agent_busy`、`resume_required`、`session_mismatch`、`session_busy`、`turn_not_hosted`、`turn_starting`、`approval_already_resolved`、`approval_invalidated` 或 `runtime_incompatible` code与 409

#### Scenario: Resume Failure 不被折叠
- **WHEN** resume 分别遇到 unsupported version、损坏 durable truth、暂不可用 runtime dependency 或 Postgres failure
- **THEN** API 分别返回 `409 runtime_incompatible`、`500 durable_state_corrupt`、`503 runtime_unavailable` 或 `503 store_unavailable`

#### Scenario: Realtime Failure 不伪装成 Store Failure
- **WHEN** NATS subscription 不可用而 Postgres 正常
- **THEN** SSE 返回 `503 realtime_unavailable`，核心 HTTP 命令不返回虚假的 `store_unavailable`

#### Scenario: Payload 超限
- **WHEN** 任一 JSON command body 超过 64 KiB
- **THEN** API 在反序列化业务 payload 前返回 `413 request_too_large`，不产生 durable mutation

#### Scenario: 未分类错误不泄漏内部细节
- **WHEN** handler 遇到不能安全映射的内部失败
- **THEN** API 返回 `500 internal_error` 与固定安全 message，只在服务端 tracing 记录 source chain
