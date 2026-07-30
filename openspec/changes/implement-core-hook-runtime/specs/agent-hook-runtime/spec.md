## ADDED Requirements

### Requirement: AgentLoop 接受单一 Hook Runtime
`AgentLoopBuilder` 必须（SHALL）允许调用方注入一个实现四个核心 Hook 的 `HookRuntime`。未注入自定义 Runtime 时必须（SHALL）使用 No-op Runtime，并保持没有 Hook 时已有的模型请求、耐久事件、Tool 调用、消息和终态行为。

#### Scenario: 默认 No-op Runtime
- **WHEN** 调用方没有为 AgentLoop 注入 Hook Runtime
- **THEN** 相同的模型响应和 Tool outcome 产生与变更前相同的请求、耐久事件、Tool 调用、消息和循环结果

#### Scenario: 注入自定义 Runtime
- **WHEN** 调用方通过 AgentLoopBuilder 注入自定义 Hook Runtime
- **THEN** AgentLoop 在对应控制流边界调用该 Runtime，并且不要求 AgentLoop 了解 Handler 列表、Session、journal 或 EventBus

### Requirement: Transform Context 只变换当前模型请求
AgentLoop 必须（SHALL）在每次模型请求开始前调用 `transform_context`。Runtime 必须（SHALL）接收当前迭代和 committed LoopContext 的借用视图，并且可以保持原 context 或为当前模型请求提供替代 context。替代 context 不得（SHALL NOT）回写 committed transcript、产生 durable message 或出现在 `LoopOutcome.new_messages` 中。

#### Scenario: 保持原 Context
- **WHEN** transform-context decision 是 Unchanged
- **THEN** AgentLoop 使用 committed system prompt、history 和本次一次性 Inject message 构造模型请求

#### Scenario: 替换当前请求 Context
- **WHEN** transform-context decision 提供替代 LoopContext
- **THEN** 当前模型请求使用替代 context，而下一次迭代仍从未被改写的 committed context 构造新 request view

#### Scenario: Transform Context 失败
- **WHEN** transform_context 返回类型化失败或无效 decision
- **THEN** AgentLoop 在发起对应模型请求前 fail closed，且不把替代内容提交为 Agent 消息

### Requirement: Before Tool Call 决定 Tool 是否以及如何执行
对于 provider 以 `tool_calls` finish reason 授权的每个 Tool call，AgentLoop 必须（SHALL）在审批或 Tool 执行之前调用 `before_tool_call`。Decision 必须（SHALL）只能继续原调用、替换 arguments 或阻断调用；它不得（SHALL NOT）改变 `CallId` 或 Tool name。

#### Scenario: 继续原 Tool Call
- **WHEN** before-tool decision 是 Continue
- **THEN** AgentLoop 将原 Tool call 交给现有 ToolExecutor

#### Scenario: 修改 Tool 参数
- **WHEN** before-tool decision 提供新的 arguments
- **THEN** ToolExecutor 接收相同 `CallId` 和 Tool name 以及修改后的 arguments，并执行其现有校验、审批和调用流程

#### Scenario: 阻断 Tool Call
- **WHEN** before-tool decision 是带有非空安全 reason 的 Block
- **THEN** AgentLoop 不进入 Tool 审批、不提交 ToolExecutionStarted、也不调用 Tool，并生成 code 为 `hook_blocked` 的结构化模型可见 Tool result

#### Scenario: 非 ToolCalls finish reason
- **WHEN** provider 响应包含 Tool call 但 finish reason 不是 `tool_calls`
- **THEN** AgentLoop 保持现有不可执行结果行为，并且不调用 before_tool_call 或 after_tool_call

### Requirement: After Tool Call 可以替换模型可见结果
对于每个被 provider 授权的 Tool cycle，AgentLoop 必须（SHALL）在产生模型可见 Tool result 后、提交该消息前调用 `after_tool_call`。Runtime 可以保留结果或替换 JSON result；AgentLoop 必须（SHALL）保留原 `CallId` 和 Tool message role。

#### Scenario: 保留 Tool Result
- **WHEN** after-tool decision 是 Keep
- **THEN** AgentLoop 原样提交 ToolExecutor 或 before-tool Block 产生的模型可见结果

#### Scenario: 替换 Tool Result
- **WHEN** after-tool decision 提供替代 JSON result
- **THEN** AgentLoop 使用原 `CallId` 构造并耐久提交替代 Tool message，原结果不会进入模型 context

#### Scenario: Tool 完成后 After Hook 失败
- **WHEN** ToolExecutor 已返回结果但 after_tool_call 返回失败、超时或无效 decision
- **THEN** AgentLoop 不提交伪造或未变换的 Tool result，并以类型化 Hook 错误 fail closed

### Requirement: Prepare Next Turn 控制下一次模型迭代
当一个被授权的 Tool cycle 的全部模型可见结果已耐久提交后，AgentLoop 必须（SHALL）调用 `prepare_next_turn`。Decision 必须（SHALL）是 Continue、Stop 或 Inject 非空 User message 列表。

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

### Requirement: 所有 Hook 共享取消、Deadline 与错误语义
AgentLoop 必须（SHALL）为四个 Hook 都提供当前 CancellationToken 和绝对 deadline，并且必须（SHALL）在核心循环边界强制取消与 timeout，而不是依赖具体 Runtime 自行遵守。Decision-affecting Hook 失败必须（SHALL）阻止受影响的模型、Tool、message 或 iteration action 继续。

#### Scenario: 调用前已经取消
- **WHEN** 任一 Hook 即将调用时 Turn CancellationToken 已取消
- **THEN** AgentLoop 不调用 Runtime，并进入既有 loop cancellation 终态

#### Scenario: Hook 执行中取消
- **WHEN** 任一 Hook 尚未返回时 Turn CancellationToken 被取消
- **THEN** AgentLoop 停止等待该 Hook，不执行其后受影响的动作，并进入 loop cancellation 终态

#### Scenario: Hook 超过 Deadline
- **WHEN** 任一 Hook 在其绝对 deadline 前没有返回
- **THEN** AgentLoop 以包含对应 HookPoint 和 `HookFailure::TimedOut` 的类型化错误 fail closed

#### Scenario: Runtime 返回失败
- **WHEN** 任一 Hook Runtime 返回安全的类型化 HookFailure
- **THEN** AgentLoop 保留 HookPoint 与失败分类，且公开错误、trace 和 durable terminal event 不包含 prompt、Tool 参数、Tool result 或 Runtime 内部错误正文

#### Scenario: 四个 Hook 的失败矩阵
- **WHEN** transform_context、before_tool_call、after_tool_call 或 prepare_next_turn 分别发生正常返回、Handler 失败、timeout 或取消
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
H1 Hook decision 必须（SHALL）只通过 `HookRuntime` 返回值影响当前 AgentLoop。系统不得（SHALL NOT）把 Hook decision 写入 Agent conversation message、runtime metadata 或 EventBus，也不得（SHALL NOT）把 DurableAgentEvent 当作 Hook journal。

#### Scenario: Hook 改变当前执行
- **WHEN** Hook 修改 context、Tool 参数、Tool result 或下一轮控制
- **THEN** AgentLoop 只应用类型化 decision；现有 Agent message 和 telemetry 仍遵守各自合同，不新增含 decision payload 的旁路记录

#### Scenario: H1 进程重启
- **WHEN** 进程在未持久化的 H1 Hook decision 后停止
- **THEN** H1 不声称能够从 Agent history 或 EventBus 恢复该 decision，后续持久化与复用由 H3 journal 提供
