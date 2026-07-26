## ADDED Requirements

### Requirement: Runtime envelope 以 Session 为作用域
每个 `StreamEnvelope` 必须（SHALL）包含 `SessionId`、创建时间戳、类型化的 `RuntimeEvent` 和允许的 metadata。envelope 不得（SHALL NOT）包含 `RunId`、`EventSource` 或顶层消息序号字段。

#### Scenario: Agent 事件 envelope
- **WHEN** Agent 发出 runtime event
- **THEN** envelope 标识 Session，Agent event 变体自身携带 Agent、Turn 和 location 身份

### Requirement: 事件归属由事件变体编码
`RuntimeEvent` 必须（SHALL）使用不同的类型化变体表示 Session、Node 与 Agent 事件。每个变体必须（SHALL）包含对应事件族所需的全部主要身份，解码时必须拒绝不完整或互相矛盾的归属。

#### Scenario: Agent 直接运行事件
- **WHEN** Agent 在 Session 中直接运行
- **THEN** 其事件包含 `AgentId`、`TurnId` 和 `AgentLocation::Direct`，且不含 Workflow node 字段

#### Scenario: 内嵌 Agent 事件
- **WHEN** Agent 作为 Workflow 节点运行
- **THEN** 其事件包含 `AgentId`、`TurnId`，以及带有 `WorkflowVersionId` 和 `NodeId` 的 Workflow-node location

#### Scenario: Node 事件
- **WHEN** 普通 Workflow 节点发出生命周期或输出数据
- **THEN** Node 事件包含不可变 Workflow 版本和 `NodeId`

### Requirement: 删除 Run 事件语义
runtime 协议不得（SHALL NOT）暴露 run 生命周期变体。Agent Turn 的完成、失败或取消必须（SHALL）表示为 Agent 事件，并且不得（SHALL NOT）暗示长期存在的 Session 已完成、失败或被取消。

#### Scenario: Agent Turn 结束
- **WHEN** Agent 在仍然开放的 Session 中完成一个 Turn
- **THEN** 系统发出 Agent 完成事件，Session 身份仍可供后续 Turn 使用

### Requirement: 完整消息序号表示 Agent 历史顺序
`message_seq` 必须（SHALL）是已提交完整 `AgentEvent::Message` 变体中的必填 `u64` 字段。其唯一性和顺序作用域必须（SHALL）是 `(AgentId, message_seq)`。未持久化消息必须使用不含序号的独立 append 输入类型；非消息 runtime event 与 `StreamEnvelope` 均不得（SHALL NOT）包含 `message_seq`。

#### Scenario: 消息提交后发布
- **WHEN** Agent 产生一条尚未持久化的完整消息
- **THEN** 持久化边界接收不含序号的输入、分配下一个 `message_seq`，并且只发布包含该必填序号的已提交 Agent message event

#### Scenario: 来自不同 Agent 的消息
- **WHEN** 同一 Session 中的两个 Agent 分别提交具有相同数字 `message_seq` 的消息
- **THEN** 因为 Agent 身份是消息历史 key 的一部分，两条记录仍然不同

#### Scenario: Streaming delta
- **WHEN** Agent 发布 LLM delta 或生命周期事件
- **THEN** 事件及其 envelope 均不包含 `message_seq`

#### Scenario: Session stream 中交错多个 Agent
- **WHEN** Session stream 按传输到达顺序交错包含多个 Agent 的已提交消息
- **THEN** consumer 使用 `(AgentId, message_seq)` 进行消息排序、分页和去重，并且不把 `message_seq` 解释为 Session 全局序号

### Requirement: Event cursor 仅用于传输
`EventCursor` 必须（SHALL）继续作为 `EventRecord` 携带的不透明保留传输位置。runtime、history 与 Hook 逻辑不得（SHALL NOT）将其与 `message_seq` 比较，也不得将其用作持久化恢复状态。

#### Scenario: SSE 重连
- **WHEN** client 使用保留的 cursor 重新连接
- **THEN** EventBus 只使用该 cursor 选择保留的 Session stream 位置

### Requirement: EventBus 以 Session 为作用域
EventBus 必须（SHALL）接收类型化的 Session、Node 和 Agent runtime event，并且必须按 `SessionId` 发布和订阅。Agent 身份不得（SHALL NOT）作为传输分区 key。

#### Scenario: 一个 Session 订阅接收多个 Agent 的事件
- **WHEN** 不同 Agent 在同一 Session 中发出事件
- **THEN** 一个 Session 订阅可以接收两者的事件，并保留各自不同的 Agent 身份

### Requirement: Metadata 不携带主要语义
Runtime metadata 不得（SHALL NOT）包含解释核心行为所必需的 Session、Workflow version、Node、Agent、Turn、Hook invocation 或 event-state 字段。敏感 prompt、tool 参数、tool 结果、secret、credential 和 host 路径不得（SHALL NOT）记录在 metadata 中。

#### Scenario: 核心 UI 投影
- **WHEN** client 投影 runtime event
- **THEN** 无需读取 metadata 即可确定事件族与主要归属

### Requirement: Beta 协议变更必须明确
对于使用已被替换的 run-oriented envelope，且无法无歧义地解码为新 Session 协议的持久化或入站记录，runtime 必须（SHALL）拒绝。runtime 不得（SHALL NOT）根据相互冲突的旧字段静默合成 Session 语义，也不得（SHALL NOT）包含旧协议双读双写、数据转换或回滚兼容路径。

#### Scenario: Runtime 边界收到旧 envelope
- **WHEN** 旧 envelope 包含 `run_id` 和 `source`，但不包含 `session_id`
- **THEN** runtime 以不受支持的协议版本为由拒绝该记录

#### Scenario: 部署新协议时存在 beta 数据
- **WHEN** 部署发现使用旧 schema 或旧事件协议的 beta 数据
- **THEN** 部署流程丢弃不兼容数据并重新初始化，不尝试转换、降级或保留回滚兼容性
