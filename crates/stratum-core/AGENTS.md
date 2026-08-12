# stratum-core 运行时协议不变量

## 范围

`stratum-core` 仅保留领域类型、ID 新类型、内核事件枚举
（`DurableAgentEvent` / `AgentTelemetryEvent`）以及钩子寻址。旧有的
传输 DTO 接口——`StreamEnvelope`、`RuntimeEvent`、`AgentEvent`、
`EventCursor`、`ReplayStart`、`EventRecord`、`NewAgentMessage`、
`HistoryQuery`/`HistoryPage` 和 `message_seq`——已删除，且不得在此处
重新引入；线协议成帧属于 API 层。

## 基础 AgentLoop 事件契约

- `DurableAgentEvent` 和 `AgentTelemetryEvent` 是由基础 `AgentLoop` 发出的局部、
  不带作用域的事件；它们不是线协议封装，也不得加入 `AgentRuntimeId`、
  账本序号、游标、分页、托管或传输字段。
- `DurableAgentEvent` 标记循环正确性边界。循环必须等到注入的持久化事件接收端确认后
  才能继续推进。哪些变体需要持久化，以及如何将其读回，由具体的
  `stratum-postgres` 执行存储和 API 组合层决定。
- `AgentTelemetryEvent` 用于尽力而为的可观测性。遥测被丢弃、超时、不受支持或
  失败时，绝不得改变循环输出、工具分发、持久化前沿或终止状态。
- `ToolExecutionStarted` 是持久化事件，发生在工具查找、输入校验和审批之后、
  分发之前。`IterationCompleted` 也是持久化事件，用于标识可推进前沿的确切迭代
  及其累计用量。
- 两个枚举都使用稳定的 snake_case `type` 名称。新增变体时，必须同步更新
  `event_type()`、serde 测试，以及存储层和 API 层中下游的穷尽式投影。

## 托管模型配置

- `stratum-core` 持有公共的 `ModelConfig` 快照：由提供方确定作用域的 `ModelId`
  及其提供方专用参数对象，会成对跨越运行时与持久化边界。

## Session 与钩子运行时身份

- `AgentId` 标识一个不可变的 Agent 模板定义；`AgentRuntimeId` 标识长期存活的外层
  运行时聚合。核心层可以在运行时快照和钩子地址中携带 `AgentId`，用作定义固定值；
  但 `AgentRuntimeId` 只属于 API、存储和基础设施组合层，不得进入内核事件、
  `AgentLoop` 或预备恢复值。
- `SessionId` 是长期存活、独立于图的协作空间的 UUIDv7 身份。Session 保持稳定时，
  Agent 和 Workflow 的版本可以发生变化。
- 宿主方提供不可变的 `AgentRuntimeContext { session_id, location }`；Agent 创建
  `TurnId`。`AgentLocation` 可以是 `Direct`，也可以是类型化的 `WorkflowNode` 位置。
- 可恢复的 Turn 固定 `agent_id: AgentId`、已解析的 `ModelConfig`、ToolSet 指纹、
  SkillSet 版本、ExtensionSet 版本，以及有序的钩子处理器版本。
- 钩子调用地址绑定 Session、Agent、Turn、钩子点、处理器位置与版本、操作身份及
  输入摘要。待处理重试的身份保持稳定；不匹配时按失败关闭。
- 当前测试版协议会拒绝不受支持的状态和载荷形态。不得添加迁移、降级、回滚、
  双读或旧格式写入路径。
