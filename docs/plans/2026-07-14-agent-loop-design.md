# Agent Loop 内核设计

日期：2026-07-14

状态：已确认

## 目标

为 Stratum 建立一个独立、强类型、默认安全的基础 agent loop。loop 只负责在 LLM 与工具之间推进控制流，不负责 session 管理、历史加载或具体持久化实现。

设计参考 pi 的 `packages/agent`：低层 loop 接收上下文、prompt、取消信号和事件输出端口；有状态的 session、扩展与 UI 位于更高层。Stratum 保留自身的强类型事件、持久化确认和安全工具执行约束，不复制 pi 的动态类型或进程内扩展模型。

## 已确认决策

1. 持久化是事件消费者，不属于 `AgentLoop` 的业务逻辑。
2. 关键持久化事件采用 fail-closed：未确认就停止 loop。
3. 授权与人工审批属于 `ToolExecutor` 执行管线；事件消费者只记录过程。
4. durable 与 telemetry 使用两个显式端口，不通过布尔参数选择可靠性。
5. `AgentLoop` 不依赖 `EventStreamBus`；基础设施适配由组合层完成。
6. 第一阶段工具顺序执行，不实现并行批次。

## 架构

```text
调用方提供 LoopContext 和 prompts
              |
              v
          AgentLoop
          /       \
         v         v
DurableEventSink  TelemetryEventSink
         |         |
         v         v
  Store consumer  Event stream adapter
         |         |
         +-----> EventStreamBus

AgentLoop -> LlmProvider
AgentLoop -> ToolExecutor -> Policy / Approval -> ToolRegistry
```

### `stratum-agent`

- 定义并实现具体的 `AgentLoop`。
- 定义 loop 所需的 durable 与 telemetry 事件及其 sink trait。
- 实现具体的 `ToolExecutor`，封装参数校验、策略、审批和工具调用。
- 不加载历史，不管理 session，不决定存储格式。
- 不直接依赖 `stratum-store` 或 `EventStreamBus`。

`ToolExecutor` 第一阶段只有一个真实生产实现，因此保持具体类型，不提前创建 trait。LLM provider、durable sink 和 telemetry sink 存在真实多实现边界，继续使用 trait。

### `stratum-store`

- 实现 `DurableEventSink`。
- 收到 durable event 后先更新 store。
- store 成功后返回，即代表持久化确认。
- 可在提交成功后转发已提交事件；转发失败只记录结构化 warning，不否定已经完成的持久化。

### 组合层

- 将运行范围信息绑定到 sink，包括 `AgentId`、`RunId` 和 `TurnId`。
- 将 telemetry sink 连接到内存、NATS 或其他 event stream 实现。
- `AgentLoop` 产生的领域事件不携带 session 管理职责。

## Loop API

概念接口如下，最终命名可在实现时依据现有公共类型微调：

```rust
pub async fn run(
    &self,
    context: LoopContext,
    prompts: Vec<ChatMessage>,
    cancellation: CancellationToken,
) -> Result<LoopOutcome, AgentLoopError>;
```

`LoopContext` 包含：

- system prompt
- 已提交消息历史
- 当前可用工具描述

`LoopOutcome` 包含：

- 本次运行新增的完整消息
- finish reason
- 累积 token usage

调用方负责准备已有历史。loop 负责在第一次 LLM 调用前提交本次 prompts，从而保证模型不会看到未持久化的输入。

## 事件端口

### Durable event

durable event 至少覆盖：

- loop started
- message appended
- tool approval requested
- tool approval resolved
- tool execution started
- iteration completed
- loop finished
- loop failed
- loop cancelled

```rust
pub trait DurableEventSink: Send + Sync {
    async fn append(
        &self,
        event: DurableAgentEvent,
    ) -> Result<(), DurableEventSinkError>;
}
```

成功返回 `()` 就是确认。当前 loop 不消费存储 sequence，因此不增加无实际用途的 ack wrapper。

### Telemetry event

telemetry event 包含：

- LLM started、delta 和 finished
- tool execution progress
- 非关键诊断信息

```rust
pub trait TelemetryEventSink: Send + Sync {
    async fn emit(&self, event: AgentTelemetryEvent);
}
```

telemetry sink 在实现内部处理发布失败。失败可以记录一次 structured warning，但不能改变 loop 控制流。

durable 与 telemetry 使用不同 enum，使可靠性要求在类型层面可见，避免调用点误标。

## 数据流与提交顺序

每次状态变化遵守同一个不变量：

```text
构造 durable event
-> 等待 append 成功
-> 更新内存中的 committed LoopContext
-> 允许下一项外部动作
```

完整循环为：

```text
提交 prompts
-> prompts 加入 context
-> 调用 LLM
-> 流式 delta 发送 telemetry
-> 提交完整 assistant message
-> assistant message 加入 context
-> 处理 tool calls
-> 逐个提交 tool result
-> tool results 加入 context
-> 提交 iteration completed
-> 如果需要，进入下一次 LLM 调用
```

任何 durable append 失败后：

- 不提交对应内存状态。
- 不执行后续 LLM 或工具调用。
- 返回 `AgentLoopError::Durability`。
- 可以发出 best-effort telemetry。
- 不再尝试通过同一个失效通道持久化 `LoopFailed`。

## 工具执行

工具调用顺序为：

```text
assistant message 已持久化
-> 查找工具并校验参数
-> 必要时持久化 approval requested
-> 等待审批
-> 持久化 approval resolved
-> 持久化 tool execution started
-> 调用工具
-> 持久化 tool result message
```

参数校验失败不会产生 `ToolExecutionStarted`，只产生模型可见的错误 tool result。

以下失败转换成 tool result，让模型有机会修正：

- 工具不存在
- 参数校验失败
- 审批拒绝
- 工具执行失败

`ToolExecutionStarted` 必须紧贴真实工具调用之前持久化。若工具产生了外部副作用，但进程在 tool result 持久化前崩溃，存储中会留下 started-without-result。该状态表示结果未知，未来恢复逻辑不得自动重试，除非工具明确提供幂等保证。

第一阶段所有工具顺序执行。并行工具批次在持久化与取消语义稳定后另行设计。

## LLM 协议

- partial assistant delta 只作为 telemetry，不进入 committed context。
- 只有完整 assistant message 才持久化。
- LLM stream 中断时丢弃 partial message，并尝试持久化 loop failed。
- assistant 中实际存在 tool calls 时进入工具阶段，finish reason 仅用于补充协议判断。
- 如果 `finish_reason == length`，不执行其中任何 tool call，而是为每个调用生成错误 tool result，避免执行被截断但仍能解析的参数。

## 取消

- 调用 LLM 或工具前检查 `CancellationToken`。
- LLM streaming、审批和工具执行接收同一个 token。
- 尚未发生外部动作时，持久化 loop cancelled 后返回。
- 工具已经开始后，以工具真实返回为准。
- 工具若完成，先持久化真实结果，再结束 loop。
- durable sink 失败时不再尝试持久化 cancelled 或 failed 终态。

## 错误模型

library 使用 `thiserror` 定义类型化错误并保留 source chain。概念错误类型为：

```rust
#[non_exhaustive]
pub enum AgentLoopError {
    Durability { source: DurableEventSinkError },
    Llm { source: LlmError },
    InvalidProtocol { reason: ProtocolError },
    Cancelled,
    LimitExceeded { limit: LoopLimit },
}
```

工具查找、参数、审批拒绝和工具自身失败不属于 `AgentLoopError`。只有无法维持 loop 不变量的错误才终止运行。

如果原始错误发生后，终态事件也无法持久化，最终错误以 durability failure 为主，同时保留原始错误上下文。

## 测试策略

单元测试使用 scripted LLM、recording durable sink 和 fake tools，验证：

- prompt ack 先于 LLM 调用。
- assistant ack 先于工具阶段。
- approval resolved 和 tool execution started ack 先于真实工具调用。
- tool result ack 先于下一次 LLM 调用。
- 任意 durable 边界失败后不发生后续外部动作。
- 工具查找、校验、审批和执行错误转换成 tool result。
- truncated assistant 中的工具调用不会执行。
- partial assistant delta 不进入 committed context。
- 工具开始后结果无法持久化时留下 started-without-result。
- LLM、审批和工具各阶段取消时保持规定顺序。
- turn 与 tool-call limit 在额外外部动作前生效。

`stratum-store` 增加集成测试，验证：

```text
store 写入成功 -> ack -> 转发已提交事件
store 写入失败 -> 不 ack -> 不转发为已提交事件
```

## 非目标

本阶段不实现：

- session 创建、切换和历史加载
- crash resume/reconciliation 完整流程
- 插件发现、加载与隔离
- steering/follow-up 消息队列
- 并行工具执行
- 自动重试
- context compaction
- 动态模型或工具切换

现有高层 `Agent` 可以暂时作为兼容编排层，逐步改为调用新的 `AgentLoop`。本设计只固定底层 loop 契约，不扩展 session API。
