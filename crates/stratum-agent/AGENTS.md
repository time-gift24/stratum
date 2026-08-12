# stratum-agent AGENTS.md

## 范围

`stratum-agent` 只包含与 Session 无关的 `AgentLoop` 内核。旧有的有状态
`Agent` 兼容路径已删除。`AgentId` 可以固定运行时快照和钩子地址中已有的不可变定义；
`AgentRuntimeId`、Postgres、HTTP、Session 托管、调度器、事件序号和分页绝不得进入
此 crate。

## AgentLoop 内核

- `AgentLoop` 接收调用方预加载的 `LoopContext` 和用户提示。它不负责创建 Session、
  加载历史记录、持久化存储或任何实时传输。
- 必需的状态转换使用 `DurableEventSink`；模型增量和非关键诊断使用单独的、
  尽力而为的 `TelemetryEventSink`。
- 内核必须先收到持久化追加的确认，才能改变其内存中的对话记录或启动下一个外部操作。
- 工具调用通过 `ToolExecutor` 依次执行。查找以及同步、无副作用、确定性的输入校验在
  工具钩子之前进行；经钩子转换的参数必须在 `decide_tool_call` 之前重新校验。
  `ToolExecutionStarted` 必须在分发之前持久化，每个工具结果也必须在下一个工具调用或
  模型请求之前持久化。
- 校验后以及紧邻 `ToolExecutionStarted` 之前都要重新检查取消状态；在启动事件得到
  确认之前发生的取消会阻止分发，并映射为循环取消。
- 本次运行提供的 `CancellationToken` 控制模型流的获取以及 `AgentLoop` 中的轮询；
  同一个取消令牌也会传给钩子和工具操作。取消采用协作式机制：
  `ToolExecutionStarted` 之后，调用方必须继续轮询循环，使其能够等待并记录结果。
  实际运行期间，内核绝不会猜测，也不会主动重试一条尚无结果的持久化启动记录。
  显式恢复时，缺失持久化结果的后缀会按至少一次（at-least-once）契约重新执行；
  幂等性由工具或外部服务负责，副作用防护由组合层负责。
- `ToolExecutor` 只负责纯执行机制：查找、校验、持久化
  `ToolExecutionStarted` 和分发。`execute` 是 `pub(crate)`，接收已解析的工具句柄以及
  经 `decide_tool_call` 批准的最终调用；其函数体只包含取消检查、持久化启动记录和分发。
  它没有授权概念，不持有审批策略，也不发出审批事件；执行决策属于
  `decide_tool_call` 钩子。
- 只有提供方返回的 `FinishReason::ToolCalls` 才能授权分发。如果响应包含工具调用，
  但结束原因是 `length`、`stop` 或其他原因，则提交结构化工具错误消息，不得调用工具。
- 除迭代次数和工具调用次数外，`LoopLimits` 还限制助手文本、推理内容以及每个工具调用
  参数缓冲区。每个流式片段追加前都要执行限制；绝不得保留无界的提供方响应。
- `ToolExecutor` 是 `AgentLoop` 所用持久化事件接收端的唯一来源；构建器不得接受第二个
  接收端，以免工具边界和循环边界被拆分到不同传输上。

## 钩子运行时

- `hook_runtime` 只包含单一的 `HookRuntime` 异步 `trait`（`runtime.rs`）及其默认实现
  `NoopHookRuntime`（`noop.rs`）；失败类型来自 `stratum-core::HookFailure`，循环侧的
  映射位于 `agent_loop/error.rs`（`AgentLoopError::Hook`）。不得向构建器添加针对各个
  钩子的闭包：该运行时是唯一的组合边界。
- 该 `trait` 只暴露五个钩子——`transform_context`、`transform_tool_call`、
  `decide_tool_call`、`after_tool_call`、`prepare_next_turn`——它们接收借用的输入并
  返回拥有所有权的决策。`AgentLoop` 持有一个通过
  `AgentLoopBuilder::hook_runtime` 注入的 `Arc<dyn HookRuntime>`；未注入时，空操作
  运行时保持钩子介入前的消息流不变（每次调用仍会追加日志记录）。
- 每个钩子输入都嵌入同一个借用的 `HookSnapshot`（`iteration`、`&LoopContext`、
  `Option<TokenUsage>`）。这是宽读窄写原则：处理器可以读取循环的周边状态，但其
  影响仅限于范围狭窄的类型化决策。新的周边输入字段只能加入 `HookSnapshot`，绝不能
  加入各钩子的输入结构；后者只携带该调用点专属的载荷（`tool_call`、`tool`、
  `result`）。
- `snapshot.context` 是该钩子边界上已提交的上下文：`transform_context` 看到已提交
  上下文以及待处理的一次性注入；工具钩子看到的已提交上下文包括当前周期中已提交的
  结果；`after_tool_call` 绝不会看到它自己尚未提交的结果（该结果位于 `result` 载荷中）；
  `prepare_next_turn` 看到该周期完整的已提交结果。`snapshot.usage` 是最近一次模型响应
  报告的词元用量；如果提供方从未报告，则为 `None`。内核只透传该值而不进行累加；
  需要累计语义的处理器自行维护总量。
- 三个工具钩子接收借用的 `ToolHookTarget`，其中包含有效授权元数据
  `ToolKind`/`DangerLevel` 和 `ToolSpec`；内核在调用前完成查找，处理器绝不自行查询
  注册表。`authorization` 是该次调用的有效值：在 `transform_tool_call` 处是注册表声明
  的默认值；在 `decide_tool_call` 和 `after_tool_call` 处则是经转换覆盖后的值（如果有）。
  内核只负责传递该值，不解释其含义。注册表声明只是一个默认依据，由工具注册的
  `ToolKind`/`DangerLevel` 和注册表的 `ToolPermissionMode`（`stratum-tools`）推导而来，
  绝非最终裁定；某次调用是否需要审批，由钩子链作出判断。
  `ToolExecutor::hook_lookup` 是内核对注册表的唯一隔离点：一次查找同时解析工具缺失
  检查关口、用于分发的工具句柄以及该默认声明。
- 身份由内核持有：钩子绝不得更改 `CallId` 或工具名称。`transform_tool_call` 只能继续，
  或返回一个 `Modify`，其中携带可选的替代参数和/或可选的授权覆盖
  （`PreAuthorize` 或 `Set`）；所有字段均未改变的 `Modify` 会被拒绝，并返回
  `HookFailure::InvalidOutput`。内核在作出决策前重新校验转换后的参数，并将有效授权
  传递给决策阶段和调用后阶段，不做任何合理性检查（包括降级检查）；覆盖授权是处理器的
  明确责任。`decide_tool_call` 只能返回 `Execute` 或 `Block`，绝不能修改参数，因此
  审批者看到的参数始终与实际执行的参数完全一致。阻止调用时会跳过
  `ToolExecutionStarted`，并生成固定的模型可见结果
  `{"error":{"code":"hook_blocked",...}}`，该结果仍会经过 `after_tool_call`。
  `after_tool_call` 只能替换 JSON 结果；内核使用原始 `CallId` 重建工具消息。
- 用户审批是一个普通的 `decide_tool_call` 处理器：批准映射为 `Execute`，拒绝映射为
  `Block`，向人工询问的通道由处理器实现私有持有。内核没有审批概念，也绝不会自行
  发出 `ToolApprovalRequested`/`ToolApprovalResolved`；这些持久化审批事实由
  `stratum-api` 中组合层的审批处理器通过事件接收端写入。审批后、分发前发生崩溃时按
  失败安全处理：恢复过程要么复用日志中已完成的决策（不再询问处理器），要么使用其
  原始身份重试待处理的调用。
- `transform_context` 补丁和 `prepare_next_turn` 注入都是仅请求视图：它们绝不写回
  已提交上下文，绝不发出持久化消息，也绝不会出现在 `LoopOutcome.new_messages` 中。
  `transform_context` 决策可以是 `Unchanged` 或 `Patch(ContextPatch)`
  （`ReplaceSystemPrompt` / `DropHistory { upto }` /
  `RewriteHistory { upto, summary }` / `Composite`）；内核会校验 `upto`：它必须是已提交
  `messages` 中一个
  从零开始、左闭右开的前缀末端索引，必须在边界内，且不得切断
  `tool_call`/`tool_result` 对；无效补丁以 `HookFailure::InvalidOutput` 拒绝。
  `Composite` 按顺序针对每个子补丁产生的演进视图校验各子补丁；空组合和嵌套组合
  会以 `HookFailure::InvalidOutput` 拒绝（嵌套组合无法推进校验视图，否则可能在应用时
  触发 `panic`）。注入的用户消息只由下一次模型请求消费一次；空注入或非用户角色注入
  会以 `HookFailure::InvalidOutput` 拒绝。
- 每次钩子调用都记录到同一条 `DurableEventSink` 事件流中：
  `HookInvocationPending`（地址为 `(iteration, HookPoint, Option<CallId>)`，并带有
  载荷层的输入摘要）在调用运行时之前提交；`HookInvocationCompleted` 在决策校验后、
  受影响的操作（模型请求、`ToolExecutionStarted`、结果提交或迭代边界）之前提交；
  类型化失败、超过截止时间和无效决策会提交 `HookInvocationFailed`。工具钩子的摘要
  对该钩子实际观察到的确切 `ToolCall` 的规范 JSON 计算哈希；上下文钩子对其
  `(iteration, point)` 地址计算摘要。用量和历史记录绝不参与摘要计算。
- `AgentLoop::resume` 根据持久化事件流重新运行一次运行过程：组合方重新提供系统提示和
  配置；重放通过 `MessageAppended` 重建已提交上下文，把前沿固定在最大
  `IterationCompleted` 之后一位，并拒绝终止态运行。内核不理解的事件变体会按失败关闭，
  返回 `ResumeError::UnsupportedEvent`；只有审批事实事件
  （`ToolApprovalRequested`/`ToolApprovalResolved`）会被显式跳过，因为它们不携带
  内核恢复状态。已提交的工具结果必须严格构成前一个助手消息中 `tool_calls` 的有序前缀
  （未知、重复、稀疏或乱序结果均按失败关闭）；缺失的后缀按至少一次立场重新执行。
  钩子调用会先查询日志：输入摘要匹配的已完成决策直接复用而不调用运行时；待处理调用
  使用其原始身份重试；失败会被重现；摘要不匹配时按失败关闭。
- 每次钩子调用都通过内核共享的执行辅助函数：调用前取消和执行中取消都解析为循环取消；
  绝对截止时间映射为 `HookFailure::TimedOut`；运行时失败报告为
  `AgentLoopError::Hook`，且只携带 `HookPoint` 和安全的失败类别，绝不携带提示、工具载荷
  或处理器内部信息。各钩子点的截止时间通过 `LoopLimits::hook_timeouts` 配置；
  `decide_tool_call` 默认没有截止时间（只响应取消），以容纳人工审批延迟；其他所有点
  默认使用按失败关闭的超时。
- `LoopOutcome.completion` 是 `LoopCompletionReason`，用于区分
  `Model(FinishReason)` 与 `HookStopped`；持久化事件 `LoopFinished` 的 `reason` 投影为
  `hook_stopped` 等稳定字符串。钩子停止绝不得伪装成提供方的结束原因。
- `ChainHookRuntime`（`hook_runtime/chain.rs`）是按顺序串联处理器的 `HookRuntime`；
  内核仍只看到一个运行时，因此取消、截止时间以及钩子点日志语义保持不变。
  `HookHandler`（`hook_runtime/handler.rs`）提供与五个钩子方法对应的方法、空操作默认
  实现，以及带不可变 `HookHandlerVersionId` 的描述符。各调用点的链语义如下：
  转换/调用后阶段按处理器顺序传递持续演进的视图（使用 `Cow`，未修改时零拷贝）；
  决策阶段遇到首个 `Block` 时短路；准备阶段遇到 `Stop` 时短路（丢弃已收集的注入），
  并按处理器顺序合并多个 `Inject` 载荷。任一处理器失败或返回无效决策，都会使整个
  调用点按失败关闭。
- 链顺序是被固定的数据，而不是私有代码：`ChainHookRuntime` 在构造时根据有序的处理器
  版本计算其 `ExtensionSetVersionId`，内核随 `LoopStarted` 提交该值；如果注入的
  运行时报告了不同版本，`resume` 会按失败关闭。不报告版本的运行时会跳过该检查。
- 处理器版本身份由处理器作者通过 `HookHandler::descriptor()` 自行声明；内核和链只负责
  消费该身份。契约包含两部分：同一个处理器版本的 ID 必须稳定（构造时只创建一次，
  或以确定性方式派生；绝不得在每次调用 `descriptor()` 时在其内部调用
  `HookHandlerVersionId::new()`，否则链版本会随每次调用改变，导致每次恢复都失败），
  并且任何决策行为变更都必须分配新的 ID。内核能够检测“ID 已变”，但不能检测
  “行为已变而 ID 未变”；当处理器成为可分发制品，且其版本由内容摘要（S1/S2）或
  固定服务身份（R3）派生时，这一缺口才会闭合。
- `prepare_next_turn` 还可以返回 `Compact { upto, summary }`：处理器提供摘要，内核在
  迭代边界执行持久化压缩。提交前会强制执行这些不变量：`upto` 非零、在边界内、不切断
  `tool_call`/`tool_result` 对，也不切入当前迭代中已提交的消息；摘要必须是普通系统消息
  （不得携带工具身份或推理内容）。`Compact.upto` 始终是准备阶段快照所示已提交上下文
  的索引；绝不得复用从补丁请求视图计算出的索引，并且每次压缩后都要重新计算。
  内核使用稳定标记模板（`COMPACTION_MARKER_PREFIX` + 换行 + 正文，见
  `agent_loop/compaction.rs`）包装摘要，并在迭代边界之前提交
  `TranscriptCompacted`；标记消息属于已提交历史记录（也会出现在
  `LoopOutcome.new_messages` 中），因此处理器能够在快照上下文开头检测先前的压缩。
- 压缩可安全重放：事件日志保留每条原始消息，重放时按顺序应用
  `TranscriptCompacted`；如果在已记入日志的 `Completed(Compact)` 和压缩事件之间
  发生崩溃，则通过重放已记录的摘要来闭合状态——绝不重新调用处理器，也绝不重新生成
  摘要。`compacted_iterations` 会对发生在压缩事件之后、迭代边界之前的崩溃进行去重。
  压缩绝不改变钩子寻址或摘要。
- 以下内容推迟到后续里程碑（不得在此加入）：每个处理器粒度的日志记录
  （H3b 评估）、Skill/Script/服务适配器，以及钩子遥测或实时传输载荷。
  另有一项待评估事项：事件流记录了版本，但注入的运行时不报告版本时，恢复的链版本
  检查仍会通过（用无版本运行时替换该链可绕过保护）；需要决定这种组合是否应按失败关闭。

## 预备恢复衔接面

- 唯一获准用于恢复组合的内核衔接面是纯计算的 `prepare_resume`：一个确切的
  `Arc<AgentLoop>` 会生成绑定到同一运行时的不透明预备值；该值既不实现 `Clone`，也不
  实现 `Serialize`，并且只暴露一个会消耗自身的 `run(token)` 路径。
- `prepare_resume` 不执行任何 I/O：不追加持久化事件，不调用模型、工具或钩子，也绝不
  接收 Postgres、Session、托管或分页相关内容。组合方（`stratum-api`）构建并校验类型化
  重放窗口；预备值复用现有的私有重放校验器，使恢复组合绝不复制内核状态机。
- 全新运行、持久化事件接收端确认和工具顺序执行的语义保持不变。
