# agent-hook-runtime Delta（make-authorization-hook-settable）

## MODIFIED Requirements

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

### Requirement: Tool Hook 输入携带工具目标元数据
`transform_tool_call`、`decide_tool_call` 与 `after_tool_call` 的输入必须（SHALL）在公共快照与 Tool call 之外携带工具目标视图，包含工具授权元数据（`ToolKind` 与 `DangerLevel`）和 `ToolSpec`。元数据查询必须（SHALL）由 AgentLoop 一侧在 Hook 调用前完成，Handler 不得（SHALL NOT）自行查询工具注册表。授权元数据必须（SHALL）是生效值：`transform_tool_call` 看到注册表默认声明，`decide_tool_call` 与 `after_tool_call` 看到 transform 覆写后（若有）的值。

#### Scenario: Hook 接收授权元数据
- **WHEN** 任一 Tool Hook 被调用
- **THEN** 输入包含该 Tool 的 ToolKind、DangerLevel 与 ToolSpec；transform 看到的是注册表默认声明，decide 与 after 看到的是生效授权

#### Scenario: 缺失工具不进入 Tool Hook
- **WHEN** Tool call 引用的 Tool 在注册表中不存在
- **THEN** AgentLoop 生成现有的工具缺失错误结果，不调用 transform_tool_call、decide_tool_call 或 after_tool_call

## ADDED Requirements

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
