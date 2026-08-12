## MODIFIED Requirements

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
- **THEN** 当前Echo composition的ToolApprovalRequested保存decide Handler所见的最终CallId、Tool name、user-authored opaque arguments与typed ToolKind/DangerLevel；不存在credential reference/provider通道，未来credential-aware Tool必须先完成独立安全PATCH才能注册

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
