## ADDED Requirements

### Requirement: Session 是长期存在的 runtime 身份
系统必须（SHALL）使用 `SessionId` 作为 Agent 与 Workflow 活动的顶层关联身份。一个 Session 必须（SHALL）在多个 Agent Turn 之间保持不变，并且不得（SHALL NOT）根据 Workflow 图或 Workflow 版本派生其身份。

#### Scenario: 多个 Agent Turn 共享一个 Session
- **WHEN** 同一个 Agent 在已有 Session 中处理后续用户 Turn
- **THEN** 后续 Turn 使用已有的 `SessionId` 和新的 `TurnId`

#### Scenario: Session 内的 Workflow 版本发生变化
- **WHEN** Session 中的后续操作选择了不同的 Workflow 版本
- **THEN** Session 保持原有的 `SessionId`

### Requirement: Session 仅允许一个活跃操作
在当前版本中，系统必须（SHALL）允许一个 Session 内至多存在一个活跃的 Agent 或 Workflow 操作。发生启动冲突时必须失败，且不得替换或修改已有的活跃操作。

#### Scenario: 操作启动冲突
- **WHEN** 一个 Agent 或 Workflow 操作处于活跃状态，同时同一 Session 请求启动另一个操作
- **THEN** 新操作被拒绝，已有操作保持活跃

### Requirement: Host 提供 Agent runtime context
在启动 Turn 之前，Agent 组合边界必须（SHALL）提供 `SessionId` 和 `AgentLocation`。Agent 不得（SHALL NOT）创建或替换 Session 身份。

#### Scenario: Agent 直接运行 Turn
- **WHEN** host 在 Session 中直接启动 Agent
- **THEN** Agent 接收到 Session 身份和 `AgentLocation::Direct`

#### Scenario: Workflow Agent 节点运行 Turn
- **WHEN** Workflow 节点启动 Agent
- **THEN** Agent 接收到 Session 身份，以及包含不可变 Workflow 版本和 `NodeId` 的 Workflow-node location

### Requirement: Agent 持有 Turn 身份
Agent 必须（SHALL）为每个已接受的 Turn 创建新的 `TurnId`，并且必须在同一 Turn 的取消、进程重启与恢复过程中保持该身份不变。

#### Scenario: 恢复时保持 Turn 身份
- **WHEN** 持久化的 Agent Turn 在进程重启后恢复
- **THEN** 恢复的工作使用原有的 `SessionId` 和 `TurnId`

### Requirement: Agent 可在 Turn 之间修改模型配置
`ModelConfig` 必须（SHALL）是 Agent 当前可修改的模型配置，而不是 Session 或 Agent version 的永久固定属性。创建 Agent 或在终态 Agent 上启动新 Turn 时，调用方可以提供已校验的模型与 LLM 参数；已接受的 Turn 必须（SHALL）原子地将该配置保存为 Agent 当前配置并固定到该 Turn 的 runtime snapshot。未提供新配置时必须复用 Agent 当前配置。

#### Scenario: 后续 Turn 修改 LLM 参数
- **WHEN** Agent 已结束当前 Turn，调用方使用新的有效 `ModelConfig` 启动后续 Turn
- **THEN** 后续 Turn 使用并固定新配置，Agent 身份与 Session 身份保持不变，且新配置成为再后续 Turn 的默认配置

#### Scenario: 模型配置修改未被接受
- **WHEN** 新配置无效、模型不可用、Session 存在活跃操作或 Turn 启动失败
- **THEN** Agent 已持久化的当前模型配置保持不变

#### Scenario: 恢复固定的 Turn 配置
- **WHEN** 未完成 Turn 在进程重启后恢复
- **THEN** 恢复使用该 Turn 已固定的 `ModelConfig`，而不是任何更晚的配置请求

### Requirement: Agent 历史按 Agent 身份隔离
对话历史必须（SHALL）归 `AgentId` 所有。同一 Session 中的不同 Agent 不得（SHALL NOT）隐式共享对话历史，包括 Agent 作为 Workflow 节点运行的情况。

#### Scenario: Workflow Agent 节点使用独立历史
- **WHEN** Workflow Agent 节点以不同的 `AgentId` 启动 Agent
- **THEN** 该 Agent 不加载同一 Session 内另一个 Agent 的对话历史

#### Scenario: Session 状态不是对话历史
- **WHEN** 未来由 Hook 将 Session 状态或结果暴露给 Agent
- **THEN** 该 context 不改变对话历史归哪个 Agent 所有

### Requirement: 推迟 Node activation 与 attempt 身份
本基线必须（SHALL）使用 `SessionId`、不可变的 `WorkflowVersionId` 和 `NodeId` 标识活跃 Workflow 节点。在尚不支持循环、节点重入和重试时，本基线不得（SHALL NOT）要求 `NodeExecutionId` 或 `AttemptId`。

#### Scenario: 无环 Workflow 中的 Node 身份
- **WHEN** 某个节点在 Session 内唯一活跃的 Workflow 操作中仅运行一次
- **THEN** 其 Session、Workflow 版本和 Node 身份能够唯一定位其 runtime event
