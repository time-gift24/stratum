## MODIFIED Requirements

### Requirement: AgentLoop 从事件流恢复执行
系统必须（SHALL）支持从Postgres agent-wide durable ledger恢复Agent的current exact Turn。`stratum-api`外层runtime编排必须（SHALL）读取该Turn唯一的`LoopStarted` row，以`LoopStarted.event_seq - 1`推导`base_event_seq`，并在同一一致性读取中捕获`through_event_seq = agent_state.last_event_seq`。`(base_event_seq,through_event_seq]`必须（SHALL）从该LoopStarted开始、event_seq连续无缺行、全部属于exact Agent/Session/Turn且不含terminal；任何不满足必须（SHALL）在外部动作前fail closed。

外层编排必须（SHALL）用base以内最新可用compaction summary与retained suffix，或从ledger起点full replay，物化跨Turn historical committed context，再按序应用current-Turn slice直到fixed through barrier。若kernel replay contract需要self-contained window，外层编排可以（SHALL）构造“current typed LoopStarted + historical baseline messages + current Turn后续typed events”；其他Turn的LoopStarted、terminal或Hook journal不得（SHALL NOT）伪装成current Turn event。

runtime snapshot必须（SHALL）从current LoopStarted envelope读取。kernel必须（SHALL）保持Postgres、AgentId、SessionId、TurnId、event_seq、compaction table与hosting registry无感。为了在HTTP接受前复用唯一replay validator，系统必须（SHALL）增加纯`prepare_resume` seam：它只校验typed replay window并返回绑定exact `Arc<AgentLoop>`、不可Clone/Serialize、只能consuming `run(token)`一次的opaque value；prepare不得（SHALL NOT）做I/O、append、模型、Tool或Hook调用。固定barrier内已有LoopFinished、LoopFailed或LoopCancelled时不得（SHALL NOT）resume。

#### Scenario: 从 Agent-wide Ledger 恢复 Exact Turn
- **WHEN** Agent已有多个terminal Turn，current running Turn的LoopStarted sequence为30且preflight high-water为38
- **THEN**外层编排以29为historical base，只将30..38作为exact current-Turn continuation并恢复崩溃前committed context

#### Scenario: Base 不来自 State 或 Snapshot
- **WHEN**外层编排读取current Turn恢复输入
- **THEN**state和snapshot均不含base字段，base只由LoopStarted event_seq推导

#### Scenario: Current-Turn Truth Slice 缺行
- **WHEN**30..38中缺少任一row、出现重复sequence或row identity属于其他Turn
- **THEN**resume返回`durable_state_corrupt`，不开始模型、Tool或Hook动作

#### Scenario: 过滤视图空洞不等于 Truth 缺行
- **WHEN**current slice含不向history/NATS公开的Hook journal row
- **THEN**recovery仍读取并校验该row，只有product view允许event_seq数值跳跃

#### Scenario: 终态 Turn 拒绝 Resume
- **WHEN**fixed through barrier内存在current Turn terminal event
- **THEN**resume返回typed not-running/terminal错误且不注册第二个task

#### Scenario: Prepare 与 Run 绑定同一 Runtime
- **WHEN**loop A成功prepare而系统另有配置不同的loop B
- **THEN**prepared value自持loop A且只能consuming run一次，不存在交给loop B执行的入口

#### Scenario: Kernel 保持存储无关
- **WHEN**外层编排完成PG barrier、版本、compaction baseline和identity校验
- **THEN**AgentLoop只接收已组装prompt/runtime/typed events与CancellationToken，不查询PG或registry

### Requirement: 恢复时 Tool 结果对账
Tool执行的唯一最终durable结果必须（SHALL）是`MessageAppended(role=tool,tool_call_id=CallId,content=final JSON)`；Tool error同样必须（SHALL）编码为该role=tool message，系统不得（SHALL NOT）增加`ToolExecutionCompleted`。raw Tool output不得（SHALL NOT）直接append；`AfterToolCall`必须（SHALL）先生成durable-safe result，可以执行业务变换与脱敏。若output仍含typed secret/token/credential value或无法安全转换，系统必须（SHALL）丢弃raw output并只提交安全结构化tool error，secret不得进入event、NATS或日志。

恢复重建时，committed tool result必须（SHALL）构成紧邻前序assistant `tool_calls`的精确有序前缀。未知CallId、重复result、稀疏result、乱序result或脱离assistant group的result必须（SHALL）作为`durable_state_corrupt` fail closed。`ToolExecutionStarted`存在但同CallId的tool message不存在表示外部结果未知；resume必须（SHALL）以同一CallId按at-least-once语义只重试缺失有序后缀。已有committed result不得（SHALL NOT）重试，runtime也不得（SHALL NOT）发明AttemptId或替外部服务定义通用幂等标准。

#### Scenario: 缺失后缀以同一 CallId 重试
- **WHEN**assistant有三个tool_calls，而barrier内只有前两个role=tool result
- **THEN**resume保留前两个result，只以第三个原CallId重新执行缺失后缀

#### Scenario: Started 但 Result 未知
- **WHEN**第三个CallId已有ToolExecutionStarted但result message提交前进程停止
- **THEN**resume把结果视为未知并重试同一CallId，不追加猜测性completion

#### Scenario: 已提交 Result 不重试
- **WHEN**ToolExecutionStarted后已有同CallId的role=tool MessageAppended
- **THEN**recovery将调用视为完成并继续iteration，不再次调用Tool service

#### Scenario: 乱序 Result Fail Closed
- **WHEN**result跳过前序call、重复CallId或引用未知call
- **THEN**resume返回`durable_state_corrupt`且任何外部动作均未开始

#### Scenario: Tool Error 也是最终 Result
- **WHEN**Tool或AfterToolCall产生模型可见错误JSON
- **THEN**错误作为同CallId的role=tool MessageAppended恢复，不新增另一completion fact

#### Scenario: Secret-bearing Result 只持久化安全错误
- **WHEN**Tool返回含typed credential value且AfterToolCall无法生成durable-safe result
- **THEN**raw result被丢弃，同CallId只提交不含secret的结构化role=tool error

## ADDED Requirements

### Requirement: 历史 Terminal Turn 规范化未闭合 Tool Group
跨Turn historical base materialization必须（SHALL）按Turn terminal boundary处理未闭合trailing Tool group。若failed或cancelled历史Turn在terminal前以assistant tool_calls与零个或部分精确有序results结束：零result时必须（SHALL）只从provider context view移除该trailing assistant group；存在`k>0`个result时必须（SHALL）保留该前缀，并在内存view中把assistant tool_calls截为相同前缀。原durable events、API history与消息内容不得（SHALL NOT）删除或改写，也不得（SHALL NOT）伪造未发生的result。

该规则不得（SHALL NOT）应用于current running Turn。finished历史Turn出现未闭合group，或failed/cancelled Turn的缺口不是terminal trailing group时，必须（SHALL）视为`durable_state_corrupt`。

#### Scenario: 等待 Approval 时取消
- **WHEN**历史Turn在assistant tool_calls提交后、任何result前durable cancelled
- **THEN**新Turn provider context不含该trailing group，永久history仍展示assistant与cancellation marker

#### Scenario: 部分 Result 后失败
- **WHEN**历史failed Turn只提交前k个有序result
- **THEN**内存baseline保留k个result并将assistant view截为前k个calls

#### Scenario: Current Running Turn 不被规范化
- **WHEN**current running Turn只有有序result前缀
- **THEN**historical normalizer不触碰它，kernel只重试缺失后缀

#### Scenario: 非 Terminal-Trailing 缺口损坏
- **WHEN**未闭合group位于历史中间或所属Turn标记finished
- **THEN**resume在执行前返回`durable_state_corrupt`

### Requirement: Resume 只托管 Exact Running Unhosted Turn
Postgres `agent_state`必须（SHALL）是current Turn status唯一durable truth。进程registry必须（SHALL）只保存exact `(agent_id,turn_id)`的process-local starting/running handle、managed future、唯一claim identity与CancellationToken，不得（SHALL NOT）写入PG lease、claim或hosting status。

resume command必须（SHALL）携带exact TurnId，且仅在Agent status为running、请求Turn等于current Turn、本进程没有该exact Turn的starting/running handle时托管。并发resume必须（SHALL）由registry原子claim保证至多一个task启动；相同exact Turn已经starting/running时必须（SHALL）幂等报告已托管。cleanup只能（SHALL）按exact Turn与process claim identity删除自己的handle。

#### Scenario: 重启后接管 Unhosted Turn
- **WHEN**PG中Turn仍running而新进程registry为空
- **THEN**exact resume安装process-local starting handle并开始preflight，不修改durable Session/Turn identity

#### Scenario: 并发 Resume 只有一个启动
- **WHEN**两个请求并发resume同一running/unhosted Turn
- **THEN**只有一个取得registry claim并启动task，另一个观察到相同starting/running handle

#### Scenario: 错误 Turn 被拒绝
- **WHEN**请求TurnId不等于current_turn_id
- **THEN**resume返回`stale_turn`且不创建registry entry

#### Scenario: 非 Running 被拒绝
- **WHEN**Agent为idle、finished、failed或cancelled
- **THEN**resume返回`turn_not_running`且不修改durable state

#### Scenario: Cleanup 不删除后来 Handle
- **WHEN**旧task cleanup与同Agent后来Turn或同Turn新claim的handle安装交错
- **THEN**cleanup只删除仍匹配旧Turn和旧claim identity的entry

### Requirement: Resume 从 Compaction Summary 或 Full Replay 构造 Base
对于`base_event_seq`，外层编排必须（SHALL）选择不大于base的最新`transcript_compactions` companion，以其单一summary开始并从`retained_from_event_seq`读取到base的后续消息和terminal control boundaries。locator与pointer有效时，结果必须（SHALL）和从event_seq 1 full replay完全等价。current Turn在base之后的TranscriptCompacted不得（SHALL NOT）提前并入historical base，而应（SHALL）在exact current-Turn replay中按序应用。

如果base前没有compaction，或companion identity/summary完整但retained pointer不能作为加速起点校验，外层编排必须（SHALL）在相同barrier内从ledger起点内存full replay；不得（SHALL NOT）写repair row、调用filesystem或提供rebuild API。若durable ledger存在TranscriptCompacted discriminator却缺少必需companion/summary，或已知summary本身畸形，则truth不完整，必须（SHALL）返回`durable_state_corrupt`而非fallback。

#### Scenario: Latest Summary 快速恢复
- **WHEN**base内存在多个compaction且最新companion和pointer均有效
- **THEN**recovery使用最新summary与retained suffix，得到与full replay相同的context

#### Scenario: Pointer 无效但 Summary 完整
- **WHEN**companion summary和identity有效但retained pointer无法校验
- **THEN**recovery忽略pointer并从ledger起点内存full replay，不修表

#### Scenario: Required Companion 缺失
- **WHEN**truth range中存在TranscriptCompacted discriminator但其companion或单一summary缺失
- **THEN**resume返回`durable_state_corrupt`，因为full replay也无法重建压缩事实

#### Scenario: Current-Turn Compaction 按序应用
- **WHEN**current Turn在base之后、through之前提交TranscriptCompacted
- **THEN**historical locator不吸收它，kernel replay按current-Turn sequence应用一次

### Requirement: Resume Preflight 区分 Preamble、Incompatible、Corrupt 与 Unavailable
resume必须（SHALL）在任何model、Tool或Hook action前完成exact identity、base/through barrier、definition/runtime versions、runtime component availability、event continuity、terminal、compaction、Hook journal与Tool result preflight。

若fixed current-Turn slice恰好只有LoopStarted，则它是started-only Turn。resume必须（SHALL）通过正常durable append transaction追加唯一安全`LoopFailed`并把state原子更新为failed；不得（SHALL NOT）进入AgentLoop或启动外部动作。API随后返回`turn_preamble_incomplete`。

除此之外，preflight失败不得（SHALL NOT）猜测性追加terminal。未知但结构完整的definition/event/runtime snapshot version必须（SHALL）返回`runtime_incompatible`；已知版本内容畸形、truth缺行/identity错误、compaction core fact、journal或Tool prefix损坏必须（SHALL）返回`durable_state_corrupt`；固定runtime identity合法但当前provider/model/tool/skill/extension/hook implementation不可用必须（SHALL）返回`runtime_unavailable`。失败后只移除（SHALL）本次exact process claim，PG Turn保持running/unhosted。

#### Scenario: Started-only 原子失败
- **WHEN**fixed slice只有LoopStarted
- **THEN**系统追加唯一LoopFailed、state变为failed并保留current Turn，不调用kernel或外部系统

#### Scenario: Started-only Commit 失败
- **WHEN**LoopFailed transaction明确rollback
- **THEN**Turn保持running/unhosted并返回保留source chain的storage error

#### Scenario: Started-only Commit 结果不确定
- **WHEN**transaction commit结果无法确认
- **THEN**系统按exact Agent/Turn重读PG后再映射响应，不猜测terminal是否提交

#### Scenario: Runtime Version 不支持
- **WHEN**snapshot shape完整但version高于当前支持范围
- **THEN**resume返回`runtime_incompatible`，Turn保持running/unhosted

#### Scenario: Durable State 损坏
- **WHEN**已知version snapshot畸形、truth不连续或journal/result顺序非法
- **THEN**resume返回`durable_state_corrupt`且不追加terminal

#### Scenario: Runtime Component 暂不可用
- **WHEN**snapshot有效但固定runtime component当前无法构造
- **THEN**resume返回`runtime_unavailable`，Turn保持running/unhosted

## REMOVED Requirements

### Requirement: Filesystem 耐久事件后端
**Reason**: Agent execution truth统一为Postgres durable ledger；filesystem只保留template与业务文件能力。

**Migration**: 删除filesystem sink、JSONL reader、run directory、截断尾行兼容与runtime fallback。既有filesystem execution data不迁移、不读取，也不由新runtime自动删除。

### Requirement: 压缩检查点索引加速恢复
**Reason**: `compact.jsonl`、物理行offset、`window_start_line`与digest属于已删除filesystem backend；新compaction fact由Postgres durable discriminator与单一summary companion原子表达。

**Migration**: recovery使用`transcript_compactions`；仅retained pointer失效时内存full replay，必需companion/summary缺失时按durable corruption fail closed，不回退filesystem或在线rebuild。
