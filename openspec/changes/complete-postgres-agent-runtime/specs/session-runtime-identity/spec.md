## MODIFIED Requirements

### Requirement: Session 是长期存在的 runtime 身份
系统必须（SHALL）使用 `SessionId` 作为同一 Agent 多个 Turn 的长期关联身份。纯创建 Agent 时不得（SHALL NOT）创建或绑定 Session；首条 message admission 可以（SHALL）接受调用方提供的 UUID SessionId，省略时必须（SHALL）由服务端生成 UUIDv7。首个 `LoopStarted` 成功提交时必须（SHALL）把该 Session永久绑定到 Agent；此 UUID当前不要求在独立Session表中预先存在，因为本版本不建立Session资源表。

后续 Turn省略Session时必须（SHALL）复用既有绑定，显式值必须（SHALL）完全相同，否则返回`session_mismatch`。即使进程在`LoopStarted`之后、首条user message之前停止，已经提交的Session绑定也不得（SHALL NOT）回退或替换。Workflow的完整Session ownership与跨runtime协调延期到scheduler change，不得（SHALL NOT）在本change用claim表提前定义。

#### Scenario: 创建 Agent 尚无 Session
- **WHEN** 调用方只成功创建immutable Agent但没有发送消息
- **THEN** Agent有稳定AgentId，state中的Session和current Turn均为空

#### Scenario: 首 Turn 使用调用方 Session
- **WHEN** 首条message提供有效UUID SessionId并成功提交LoopStarted
- **THEN** Agent永久绑定该Session并创建新TurnId，不要求Session row预先存在

#### Scenario: 首 Turn 由服务端生成 Session
- **WHEN** 首条message省略SessionId
- **THEN** API生成UUIDv7并在LoopStarted事务中绑定，response返回该SessionId

#### Scenario: 后续请求不能替换 Session
- **WHEN** 已绑定Agent的后续message显式携带不同SessionId
- **THEN**请求返回`session_mismatch`，既有Session/current Turn/default model均不变

#### Scenario: Started-only 仍保留绑定
- **WHEN** LoopStarted已提交但首条user MessageAppended未提交
- **THEN** Agent仍永久绑定该Session，后续started-only reconciliation不清空绑定

### Requirement: Session 仅允许一个活跃操作
当前版本必须（SHALL）在Agent runtime范围内保证同一Session至多有一个`agent_state.status='running'`的Agent，具体由`agent_state(session_id) WHERE status='running'` partial unique index在线性化admission事务中执行。unhosted running Turn仍必须（SHALL）占用该Session；只有terminal durable event与state status在同一事务提交后，该Session才能（SHALL）接受另一个Agent Turn。

本change不得（SHALL NOT）创建`sessions`、`session_operation_claims`或claim generation，也不得（SHALL NOT）声称该partial index协调Workflow、多个store或未来scheduler。Agent/Workflow跨owner唯一性、multi-instance ownership与调度必须（SHALL）由后续scheduler PATCH统一设计。

#### Scenario: 两个 Agent 并发占用 Session
- **WHEN** 两个Agent并发尝试在同一Session开始running Turn
- **THEN**最多一个admission提交，另一个返回`session_busy`且不留下LoopStarted或state mutation

#### Scenario: Unhosted Running 仍占用 Session
- **WHEN** 进程重启后PG中的Turn仍running而本机registry为空
- **THEN**同一Session的新Agent admission仍被拒绝，只有exact resume可以重新托管原Turn

#### Scenario: Terminal 释放 Agent-only 单活约束
- **WHEN**原Turn的finished、failed或cancelled event与state更新提交
- **THEN**其state不再匹配running partial index，Session可用于后续Agent admission

#### Scenario: Workflow 协调不在当前保证内
- **WHEN**设计者需要在同一Session协调Agent与Workflow operation
- **THEN**必须使用后续scheduler方案，当前runtime不创建临时claim表冒充跨owner真相

### Requirement: Agent 持有 Turn 身份
Agent必须（SHALL）为每个成功admission创建新的`TurnId`并保存为`agent_state.current_turn_id`。同一Turn的执行、approval、cancel、进程重启与resume必须（SHALL）保持该身份不变。finished、failed或cancelled后`current_turn_id`必须（SHALL）继续指向最近Turn，直到后续合法message admission使用exact current-Turn CAS原子替换。

进程registry必须（SHALL）以exact `(agent_id,turn_id)`标识starting/running handle；hosted或unhosted变化不得（SHALL NOT）改变durable Turn identity。旧task cleanup只能（SHALL）compare-and-remove自己的exact Turn与process claim identity，不得（SHALL NOT）删除后来Turn的handle。

#### Scenario: Resume 保持 Turn 身份
- **WHEN** running Turn在进程重启后由显式resume接管
- **THEN**恢复继续使用原AgentId、SessionId和TurnId，不创建替代Turn

#### Scenario: Terminal 保留 Recent Turn
- **WHEN** current Turn提交finished、failed或cancelled
- **THEN**state保留同一current_turn_id作为下次message的CAS基准

#### Scenario: 后续 Admission 替换 Turn
- **WHEN** terminal Agent收到携带exact recent TurnId的合法message
- **THEN**系统创建新TurnId并原子替换current_turn_id，Agent和Session身份不变

#### Scenario: 陈旧 Cleanup 不删除新 Handle
- **WHEN**旧Turn cleanup与同Agent新Turn handle安装交错
- **THEN**exact Turn和process claim比较使新handle保持可用

### Requirement: Agent 可在 Turn 之间修改模型配置
`ModelConfig`必须（SHALL）是Agent可在Turn之间替换的当前默认配置，不是Session属性。创建Agent时可选model override必须（SHALL）作为完整替换应用于模板默认值；未提供则使用模板默认。新Turn也可以（SHALL）提供完整替换，任何字段不得（SHALL NOT）与旧default做隐式merge。模型、provider参数与可用性校验必须（SHALL）在任何durable mutation前完成。

`LoopStarted` runtime snapshot必须（SHALL）固定本Turn的effective model config。只有随后首条user `MessageAppended`成功提交，且effective config与`agent_state.default_model_config`不同时，系统才可（SHALL）在同一append事务更新default；相同值不得（SHALL NOT）冗余写入。LoopStarted失败或形成started-only Turn时default必须（SHALL）保持不变。resume必须（SHALL）使用目标Turn snapshot中的effective config并拒绝新的override。

#### Scenario: 创建时完整覆盖模型
- **WHEN** create提供有效model override
- **THEN**override完整替换模板model并成为Agent初始default与creation request identity

#### Scenario: 新 Turn 覆盖在首消息后生效
- **WHEN** terminal Agent以新model config开始Turn，LoopStarted和首条user message均提交
- **THEN**snapshot固定新config，首条user append仅在值变化时把它保存为后续Turn default

#### Scenario: Started-only 不修改 Default
- **WHEN** LoopStarted固定了override但首条user message未提交
- **THEN**Agent原default不变，started-only failure后下一个Turn仍从原default开始

#### Scenario: 相同配置不写入
- **WHEN** effective model config与当前default完全相同
- **THEN**snapshot仍固定该值，但message append不更新default字段或updated_at仅为该原因变化

#### Scenario: Resume 不接受新覆盖
- **WHEN**调用方恢复未完成Turn
- **THEN**runtime使用LoopStarted已固定配置，任何resume model override均被拒绝或不属于API schema

## ADDED Requirements

### Requirement: AgentId 永久关联不可变定义版本
每个`AgentId`必须（SHALL）永久关联一份创建时解析、校验且自包含的immutable resolved definition，并以一对一unique `agent_version_id`固定其内部版本身份。定义至少必须（SHALL）包含Agent名称、system prompt、按序tools、creation-time effective model config及运行时所需的非敏感版本identity；不同Agent即使来源模板名相同也必须（SHALL）拥有不同AgentId和version pin。后续状态、Turn、default model与template文件变化不得（SHALL NOT）改写定义。

#### Scenario: 同模板创建两个历史实例
- **WHEN**同一模板在两个时点分别创建Agent且中间模板发生修改
- **THEN**两个Agent各自永久关联创建时resolved definition，不通过模板名共享可变内容

#### Scenario: Prompt 与 Tools 的恢复来源
- **WHEN**`stratum-api`外层runtime编排开始新Turn或resume既有Turn
- **THEN**prompt/tools来自该Agent immutable definition，不能从当前filesystem模板或其他Agent复制

#### Scenario: Mutable Default 不回写 Definition
- **WHEN**后续Turn成功更新Agent default model
- **THEN**`agent_state.default_model_config`变化，immutable definition和creation override保持原值

### Requirement: Message Admission 使用 Exact Current-Turn CAS
每个message command必须（SHALL）显式携带nullable `expected_current_turn_id`。idle Agent只接受null；terminal Agent只接受与recent `current_turn_id`完全相同的值；running Agent不得（SHALL NOT）接受新Turn。CAS、Session绑定/校验、Agent-only Session单活、new TurnId、LoopStarted event与running state transition必须（SHALL）在同一PG事务线性化。

系统必须（SHALL）保留LoopStarted与首条user MessageAppended两个独立durable boundary。LoopStarted先绑定Session/current Turn并固定runtime snapshot，首条消息随后通过标准sink另行commit；不得（SHALL NOT）为了隐藏崩溃窗口把两者合并。API只有在managed task安装且首条user message提交后才可（SHALL）返回accepted。

#### Scenario: Idle Agent 只接受 Null CAS
- **WHEN**idle Agent收到`expected_current_turn_id=null`
- **THEN**合法请求可以创建首Turn；非null请求返回`stale_turn`

#### Scenario: Terminal Agent 比较 Recent Turn
- **WHEN**terminal Agent收到与state recent Turn完全相同的expected ID
- **THEN**admission可以原子创建下一Turn；null或陈旧ID均返回`stale_turn`

#### Scenario: 丢失 Response 的重试不创建第二 Turn
- **WHEN**第一次admission已经提交新current Turn但HTTP response丢失，调用方用旧expected identity重试
- **THEN**CAS失败且不产生第二个LoopStarted，即使第一次Turn已快速terminal

#### Scenario: LoopStarted 后崩溃可观察
- **WHEN**LoopStarted commit后、首条user MessageAppended commit前进程停止
- **THEN**Postgres保留一个running started-only Turn，由后续explicit resume按专门语义处理
