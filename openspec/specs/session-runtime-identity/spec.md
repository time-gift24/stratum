# session-runtime-identity Specification

## Purpose
定义 AgentId、AgentRuntimeId、SessionId 与 TurnId 的职责、绑定、单活、模型配置和历史隔离边界。
## Requirements
### Requirement: Session 是长期存在的 runtime 身份
系统必须（SHALL）使用 `SessionId` 作为一个 AgentRuntime 跨多个 Turn 的长期关联身份；`AgentRuntimeId` 则标识该长期运行聚合本身。纯创建 AgentRuntime 时不得（SHALL NOT）创建或绑定 Session。首条 message admission可以（SHALL）接受调用方提供的 UUID SessionId，省略时必须（SHALL）由服务端生成 UUIDv7。首个 `LoopStarted` 成功提交时必须（SHALL）把该 Session永久绑定到 exact `agent_states.id: AgentRuntimeId`；此 UUID当前不要求在独立Session表中预先存在，因为本版本不建立Session资源表。

后续 Turn省略Session时必须（SHALL）复用既有绑定，显式值必须（SHALL）完全相同，否则返回`session_mismatch`。即使进程在`LoopStarted`之后、首条user message之前停止，已经提交的Session绑定也不得（SHALL NOT）回退或替换。Workflow的完整Session ownership与跨runtime协调延期到scheduler change，不得（SHALL NOT）在本change用claim表提前定义。

#### Scenario: 创建 AgentRuntime 尚无 Session
- **WHEN** 调用方只成功创建idle AgentRuntime但没有发送消息
- **THEN** runtime有稳定`AgentRuntimeId`与pinned `AgentId`，state中的Session和current Turn均为空

#### Scenario: 首 Turn 使用调用方 Session
- **WHEN** 首条message提供有效UUID SessionId并成功提交`LoopStarted`
- **THEN** exact AgentRuntime永久绑定该Session并创建新TurnId，不要求Session row预先存在

#### Scenario: 首 Turn 由服务端生成 Session
- **WHEN** 首条message省略SessionId
- **THEN** API生成UUIDv7并在`LoopStarted`事务中绑定，response返回该SessionId

#### Scenario: 多个 Turn 共享一个 Session
- **WHEN** 同一AgentRuntime在已有Session中处理后续用户Turn
- **THEN** 后续Turn使用已有`AgentRuntimeId`、`AgentId`与`SessionId`，并使用新的`TurnId`

#### Scenario: 后续请求不能替换 Session
- **WHEN** 已绑定runtime的后续message显式携带不同SessionId
- **THEN** 请求返回`session_mismatch`，既有Session/current Turn/model_config均不变

#### Scenario: Started-only 仍保留绑定
- **WHEN** `LoopStarted`已提交但首条user `MessageAppended`未提交
- **THEN** runtime仍永久绑定该Session，后续started-only reconciliation不清空绑定

#### Scenario: Session 内 Workflow 版本发生变化
- **WHEN** 未来scheduler允许Session中的后续操作选择不同Workflow版本
- **THEN** Session保持原有`SessionId`，不得从Workflow图或版本派生替代身份

### Requirement: Session 仅允许一个活跃操作
当前版本必须（SHALL）在AgentRuntime范围内保证同一Session至多有一个`agent_states.status='running'`的runtime，具体由`agent_states(session_id) WHERE status='running'` partial unique index在线性化admission事务中执行。unhosted running Turn仍必须（SHALL）占用该Session；只有terminal durable event与state status在同一事务提交后，该Session才能（SHALL）接受另一个AgentRuntime Turn。

本change不得（SHALL NOT）创建`sessions`、`session_operation_claims`或claim generation，也不得（SHALL NOT）声称该partial index协调Workflow、多个store、multi-instance hosting或未来scheduler。Agent/Workflow跨owner唯一性、ownership lease/fencing与调度必须（SHALL）由后续scheduler PATCH统一设计。

#### Scenario: 两个 AgentRuntime 并发占用 Session
- **WHEN** 两个runtime并发尝试在同一Session开始running Turn
- **THEN** 最多一个admission提交，另一个返回`session_busy`且不留下`LoopStarted`或state mutation

#### Scenario: Unhosted Running 仍占用 Session
- **WHEN** 进程重启后PG中的Turn仍running而本机registry为空
- **THEN** 同一Session的新runtime admission仍被拒绝，只有exact resume可以重新托管原Turn

#### Scenario: Terminal 释放 Runtime-only 单活约束
- **WHEN** 原Turn的finished、failed或cancelled event与state更新提交
- **THEN** 其state不再匹配running partial index，Session可用于后续AgentRuntime admission

#### Scenario: Agent 与 Workflow 操作冲突交给 Scheduler
- **WHEN** 设计者需要保证同一Session中的AgentRuntime与Workflow操作不能同时活跃
- **THEN** 必须使用后续scheduler的统一owner方案，当前runtime不得用临时claim表声称已经提供该保证

### Requirement: 推迟 Node activation 与 attempt 身份
本基线必须（SHALL）使用 `SessionId`、不可变的 `WorkflowVersionId` 和 `NodeId` 标识活跃 Workflow 节点。在尚不支持循环、节点重入和重试时，本基线不得（SHALL NOT）要求 `NodeExecutionId` 或 `AttemptId`。

#### Scenario: 无环 Workflow 中的 Node 身份
- **WHEN** 某个节点在 Session 内唯一活跃的 Workflow 操作中仅运行一次
- **THEN** 其 Session、Workflow 版本和 Node 身份能够唯一定位其 runtime event

### Requirement: Host 提供 AgentLoop runtime context
API/storage组合边界必须（SHALL）使用`AgentRuntimeId`定位`agent_states`、pinned `AgentId`、durable ledger、registry与realtime transport，并在外层通过AgentRuntime-scoped sink绑定执行事实。`AgentRuntimeId`不得（SHALL NOT）进入kernel `AgentLoop`、runtime snapshot、kernel durable variant或`AgentLocation`。

在启动或恢复Turn之前，host仍必须（SHALL）向`AgentLoop`提供`SessionId`与`AgentLocation`，并以state pinned `AgentId`加载immutable definition。`AgentLoop`不得（SHALL NOT）创建、替换或推断Session/AgentRuntime identity；外层必须（SHALL）校验每个kernel payload中的`AgentId`等于state pin。

#### Scenario: Agent 直接运行 Turn
- **WHEN** API host在Session中直接启动AgentRuntime的Turn
- **THEN** 外层以`AgentRuntimeId`scope sink与ledger，AgentLoop只接收Session身份、pinned definition与`AgentLocation::Direct`

#### Scenario: Workflow Agent 节点运行 Turn
- **WHEN** 未来Workflow节点通过scheduler启动AgentRuntime Turn
- **THEN** 外层保留AgentRuntime identity，AgentLoop接收Session身份以及包含immutable Workflow版本与`NodeId`的Workflow-node location

#### Scenario: Resume 不把 Runtime Identity 推入 Kernel
- **WHEN** API按`AgentRuntimeId`加载state、definition与durable rows后恢复Turn
- **THEN** replay authority只使用kernel已有的Agent/Session/Turn/runtime snapshot输入，AgentRuntime scope继续由外层sink和registry持有

### Requirement: AgentRuntime 持有 Turn 身份
本合同中的运行实例必须（SHALL）由`AgentRuntimeId`定位。外层orchestration必须（SHALL）为每个成功admission生成新的`TurnId`并保存为`agent_states.current_turn_id`；同一Turn的执行、approval、cancel、进程重启与resume必须（SHALL）保持该身份不变。finished、failed或cancelled后`current_turn_id`必须（SHALL）继续指向最近Turn，直到后续合法message admission使用exact current-Turn CAS原子替换。

进程registry必须（SHALL）以exact `(agent_runtime_id, turn_id)`标识starting/running handle；hosted或unhosted变化不得（SHALL NOT）改变durable Turn identity。旧task cleanup只能（SHALL）compare-and-remove自己的exact AgentRuntime、Turn与process claim identity，不得（SHALL NOT）删除后来Turn或共享同一`AgentId`的另一runtime handle。

#### Scenario: Resume 保持 Turn 身份
- **WHEN** running Turn在进程重启后由显式resume接管
- **THEN** 恢复继续使用原`AgentRuntimeId`、pinned `AgentId`、`SessionId`与`TurnId`，不创建替代Turn

#### Scenario: Terminal 保留 Recent Turn
- **WHEN** current Turn提交finished、failed或cancelled
- **THEN** state保留同一`current_turn_id`作为下次message的CAS基准

#### Scenario: 后续 Admission 替换 Turn
- **WHEN** terminal runtime收到携带exact recent TurnId的合法message
- **THEN** 系统创建新TurnId并原子替换`current_turn_id`，AgentRuntime、pinned Agent与Session身份不变

#### Scenario: 陈旧 Cleanup 不删除新 Handle
- **WHEN** 旧Turn cleanup与同runtime新Turn handle安装交错
- **THEN** exact AgentRuntime、Turn和process claim比较使新handle保持可用

#### Scenario: 共享 AgentId 的 Runtime 不共享 Turn
- **WHEN** 两个AgentRuntime pin同一`AgentId`
- **THEN** 两者分别持有current/recent Turn与registry handle，不以共享definition identity合并执行

### Requirement: AgentRuntime 可在 Turn 之间修改模型配置
`agent_states.model_config`必须（SHALL）是每个AgentRuntime唯一可变的当前模型配置，不是Session属性，也不得（SHALL NOT）回写immutable template definition。创建runtime时可选model override必须（SHALL）作为完整替换初始化该字段；未提供则使用pinned template definition的默认模型。state不得（SHALL NOT）保存第二个creation/default model字段。

新Turn可以（SHALL）提供完整model replacement，任何字段不得（SHALL NOT）与旧config隐式merge。模型、provider参数与可用性校验必须（SHALL）在任何durable mutation前完成。`LoopStarted` runtime snapshot必须（SHALL）固定本Turn的effective model config。只有随后首条user `MessageAppended`成功提交，且effective config与`agent_states.model_config`不同时，系统才可（SHALL）在同一append事务更新该字段；相同值不得（SHALL NOT）冗余写入。started-only Turn不得（SHALL NOT）修改state model。resume必须（SHALL）使用目标Turn snapshot中的effective config并拒绝新的override。

#### Scenario: 创建时完整覆盖模型
- **WHEN** runtime create提供有效model override
- **THEN** override完整替换template默认值并初始化该runtime的唯一`model_config`，但不进入immutable definition equality

#### Scenario: 未提供创建覆盖
- **WHEN** runtime create省略model override
- **THEN** `agent_states.model_config`初始化为pinned template definition中的默认模型

#### Scenario: 后续 Turn 修改 LLM 参数
- **WHEN** terminal runtime以新的有效`ModelConfig`开始后续Turn且首条user message成功提交
- **THEN** snapshot固定新config，append仅在值变化时把同一字段更新为再后续Turn的配置

#### Scenario: 模型配置修改未被接受
- **WHEN** 新配置无效、模型不可用、Session忙、`LoopStarted`失败或首条user message未提交
- **THEN** runtime已持久化的`model_config`保持不变

#### Scenario: 相同配置不写入
- **WHEN** effective model config与当前state值完全相同
- **THEN** snapshot仍固定该值，但message append不因model执行冗余update

#### Scenario: 恢复固定的 Turn 配置
- **WHEN** 未完成Turn在进程重启后恢复
- **THEN** resume使用该Turn snapshot已固定的`ModelConfig`，不接受或应用更晚的override

### Requirement: 对话历史按 AgentRuntimeId 隔离
对话历史必须（SHALL）归`AgentRuntimeId`所有。`durable_events`、compaction、history barrier、approval、usage、realtime subject与恢复上下文都必须（SHALL）按exact AgentRuntimeId隔离。同一Session中的不同runtime，以及pin同一`AgentId`的不同runtime，不得（SHALL NOT）隐式共享对话历史。

#### Scenario: 共享 AgentId 的 Runtime 使用独立历史
- **WHEN** 两个AgentRuntime pin同一immutable `AgentId`
- **THEN** 每个runtime只加载自己的AgentRuntime-wide ledger，不读取另一个runtime的message、Tool result、compaction或approval

#### Scenario: Workflow Agent 节点使用独立历史
- **WHEN** Workflow节点启动一个独立`AgentRuntimeId`
- **THEN** 该runtime不加载同一Session内另一个runtime的对话历史，即使二者pin同一`AgentId`

#### Scenario: Session 状态不是对话历史
- **WHEN** 未来由Hook将Session状态或结果暴露给Agent
- **THEN** 该context不改变对话历史归哪个AgentRuntimeId所有

### Requirement: AgentId 标识可复用的不可变 Template 版本
每个`AgentId`必须（SHALL）永久标识`agents`中的一条immutable template版本row；row由作者命名的exact `(name, version string tag)`唯一定位。version tag必须（SHALL）来自template TOML，大小写敏感且没有排序语义；create request不得（SHALL NOT）指定或覆盖它。

exact pair已存在且`definition_schema_version + canonical resolved_definition`严格相同时必须（SHALL）复用原`AgentId`，定义不同时必须（SHALL）返回`agent_version_conflict`且不得覆盖；不同tag即使定义相同也必须（SHALL）创建新`AgentId`。同一`AgentId`可以（SHALL）被多个AgentRuntime pin；后续runtime state、Turn model override与template文件变化不得（SHALL NOT）改写definition。

#### Scenario: 同一 Template 版本被多个 Runtime 复用
- **WHEN** 两个不同create key读取same name/tag且canonical definition相同
- **THEN** 它们获得不同`AgentRuntimeId`但pin同一`AgentId`

#### Scenario: Prompt 与 Tools 的恢复来源
- **WHEN** API外层开始新Turn或resume既有Turn
- **THEN** prompt与ordered tools来自state pinned `AgentId`的immutable definition，不从当前filesystem template或另一runtime复制

#### Scenario: Mutable Model 不回写 Definition
- **WHEN** 后续Turn成功更新某个runtime的`model_config`
- **THEN** 只有该`agent_states` row变化，immutable definition与共享它的其他runtime保持不变

#### Scenario: 作者复用 Tag 修改定义
- **WHEN** exact name/tag已存在但当前template canonical definition不同
- **THEN** create返回`agent_version_conflict`，既有AgentId与所有runtime pin不变

### Requirement: AgentRuntimeId 标识长期运行聚合
每个`AgentRuntimeId`必须（SHALL）永久标识`agent_states`中的一个长期运行聚合，并在整个生命期内pin exactly one `AgentId`。`AgentRuntimeId`与`AgentId`都必须（SHALL）是服务端生成的UUIDv7 newtype，客户端不得（SHALL NOT）指定。一个AgentRuntime可以（SHALL）跨多个Turn，terminal状态不得（SHALL NOT）终结或重新编号该identity。

所有运行态route、command、query、registry、ledger、dispatcher、NATS subject、SSE frame与tracing primary scope必须（SHALL）使用AgentRuntimeId；只有需要说明immutable definition provenance时才附带pinned AgentId/name/version。`AgentRuntimeId`不得（SHALL NOT）进入kernel AgentLoop。

#### Scenario: Terminal 后 Runtime Identity 保持
- **WHEN** AgentRuntime的Turn进入finished、failed或cancelled后又admit新Turn
- **THEN** `AgentRuntimeId`与pinned`AgentId`不变，仅current Turn、status和runtime-owned state演进

#### Scenario: 运行态 URL 不使用 AgentId
- **WHEN** 客户端读取view、发送message、分页history、订阅events、resume、cancel或resolve approval
- **THEN** URL使用`/v1/agent-runtimes/{agent_runtime_id}/...`，不得用共享`AgentId`选择运行实例

### Requirement: Message Admission 使用 Exact Current-Turn CAS
每个message command必须（SHALL）显式携带nullable `expected_current_turn_id`。idle AgentRuntime只接受null；terminal runtime只接受与recent `current_turn_id`完全相同的值；running runtime不得（SHALL NOT）接受新Turn。CAS、Session绑定/校验、AgentRuntime-only Session单活、new TurnId、AgentRuntime-wide `LoopStarted` event与running state transition必须（SHALL）在同一PG事务线性化。

系统必须（SHALL）保留`LoopStarted`与首条user `MessageAppended`两个独立durable boundary。`LoopStarted`先绑定Session/current Turn并固定pinned `AgentId`与runtime snapshot，首条消息随后通过标准sink另行commit；不得（SHALL NOT）为了隐藏崩溃窗口把两者合并。API只有在managed task安装且首条user message提交后才可（SHALL）返回accepted。

#### Scenario: Idle Runtime 只接受 Null CAS
- **WHEN** idle AgentRuntime收到`expected_current_turn_id=null`
- **THEN** 合法请求可以创建首Turn；非null请求返回`stale_turn`

#### Scenario: Terminal Runtime 比较 Recent Turn
- **WHEN** terminal AgentRuntime收到与state recent Turn完全相同的expected ID
- **THEN** admission可以原子创建下一Turn；null或陈旧ID均返回`stale_turn`

#### Scenario: 丢失 Response 的重试不创建第二 Turn
- **WHEN** 第一次admission已提交新current Turn但HTTP response丢失，调用方用旧expected identity重试
- **THEN** CAS失败且不产生第二个`LoopStarted`，即使第一次Turn已快速terminal

#### Scenario: LoopStarted 后崩溃可观察
- **WHEN** `LoopStarted` commit后、首条user `MessageAppended` commit前进程停止
- **THEN** Postgres保留一个running started-only Turn，由后续explicit resume按专门语义处理
