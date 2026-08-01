# agent-hook-runtime Delta（implement-hook-journal-resume）

## MODIFIED Requirements

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

### Requirement: H1 Hook 决策不借用观测或历史作为存储
Hook decision 必须（SHALL）只通过 `HookRuntime` 返回值影响当前 AgentLoop。系统不得（SHALL NOT）把 Hook decision 写入 Agent conversation message 或 EventBus 观测流；decision 的持久化必须（SHALL）只通过 `DurableAgentEvent` 的 hook invocation 变体作为执行状态承载，且 resume 不得（SHALL NOT）根据 EventBus 观测重建 invocation 状态。

#### Scenario: Hook 改变当前执行
- **WHEN** Hook 修改 context、Tool 参数、Tool result 或下一轮控制
- **THEN** AgentLoop 只应用类型化 decision；现有 Agent message 和 telemetry 仍遵守各自合同，不新增含 decision payload 的旁路记录

#### Scenario: H1 进程重启
- **WHEN** 进程在 Hook decision 持久化提交后停止并恢复（语义扩展：decision 现在可恢复）
- **THEN** resume 复用 journal 中 digest 匹配的 Completed decision，不重新调用 Runtime；Hook decision 仍不出现在 Agent history 或 EventBus 中

## ADDED Requirements

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
