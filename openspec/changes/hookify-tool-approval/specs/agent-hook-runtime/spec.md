# agent-hook-runtime Delta（hookify-tool-approval）

## REMOVED Requirements

### Requirement: Before Tool Call 决定 Tool 是否以及如何执行
**Reason**: `before_tool_call` 混合了参数变换与执行决策两种语义，且审批作为独立 `ToolApproval` 边界寄生在 `ToolExecutor` 内，无法满足"审批所见即所执行"。该 Hook 点按语义拆分为 `transform_tool_call` 与 `decide_tool_call` 两个点，审批收敛为 decide 相位的普通 Handler。
**Migration**: 实现方将 `before_tool_call` 的 Continue / ModifyArguments 迁移到 `transform_tool_call`，将 Block 与审批逻辑迁移到 `decide_tool_call`；`ToolApproval` 注入点删除，由 decide 相位 Handler 私有实现问人交互。

## MODIFIED Requirements

### Requirement: AgentLoop 接受单一 Hook Runtime
`AgentLoopBuilder` 必须（SHALL）允许调用方注入一个实现核心 Hook 合同（`transform_context`、`transform_tool_call`、`decide_tool_call`、`after_tool_call`、`prepare_next_turn`）的单一 `HookRuntime`。未注入自定义 Runtime 时必须（SHALL）使用 No-op Runtime，并保持没有 Hook 时已有的模型请求、耐久事件、Tool 调用、消息和终态行为。

#### Scenario: 默认 No-op Runtime
- **WHEN** 调用方没有为 AgentLoop 注入 Hook Runtime
- **THEN** 相同的模型响应和 Tool outcome 产生与变更前相同的请求、耐久事件、Tool 调用、消息和循环结果

#### Scenario: 注入自定义 Runtime
- **WHEN** 调用方通过 AgentLoopBuilder 注入自定义 Hook Runtime
- **THEN** AgentLoop 在对应控制流边界调用该 Runtime，并且不要求 AgentLoop 了解 Handler 列表、Session、journal 或 EventBus

### Requirement: After Tool Call 可以替换模型可见结果
对于每个被 provider 授权的 Tool cycle，AgentLoop 必须（SHALL）在产生模型可见 Tool result 后、提交该消息前调用 `after_tool_call`。Runtime 必须（SHALL）接收携带授权元数据与 `ToolSpec` 的工具目标视图，可以保留结果或替换 JSON result；AgentLoop 必须（SHALL）保留原 `CallId` 和 Tool message role。

#### Scenario: 保留 Tool Result
- **WHEN** after-tool decision 是 Keep
- **THEN** AgentLoop 原样提交 ToolExecutor 或 decide-tool Block 产生的模型可见结果

#### Scenario: 替换 Tool Result
- **WHEN** after-tool decision 提供替代 JSON result
- **THEN** AgentLoop 使用原 `CallId` 构造并耐久提交替代 Tool message，原结果不会进入模型 context

#### Scenario: Tool 完成后 After Hook 失败
- **WHEN** ToolExecutor 已返回结果但 after_tool_call 返回失败、超时或无效 decision
- **THEN** AgentLoop 不提交伪造或未变换的 Tool result，并以类型化 Hook 错误 fail closed

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

#### Scenario: 各 Hook 的失败矩阵
- **WHEN** transform_context、transform_tool_call、decide_tool_call、after_tool_call 或 prepare_next_turn 分别发生正常返回、Handler 失败、timeout 或取消
- **THEN** 每个 Hook point 都遵守相同的成功、fail-closed、deadline 和 cancellation 合同

## ADDED Requirements

### Requirement: Transform Tool Call 只变换工具参数
对于 provider 以 `tool_calls` finish reason 授权的每个 Tool call，AgentLoop 必须（SHALL）在原始参数校验通过后、最终参数复验之前调用 `transform_tool_call`。Decision 必须（SHALL）只能继续原调用或替换 arguments；它不得（SHALL NOT）改变 `CallId` 或 Tool name，也不得（SHALL NOT）阻断调用。

#### Scenario: 继续原 Tool Call
- **WHEN** transform-tool decision 是 Continue
- **THEN** AgentLoop 以原 arguments 进入最终参数复验

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
工具审批必须（SHALL）以实现 `decide_tool_call` 的普通 Hook Handler 形式存在：批准映射为 Execute，拒绝映射为 Block。`ToolExecutor` 不得（SHALL NOT）持有审批策略、发起审批交互或提交 `ToolApprovalRequested` / `ToolApprovalResolved` 耐久事件；审批的问人交互通道必须（SHALL）归审批 Handler 实现私有，AgentLoop 不感知审批概念。

#### Scenario: 审批 Handler 批准
- **WHEN** 审批 Handler 的问人交互结果为批准
- **THEN** decide_tool_call 返回 Execute，AgentLoop 按普通 Execute 路径执行 Tool

#### Scenario: 审批 Handler 拒绝
- **WHEN** 审批 Handler 的问人交互结果为拒绝
- **THEN** decide_tool_call 返回带 reason 的 Block，AgentLoop 生成 `hook_blocked` 模型可见结果且不执行 Tool

#### Scenario: 审批交互取消
- **WHEN** 审批 Handler 等待问人结果期间 Turn CancellationToken 被取消
- **THEN** AgentLoop 停止等待，不提交 ToolExecutionStarted，并进入 loop cancellation 终态

#### Scenario: 崩溃恢复重复提示
- **WHEN** 进程在审批批准后、ToolExecutionStarted 提交前停止并恢复
- **THEN** 恢复后的 decide_tool_call 重新执行，审批 Handler 再次问人；系统不得（SHALL NOT）在未重新确认的情况下执行 Tool，去重由后续 hook journal 提供

### Requirement: Tool Hook 输入携带工具目标元数据
`transform_tool_call`、`decide_tool_call` 与 `after_tool_call` 的输入必须（SHALL）在迭代位置与 Tool call 之外携带工具目标视图，包含工具授权元数据（`ToolKind` 与 `DangerLevel`）和 `ToolSpec`。元数据查询必须（SHALL）由 AgentLoop 一侧在 Hook 调用前完成，Handler 不得（SHALL NOT）自行查询工具注册表。

#### Scenario: Hook 接收授权元数据
- **WHEN** 任一 Tool Hook 被调用
- **THEN** 输入包含该 Tool 的 ToolKind、DangerLevel 与 ToolSpec，且与 ToolExecutor 实际使用的授权判定一致

#### Scenario: 缺失工具不进入 Tool Hook
- **WHEN** Tool call 引用的 Tool 在注册表中不存在
- **THEN** AgentLoop 生成现有的工具缺失错误结果，不调用 transform_tool_call、decide_tool_call 或 after_tool_call
