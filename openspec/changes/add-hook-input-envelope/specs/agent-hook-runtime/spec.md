# agent-hook-runtime Delta（add-hook-input-envelope）

## MODIFIED Requirements

### Requirement: Transform Context 只变换当前模型请求
AgentLoop 必须（SHALL）在每次模型请求开始前调用 `transform_context`。Runtime 必须（SHALL）经由公共快照接收当前迭代和 committed LoopContext 的借用视图（含本次待消费 Inject），并且可以保持原 context 或为当前模型请求提供替代 context。替代 context 不得（SHALL NOT）回写 committed transcript、产生 durable message 或出现在 `LoopOutcome.new_messages` 中。

#### Scenario: 保持原 Context
- **WHEN** transform-context decision 是 Unchanged
- **THEN** AgentLoop 使用 committed system prompt、history 和本次一次性 Inject message 构造模型请求

#### Scenario: 替换当前请求 Context
- **WHEN** transform-context decision 提供替代 LoopContext
- **THEN** 当前模型请求使用替代 context，而下一次迭代仍从未被改写的 committed context 构造新 request view

#### Scenario: Transform Context 失败
- **WHEN** transform_context 返回类型化失败或无效 decision
- **THEN** AgentLoop 在发起对应模型请求前 fail closed，且不把替代内容提交为 Agent 消息

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

### Requirement: Tool Hook 输入携带工具目标元数据
`transform_tool_call`、`decide_tool_call` 与 `after_tool_call` 的输入必须（SHALL）在公共快照与 Tool call 之外携带工具目标视图，包含工具授权元数据（`ToolKind` 与 `DangerLevel`）和 `ToolSpec`。元数据查询必须（SHALL）由 AgentLoop 一侧在 Hook 调用前完成，Handler 不得（SHALL NOT）自行查询工具注册表。

#### Scenario: Hook 接收授权元数据
- **WHEN** 任一 Tool Hook 被调用
- **THEN** 输入包含该 Tool 的 ToolKind、DangerLevel 与 ToolSpec，且与 ToolExecutor 实际使用的授权判定一致

#### Scenario: 缺失工具不进入 Tool Hook
- **WHEN** Tool call 引用的 Tool 在注册表中不存在
- **THEN** AgentLoop 生成现有的工具缺失错误结果，不调用 transform_tool_call、decide_tool_call 或 after_tool_call

## ADDED Requirements

### Requirement: Hook 输入共享公共快照
全部五个 Hook 的输入必须（SHALL）嵌入同一个借用公共快照 `HookSnapshot`，携带 `iteration`、该边界时刻 committed `LoopContext` 的借用视图和本次 run 累计的 `Option<TokenUsage>`。快照必须（SHALL）是只读的；新增公共输入字段必须（SHALL）只改 `HookSnapshot` 一处即可被全部 Hook 点继承。provider 未上报 usage 时快照的 usage 必须（SHALL）为 `None`。

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
- **WHEN** provider 在部分或全部模型响应中上报了 token usage
- **THEN** 快照 usage 是截至该 Hook 边界的累计值；provider 从未上报时 usage 为 None

#### Scenario: 公共字段单点扩展
- **WHEN** 需要为全部 Hook 增加公共输入信息
- **THEN** 只需在 `HookSnapshot` 增加字段，五个 Hook 输入结构无需逐一改动
