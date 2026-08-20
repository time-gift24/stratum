# agent-hook-runtime Specification

## Purpose
定义 AgentLoop 的五个 Hook 边界、执行顺序、journal 恢复与审批组合约束，同时保持 kernel 对运行时基础设施无感。
## Requirements
### Requirement: AgentLoop 接受单一 Hook Runtime
`AgentLoopBuilder` 必须（SHALL）允许调用方注入一个实现核心 Hook 合同（`transform_context`、`transform_tool_call`、`decide_tool_call`、`after_tool_call`、`prepare_next_turn`）的单一 `HookRuntime`。未注入自定义 Runtime 时必须（SHALL）使用 No-op Runtime，并保持没有 Hook 时已有的模型请求、耐久事件、Tool 调用、消息和终态行为。

#### Scenario: 默认 No-op Runtime
- **WHEN** 调用方没有为 AgentLoop 注入 Hook Runtime
- **THEN** 相同的模型响应和 Tool outcome 产生与变更前相同的请求、耐久事件、Tool 调用、消息和循环结果

#### Scenario: 注入自定义 Runtime
- **WHEN** 调用方通过 AgentLoopBuilder 注入自定义 Hook Runtime
- **THEN** AgentLoop 在对应控制流边界调用该 Runtime，并且不要求 AgentLoop 了解 Handler 列表、Session、journal 或 EventBus

### Requirement: Transform Context 只变换当前模型请求
AgentLoop 必须（SHALL）在每次模型请求开始前调用 `transform_context`。Runtime 必须（SHALL）经由公共快照接收当前迭代和 committed LoopContext 的借用视图（含本次待消费 Inject），并且必须（SHALL）只能保持原 context 或提交增量 `ContextPatch`（`ReplaceSystemPrompt`、`DropHistory { upto }`、`RewriteHistory { upto, summary }`）调整当前请求视图。Runtime 不得（SHALL NOT）提交整个替代 context；patch 不得（SHALL NOT）回写 committed transcript、产生 durable message 或出现在 `LoopOutcome.new_messages` 中。

#### Scenario: 保持原 Context
- **WHEN** transform-context decision 是 Unchanged
- **THEN** AgentLoop 使用 committed system prompt、history 和本次一次性 Inject message 构造模型请求

#### Scenario: 替换当前请求 Context
- **WHEN** transform-context decision 提供 ContextPatch
- **THEN** 当前模型请求使用应用 patch 后的 request view，而下一次迭代仍从未被改写的 committed context 构造新 request view

#### Scenario: Transform Context 失败
- **WHEN** transform_context 返回类型化失败或无效 decision
- **THEN** AgentLoop 在发起对应模型请求前 fail closed，且不把替代内容提交为 Agent 消息

#### Scenario: Patch 不切断 Tool 配对
- **WHEN** patch 的 upto 越界、不落消息边界或切断 tool_call/tool_result 配对
- **THEN** AgentLoop 返回 `HookFailure::InvalidOutput`，不发起对应模型请求

#### Scenario: Patch 即 Journal 记录
- **WHEN** patch decision 被提交进 journal
- **THEN** 记录内容为 patch 本身；resume 通过事件流重建 committed context 并回放 patch，得到与崩溃前一致的 request view

### Requirement: After Tool Call 可以替换模型可见结果
对于每个被 provider 授权的 Tool cycle，AgentLoop 必须（SHALL）在产生模型可见 Tool result 后、提交该消息前调用 `after_tool_call`。Runtime 必须（SHALL）经由公共快照接收完整对话历史，并同时接收携带授权元数据与 `ToolSpec` 的工具目标视图，可以保留结果或替换 JSON result；AgentLoop 必须（SHALL）保留原 `CallId` 和 Tool message role。

#### Scenario: 保留 Tool Result
- **WHEN** after-tool decision 是 Keep
- **THEN** AgentLoop 原样提交 ToolExecutor 或 decide-tool Block 产生的模型可见结果

#### Scenario: 替换 Tool Result
- **WHEN** after-tool decision 提供替代 JSON result
- **THEN** AgentLoop 使用原 `CallId` 构造并耐久提交替代 Tool message，原结果不会进入模型 context

#### Scenario: Tool 完成后 After Hook 失败
- **WHEN** ToolExecutor 已返回结果但 after_tool_call 返回失败、超时或无效 decision
- **THEN** AgentLoop 不提交伪造或未变换的 Tool result，并以类型化 Hook 错误 fail closed

### Requirement: Prepare Next Turn 控制下一次模型迭代
当一个被授权的 Tool cycle 的全部模型可见结果已耐久提交后，AgentLoop 必须（SHALL）调用 `prepare_next_turn`。Decision 必须（SHALL）是 Continue、Stop、Inject 非空 User message 列表，或携带切割点与摘要的 Compact（其执行语义由 context-compaction 能力定义）。

#### Scenario: 继续下一迭代
- **WHEN** prepare-next-turn decision 是 Continue
- **THEN** AgentLoop 提交当前 iteration 完成边界并开始下一次模型迭代

#### Scenario: Hook 主动停止
- **WHEN** prepare-next-turn decision 是 Stop
- **THEN** AgentLoop 提交当前 iteration 和 LoopFinished，并以 `LoopCompletionReason::HookStopped` 结束，而不把该结果伪装成 provider FinishReason

#### Scenario: 注入下一次请求消息
- **WHEN** prepare-next-turn decision 注入一条或多条合法 User message
- **THEN** 这些消息只加入下一次模型 request view、只消费一次，并且不产生 durable Agent message、不进入 Agent history 或 LoopOutcome.new_messages

#### Scenario: 拒绝非法 Inject
- **WHEN** Inject 为空，或包含非 User role、Tool call、reasoning content 或 tool-call identity
- **THEN** AgentLoop 返回 `HookFailure::InvalidOutput`，不开始下一次模型请求

#### Scenario: Compact 触发持久压缩
- **WHEN** prepare-next-turn decision 是携带合法切割点与 system 摘要消息的 Compact
- **THEN** AgentLoop 先提交该 decision 的 Completed 记录，再执行 durable 基线改写，随后提交迭代边界并开始下一次模型迭代

#### Scenario: 压缩后快照即为新基线
- **WHEN** 压缩完成后的下一次迭代调用 transform_context 或 prepare_next_turn
- **THEN** HookSnapshot 的 context 是压缩后的 committed 基线

### Requirement: 所有 Hook 共享取消、Deadline 与错误语义
AgentLoop 必须（SHALL）为每个 Hook 提供当前 CancellationToken，并且必须（SHALL）在核心循环边界强制取消与 timeout，而不是依赖具体 Runtime 自行遵守。Hook deadline 必须（SHALL）按 HookPoint 独立配置并带有默认值；`decide_tool_call` 的默认配置必须（SHALL）是无 deadline（仅受取消约束），以容纳交互式审批的长时间等待。Decision-affecting Hook 失败必须（SHALL）阻止受影响的模型、Tool、message 或 iteration action 继续。

#### Scenario: 调用前已经取消
- **WHEN** 任一 Hook 即将调用时 Turn CancellationToken 已取消
- **THEN** AgentLoop 不调用 Runtime，并进入既有 loop cancellation 终态

#### Scenario: Hook 执行中取消
- **WHEN** 任一 Hook 尚未返回时 Turn CancellationToken 被取消
- **THEN** AgentLoop 停止等待该 Hook，不执行其后受影响的动作，并进入 loop cancellation 终态

#### Scenario: Hook 超过 Deadline
- **WHEN** 配置了 deadline 的 Hook 在其绝对 deadline 前没有返回
- **THEN** AgentLoop 以包含对应 HookPoint 和 `HookFailure::TimedOut` 的类型化错误 fail closed

#### Scenario: Decide Tool Call 默认无 Deadline
- **WHEN** decide_tool_call 使用默认 deadline 配置且长时间未返回
- **THEN** AgentLoop 持续等待该 Hook，只在 Turn CancellationToken 取消时离开等待

#### Scenario: Runtime 返回失败
- **WHEN** 任一 Hook Runtime 返回安全的类型化 HookFailure
- **THEN** AgentLoop 保留 HookPoint 与失败分类，且公开错误、trace 和 durable terminal event 不包含 prompt、Tool 参数、Tool result 或 Runtime 内部错误正文

#### Scenario: 四个 Hook 的失败矩阵
- **WHEN** transform_context、transform_tool_call、decide_tool_call、after_tool_call 或 prepare_next_turn 分别发生正常返回、Handler 失败、timeout 或取消
- **THEN** 每个 Hook point 都遵守相同的成功、fail-closed、deadline 和 cancellation 合同

### Requirement: 模型结束原因与 Hook 停止原因分离
`LoopOutcome` 必须（SHALL）使用类型化的 Loop completion reason 区分 provider `FinishReason` 与 Hook Stop，durable LoopFinished 必须（SHALL）使用稳定且不歧义的字符串投影。

#### Scenario: 模型自然结束
- **WHEN** provider 在没有可执行 Tool call 的情况下以某个 FinishReason 结束
- **THEN** LoopOutcome completion 是携带该 FinishReason 的 Model 变体

#### Scenario: Hook 结束循环
- **WHEN** prepare_next_turn 返回 Stop
- **THEN** LoopOutcome completion 是 HookStopped，durable LoopFinished 的原因是 `hook_stopped`

### Requirement: H1 Hook 决策不借用观测或历史作为存储
Hook decision 必须（SHALL）只通过 `HookRuntime` 返回值影响当前 AgentLoop。系统不得（SHALL NOT）把 Hook decision 写入 AgentRuntime conversation message、history product event 或 NATS telemetry；decision 的持久化必须（SHALL）只通过 `DurableAgentEvent` 的 hook invocation 变体作为执行状态承载，且 resume 不得（SHALL NOT）根据 NATS 观测重建 invocation 状态。

用户提交的 `ApprovalDecision` 是审批 Handler 的外部耐久输入，不是 Hook decision：它必须（SHALL）以 `ToolApprovalResolved` 写入exact AgentRuntime durable ledger。Handler 读取它后返回 Execute 或 Block，随后只有matching `HookInvocationCompleted`才持久化真正的Hook decision。外围sink与resolver必须（SHALL）以AgentRuntimeId限定ledger并验证outer Session/Turn identity；系统必须（SHALL）从该Postgres ledger恢复Handler输入，不得（SHALL NOT）从NATS或审批专用状态表恢复。

#### Scenario: Hook 改变当前执行
- **WHEN** Hook 修改 context、Tool 参数、Tool result 或下一轮控制
- **THEN** AgentLoop 只应用类型化 decision；现有 AgentRuntime message、product event 和 telemetry 仍遵守各自合同，不新增含 Hook decision payload 的旁路记录

#### Scenario: H1 进程重启
- **WHEN** 进程在 HookInvocationCompleted 提交后停止并恢复
- **THEN** resume 在exact AgentRuntime ledger中复用journal里地址与digest匹配的Completed decision，不重新调用Runtime；Hook decision仍不出现在AgentRuntime history或NATS中

#### Scenario: ApprovalDecision 先于 HookDecision
- **WHEN** HTTP resolver 提交批准或拒绝
- **THEN** ToolApprovalResolved 先作为外部输入持久化；Handler 映射后由 AgentLoop 另行提交 HookInvocationCompleted，二者不得合并为同一语义或写成第二份状态

#### Scenario: NATS 不作为审批真相
- **WHEN** ToolApprovalResolved 的 NATS publish 丢失但 Postgres 事务已提交
- **THEN** hosted 或 resumed Handler 仍从exact AgentRuntime durable ledger读取决定，执行结果不受观测丢失影响

#### Scenario: Kernel 不感知外部输入来源
- **WHEN** 审批决定来自 HTTP API 并存入 Postgres
- **THEN** AgentLoop 仍只接收HookRuntime返回的Execute或Block，不解析HTTP请求、不执行SQL、不管理ApprovalId waiter，也不接收AgentRuntimeId

### Requirement: Transform Tool Call 只变换工具参数
对于 provider 以 `tool_calls` finish reason 授权的每个 Tool call，AgentLoop 必须（SHALL）在原始参数校验通过后、最终参数复验之前调用 `transform_tool_call`。Decision 必须（SHALL）是 Continue，或携带可选新 arguments 与可选授权覆写的 Modify；它不得（SHALL NOT）改变 `CallId` 或 Tool name，也不得（SHALL NOT）阻断调用。所有字段均无变化的 Modify 必须（SHALL）判为 `HookFailure::InvalidOutput`。

#### Scenario: 继续原 Tool Call
- **WHEN** transform-tool decision 是 Continue
- **THEN** AgentLoop 以原 arguments 进入最终参数复验，生效授权保持注册表默认

#### Scenario: 修改 Tool 参数
- **WHEN** transform-tool decision 提供新的 arguments
- **THEN** AgentLoop 保留相同 `CallId` 和 Tool name，以修改后的 arguments 进入最终参数复验

#### Scenario: 复验拦截非法变换结果
- **WHEN** transform-tool decision 修改后的 arguments 未通过最终参数复验
- **THEN** AgentLoop 生成校验错误结果，不进入 decide_tool_call、不提交 ToolExecutionStarted、也不调用 Tool

#### Scenario: 非 ToolCalls finish reason
- **WHEN** provider 响应包含 Tool call 但 finish reason 不是 `tool_calls`
- **THEN** AgentLoop 保持现有不可执行结果行为，并且不调用 transform_tool_call、decide_tool_call 或 after_tool_call

### Requirement: Decide Tool Call 决定工具是否执行
对于每个通过最终参数复验的 Tool call，AgentLoop 必须（SHALL）在 `ToolExecutionStarted` 提交之前调用 `decide_tool_call`。Decision 必须（SHALL）只能执行或阻断；decide 相位不得（SHALL NOT）修改 arguments、不得（SHALL NOT）改变 `CallId` 或 Tool name，保证决策方看到的参数与实际执行的参数一致。

#### Scenario: 执行 Tool Call
- **WHEN** decide-tool decision 是 Execute
- **THEN** AgentLoop 提交 ToolExecutionStarted 并以复验后的参数调用 Tool

#### Scenario: 阻断 Tool Call
- **WHEN** decide-tool decision 是带有非空安全 reason 的 Block
- **THEN** AgentLoop 不提交 ToolExecutionStarted、也不调用 Tool，并生成 code 为 `hook_blocked` 的结构化模型可见 Tool result，该结果仍经过 after_tool_call

#### Scenario: Decide 输入即最终参数
- **WHEN** transform_tool_call 修改了 arguments 且复验通过
- **THEN** decide_tool_call 接收的 Tool call 携带复验后的最终 arguments

### Requirement: 工具审批是 Decide 相位的 Hook Handler
工具审批必须（SHALL）以实现 `decide_tool_call` 的普通 Hook Handler 形式存在：批准映射为 Execute，拒绝映射为 Block。`ToolExecutor` 不得（SHALL NOT）持有审批策略、发起审批交互或提交 `ToolApprovalRequested` / `ToolApprovalResolved`；AgentLoop 只调用 `HookRuntime` 并应用类型化 decision，不得（SHALL NOT）感知 HTTP、Postgres、waiter、审批生命周期或`AgentRuntimeId`。API外层组合必须（SHALL）把审批Handler的私有ledger协作者绑定到exact AgentRuntimeId及`agent_states.agent_id`固定的immutable AgentId，而不把runtime aggregate identity注入AgentLoop。

内核必须（SHALL）保留 `DurableAgentEvent` 的 `ToolApprovalRequested`、`ToolApprovalResolved` 变体以及既有 Hook journal 变体，但审批 Handler 的私有协作负责在外层绑定的exact AgentRuntime ledger中以exact `HookInvocationId`追加和读取审批事件。对于需要人工决定的调用，事件顺序必须（SHALL）是 `HookInvocationPending`、`ToolApprovalRequested`、`ToolApprovalResolved`、`HookInvocationCompleted`，随后才是 `ToolExecutionStarted` 或 Block 结果；恢复不得（SHALL NOT）跨越或重排这些耐久边界。审批 Handler 必须（SHALL）从该AgentRuntime durable ledger复用既有请求或决定，不得（SHALL NOT）依赖审批专用状态表。

现有Hook journal durable variant必须（SHALL）保持kernel-minimal，只保存invocation identity、Hook point、iteration、CallId、digest与decision/failure；不得（SHALL NOT）为了外围分区而新增`AgentRuntimeId`、Session、Turn、Agent definition字段或持久化整个进程内address。外层sink、resolver与strict replay必须（SHALL）以AgentRuntimeId限定ledger，并在Handler、Tool或provider动作前验证row属于current exact Session/Turn；immutable definition只由current `LoopStarted.runtime_snapshot.agent_id`与`agent_states.agent_id` pin校验。

#### Scenario: 审批 Handler 批准
- **WHEN** 审批 Handler 读取到 exact ApprovalId 的 durable approve 决定
- **THEN** decide_tool_call 返回 Execute，AgentLoop 先提交 matching HookInvocationCompleted，再按普通 Execute 路径提交 ToolExecutionStarted 并执行 Tool

#### Scenario: 审批 Handler 拒绝
- **WHEN** 审批 Handler 读取到 exact ApprovalId 的 durable reject 决定
- **THEN** decide_tool_call 返回带安全 reason 的 Block，AgentLoop 先提交 matching HookInvocationCompleted，再生成经过 after_tool_call 的 hook_blocked Tool result且不执行 Tool，并继续后续模型迭代

#### Scenario: 审批请求使用最终参数
- **WHEN** transform_tool_call 修改 arguments 或授权元数据且最终复验通过
- **THEN** 当前shell/apply_patch composition的ToolApprovalRequested保存decide Handler所见的最终CallId、Tool name、opaque arguments与typed ToolKind/DangerLevel；不存在credential reference/provider通道，未来credential-aware Tool必须先完成独立安全PATCH才能注册

#### Scenario: Pending 后才允许请求审批
- **WHEN** 一个 decide_tool_call invocation 需要人工审批
- **THEN** AgentLoop 先通过唯一 DurableEventSink 提交该 invocation 的 HookInvocationPending，外围sink将它写入exact AgentRuntime ledger，审批 Handler 随后才可追加绑定exact HookInvocationId的ToolApprovalRequested

#### Scenario: 共享 Definition 的 Runtime 审批隔离
- **WHEN** 两个AgentRuntime共享同一immutable AgentId且各自存在地址相似的Pending invocation
- **THEN** Handler与resolver只在请求携带的exact AgentRuntime ledger中复用Requested或Resolved，任一runtime的决定不得满足另一个runtime的审批

#### Scenario: Hook Journal 外层身份不一致
- **WHEN** Hook journal row不属于exact AgentRuntime、current Session或current Turn，或current snapshot的AgentId不等于state pin
- **THEN** 外层strict append/replay返回`durable_state_corrupt`，且不调用审批Handler、Tool或provider

#### Scenario: 审批交互取消
- **WHEN** 审批 Handler 等待 durable 决定期间 Turn CancellationToken 被取消
- **THEN** Handler 停止等待且不返回 decision，AgentLoop 不提交 HookInvocationCompleted 或 ToolExecutionStarted，并由 Turn terminal event 使未 Consumed 审批失效

#### Scenario: Requested 后崩溃恢复不重复提示
- **WHEN** ToolApprovalRequested 已提交到exact AgentRuntime ledger但决定尚未 Resolved 时进程停止并恢复
- **THEN** AgentLoop 以原 invocation 身份重试 Pending，审批 Handler 复用相同 ApprovalId 并继续等待，不追加第二个 Requested，也不重新创建审批提示

#### Scenario: Resolved 后崩溃恢复不重复询问
- **WHEN** ToolApprovalResolved 已提交到exact AgentRuntime ledger但 HookInvocationCompleted 尚未提交时进程停止并恢复
- **THEN** AgentLoop 以原 invocation 身份重试 Pending，审批 Handler 从 ledger 直接映射既有决定而不重新问人，再由 AgentLoop 提交 matching Completed

#### Scenario: Completed 后恢复跳过审批 Handler
- **WHEN** 审批决定与 matching HookInvocationCompleted 都已在崩溃前提交
- **THEN** resume 复用 journal 中地址与 digest 匹配的 Completed decision，不调用审批 Handler，也不读取 NATS 重建决定

#### Scenario: 未托管期间 Resolve 后显式恢复
- **WHEN** AgentRuntime/Turn未被当前进程托管时审批决定已持久化，随后调用方显式resume exact AgentRuntime/Turn
- **THEN** Pending invocation 的审批 Handler 读取既有 Resolved 并返回决定；resolve 本身不托管或恢复 Turn

#### Scenario: Terminal 后不得应用审批决定
- **WHEN** Turn 在 matching HookInvocationCompleted 之前进入 Finished、Failed 或 Cancelled 终态
- **THEN** 审批派生为 Invalidated，恢复不调用 Tool，审批 Handler 不得把此前或随后到达的决定返回给 AgentLoop

#### Scenario: Kernel 不依赖审批基础设施
- **WHEN** 审批由 HTTP resolver 与 Postgres durable ledger 驱动
- **THEN** stratum-agent 与 AgentLoop 的依赖仍只包含类型化 HookRuntime、DurableEventSink 和 durable event variants，不包含AgentRuntimeId、HTTP handler、SQL query或进程托管逻辑

### Requirement: Tool Hook 输入携带工具目标元数据
`transform_tool_call`、`decide_tool_call` 与 `after_tool_call` 的输入必须（SHALL）在公共快照与 Tool call 之外携带工具目标视图，包含工具授权元数据（`ToolKind` 与 `DangerLevel`）和 `ToolSpec`。元数据查询必须（SHALL）由 AgentLoop 一侧在 Hook 调用前完成，Handler 不得（SHALL NOT）自行查询工具注册表。授权元数据必须（SHALL）是生效值：`transform_tool_call` 看到注册表默认声明，`decide_tool_call` 与 `after_tool_call` 看到 transform 覆写后（若有）的值。

#### Scenario: Hook 接收授权元数据
- **WHEN** 任一 Tool Hook 被调用
- **THEN** 输入包含该 Tool 的 ToolKind、DangerLevel 与 ToolSpec；transform 看到的是注册表默认声明，decide 与 after 看到的是生效授权

#### Scenario: 缺失工具不进入 Tool Hook
- **WHEN** Tool call 引用的 Tool 在注册表中不存在
- **THEN** AgentLoop 生成现有的工具缺失错误结果，不调用 transform_tool_call、decide_tool_call 或 after_tool_call

### Requirement: Hook 输入共享公共快照
全部五个 Hook 的输入必须（SHALL）嵌入同一个借用公共快照 `HookSnapshot`，携带 `iteration`、该边界时刻 committed `LoopContext` 的借用视图和 `Option<TokenUsage>`。快照必须（SHALL）是只读的；新增公共输入字段必须（SHALL）只改 `HookSnapshot` 一处即可被全部 Hook 点继承。快照的 usage 必须（SHALL）是最近一次模型响应上报的 usage（尚无上报时为 `None`），kernel 不得（SHALL NOT）跨调用累计；需要累计语义的 Handler 自行维护。

#### Scenario: 每个 Hook 接收公共快照
- **WHEN** 任一 Hook 被调用
- **THEN** 输入包含携带 iteration、context 和 usage 的同一形状快照，且各点的专属载荷（Tool call、工具目标、result）不在快照中

#### Scenario: 快照 Context 为边界时刻的 Committed Context
- **WHEN** transform_tool_call、decide_tool_call 或 prepare_next_turn 被调用
- **THEN** 快照 context 是该边界时刻的 committed context，含当前 assistant 消息与本 cycle 已提交的 tool result

#### Scenario: After Tool Call 快照不含未提交结果
- **WHEN** after_tool_call 被调用
- **THEN** 快照 context 不含当前未提交的 result，该 result 只出现在点的专属载荷中

#### Scenario: Usage 累计与缺省
- **WHEN** provider 在部分或全部模型响应中上报了 token usage（语义修正：usage 不再是累计值）
- **THEN** 快照 usage 是最近一次模型响应的上报值；provider 从未上报时 usage 为 None；kernel 不做跨调用累计

#### Scenario: 公共字段单点扩展
- **WHEN** 需要为全部 Hook 增加公共输入信息
- **THEN** 只需在 `HookSnapshot` 增加字段，五个 Hook 输入结构无需逐一改动

### Requirement: Transform 相位可以覆写工具授权
注册表的授权声明必须（SHALL）只是默认依据而非终判。`transform_tool_call` 的 Modify decision 必须（SHALL）允许通过 `AuthorizationOverride` 覆写本次调用的生效授权：`Set` 替换授权元数据，`PreAuthorize` 将调用标记为预授权。AgentLoop 必须（SHALL）把生效授权搬运到 `decide_tool_call` 与 `after_tool_call`，且不得（SHALL NOT）基于该值做任何分支或合理性检查（含降级检查）；覆写是 Handler 的明示责任。

#### Scenario: 无覆写时生效值即注册表默认
- **WHEN** transform-tool decision 是 Continue 或不含授权覆写的 Modify
- **THEN** decide_tool_call 与 after_tool_call 接收的授权元数据与注册表声明一致

#### Scenario: Set 覆写到达 Decide 与 After
- **WHEN** transform-tool decision 携带 `Set` 授权覆写
- **THEN** decide_tool_call 与 after_tool_call 接收覆写后的 ToolKind 与 DangerLevel，AgentLoop 的正常执行路径不受该值影响

#### Scenario: PreAuthorize 抹除授权
- **WHEN** transform-tool decision 携带 `PreAuthorize` 覆写
- **THEN** decide_tool_call 与 after_tool_call 接收的授权元数据为 None

#### Scenario: 空 Modify 判为非法输出
- **WHEN** Modify 的 arguments 与 authorization 均无变化
- **THEN** AgentLoop 返回 `HookFailure::InvalidOutput`，不进入 decide_tool_call、不提交 ToolExecutionStarted、也不调用 Tool

### Requirement: Hook 调用写入 Journal 记录
AgentLoop 必须（SHALL）以 hook-point 粒度在唯一 `DurableEventSink` 中记录每次 Hook 调用：调用 Runtime 前提交 `HookInvocationPending`（含 invocation id、`(iteration, HookPoint, Option<CallId>)` 地址与 input digest）；应用 decision 影响的动作之前提交 `HookInvocationCompleted`；类型化失败时提交 `HookInvocationFailed`。系统不得（SHALL NOT）为 journal 引入第二个耐久 sink。

#### Scenario: 调用前提交 Pending
- **WHEN** 任一 Hook 即将调用 Runtime
- **THEN** AgentLoop 先耐久提交该调用的 Pending 记录，且同一逻辑调用重试时不创建第二个 invocation 身份

#### Scenario: 应用前提交 Completed
- **WHEN** Hook 返回合法 decision
- **THEN** AgentLoop 在执行受影响的模型请求、ToolExecutionStarted、Tool result 提交或迭代边界之前耐久提交 Completed 记录

#### Scenario: 失败提交 Failed
- **WHEN** Hook 返回类型化失败、超时或无效 decision
- **THEN** AgentLoop 耐久提交 Failed 记录并以既有 fail-closed 路径结束

### Requirement: Resume 复用 Journal 中的 Hook 决定
恢复执行时，AgentLoop 必须（SHALL）在每个 Hook 点先查 journal：地址与 digest 匹配的 Completed 必须（SHALL）复用其 decision 而不调用 Runtime；Pending 必须（SHALL）以原 invocation 身份重试；Failed 必须（SHALL）重现类型化失败；地址或 digest 不匹配必须（SHALL）fail closed。input digest 必须（SHALL）是载荷级：Tool Hook 对 canonical ToolCall 做 sha256，无专属载荷的 Hook 点以地址本身为 digest；usage 与对话历史不得（SHALL NOT）参与 digest。

#### Scenario: 复用 Completed 决定
- **WHEN** 恢复后某 Hook 点存在地址与 digest 都匹配的 Completed 记录
- **THEN** AgentLoop 直接应用记录的 decision，不调用 Runtime

#### Scenario: Pending 以原身份重试
- **WHEN** 恢复后某 Hook 点只有 Pending 记录
- **THEN** AgentLoop 以原 invocation 身份重新调用 Runtime，不创建第二个逻辑 invocation

#### Scenario: 重现 Failed
- **WHEN** 恢复后某 Hook 点存在 Failed 记录
- **THEN** AgentLoop 重现该类型化失败，不调用 Runtime

#### Scenario: Digest 不匹配 fail closed
- **WHEN** 恢复后某 Hook 点地址匹配但 input digest 不匹配
- **THEN** AgentLoop 以类型化错误 fail closed，不调用 Runtime、不应用任何 decision

#### Scenario: 审批决定不再重复询问
- **WHEN** 审批 Handler 的 Execute 决定已在崩溃前提交 Completed
- **THEN** 恢复后 AgentLoop 直接按 Execute 执行，审批 Handler 不再被调用

### Requirement: Hook Handler 链按序执行
系统必须（SHALL）提供 `HookHandler`（与五个 Hook 点同形、默认 No-op、携带不可变 `HookHandlerVersionId`）与实现 `HookRuntime` 的有序 `ChainHookRuntime`。链语义按点定义：`transform_context`、`transform_tool_call`、`after_tool_call` 必须（SHALL）顺序变换——前一 Handler 的输出视图是后一 Handler 的输入；`decide_tool_call` 必须（SHALL）在第一个 `Block` 处短路；`prepare_next_turn` 必须（SHALL）在 `Stop` 处短路并按 Handler 顺序合并多个 `Inject` 的消息。任一 Handler 失败或返回非法 decision 必须（SHALL）使整个 Hook 点 fail closed。

#### Scenario: 顺序变换线程化
- **WHEN** transform 链中第一个 Handler 修改了 Tool 参数
- **THEN** 第二个 Handler 看到的是修改后的参数，链结束后 kernel 对最终参数复验

#### Scenario: Block 短路
- **WHEN** decide 链中第二个 Handler 返回 Block
- **THEN** 后续 Handler 不被调用，该调用按 Block 处理

#### Scenario: Stop 短路丢弃已收集 Inject
- **WHEN** prepare 链中前一 Handler 返回 Inject、后一 Handler 返回 Stop
- **THEN** 决策为 Stop，已收集的 Inject 消息被丢弃

#### Scenario: Inject 有序合并
- **WHEN** prepare 链中两个 Handler 分别返回 Inject
- **THEN** 决策为单个 Inject，消息按 Handler 顺序拼接

#### Scenario: Handler 失败 fail closed
- **WHEN** 链中任一 Handler 返回类型化失败或非法 decision
- **THEN** 整个 Hook 点以该失败结束，后续 Handler 不被调用

### Requirement: 链版本固定并可恢复校验
`ChainHookRuntime` 必须（SHALL）在构造时按声明顺序固定 Handler 序列，并从有序 Handler 版本计算 `ExtensionSetVersionId`。AgentLoop 必须（SHALL）把该版本随 `LoopStarted` 耐久提交；resume 时事件流中的版本与当前注入 runtime 报告的版本不一致必须（SHALL）fail closed。未提供版本的 runtime 必须（SHALL）视为无固定链，跳过校验。

#### Scenario: 重启后顺序一致
- **WHEN** 组合方以相同顺序构造同名 Handler 链并 resume
- **THEN** 计算的链版本与事件流中的版本一致，恢复继续

#### Scenario: 链版本不匹配 fail closed
- **WHEN** resume 时注入链的版本与事件流记录的版本不同（顺序、成员或 Handler 版本变化）
- **THEN** resume 以类型化错误拒绝，不开始任何模型、Tool 或 Hook 动作
