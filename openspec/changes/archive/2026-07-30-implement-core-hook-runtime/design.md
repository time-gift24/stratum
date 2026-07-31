## Context

M0 已在 `stratum-core` 定义 `HookPoint`、`HookFailure`、Handler/version identity 与未来 journal record，但没有执行 Hook。当前真正适合作为策略注入点的是 `stratum-agent::AgentLoop`：它是 session-independent kernel，拥有模型请求、顺序 Tool 调用和迭代边界，同时不拥有 Session、Store 或 EventBus。标记为 legacy 的 stateful `Agent` 仍承担现有 API 兼容路径，不应继续复制新的循环策略。

`AgentLoop` 当前直接从 committed `LoopContext` 生成 `ChatRequest`，把每个可执行 `ToolCall` 交给 `ToolExecutor`，将 Tool result 持久化后直接进入下一次模型迭代。H1 需要在这些既有边界上增加四个策略入口，但不能提前实现 H2 的 Handler chain 或 H3 的 journal。

## Goals / Non-Goals

**Goals:**

- 在 `AgentLoop` 中执行四个核心 Hook，并保持每个 Hook 的输入、decision 和作用范围明确。
- 未注入自定义 Runtime 时完全保持现有 kernel 行为和耐久边界。
- 让 Tool Block、参数修改、结果替换、下一轮停止和一次性消息注入成为类型可表达的结果。
- 对所有 Hook 统一执行取消、deadline、输出校验和脱敏错误映射。
- 保持 AgentLoop 不感知 Session、Agent、Turn、Handler 顺序、journal 或 EventBus。

**Non-Goals:**

- 不修改 legacy stateful `Agent`、API host、Store、SSE 或 Web。
- 不实现多个 Handler 的解析、排序、短路或 version 固定；`HookRuntime` 在 H1 中是一个已经组合好的单一边界。
- 不实现 Hook invocation journal、input digest、恢复复用或 Pending retry。
- 不调整 Tool 的原始参数预校验、最终参数复验和审批顺序；这些属于 H2。
- 不实现 Skill、Script、链接式 Rust Handler 或 Hook Service adapter。
- 不发布 Hook runtime event，也不把 decision 写入 Agent message。

## Decisions

### 1. Hook Runtime 属于 `stratum-agent`，暂不拆 crate

新增 `hook_runtime` 功能模块，并按仓库约定分离接口、错误和 No-op 实现。`stratum-agent` 公开 re-export 合同；`AgentLoop` 持有 `Arc<dyn HookRuntime>`，`AgentLoopBuilder::hook_runtime` 允许注入，缺省使用 `NoopHookRuntime`。

H1 只有 AgentLoop 一个真实消费者，单独建立 `stratum-hook` crate 只会增加依赖和 facade。等 H2/H3 形成独立 runner/journal 边界，或 extension host 出现第二个消费者后，再依据真实依赖决定是否拆 crate。

`HookRuntime` 使用 dyn-compatible async trait，因为 Runtime 是应用组合边界，必须允许不同实现通过 `Arc` 注入。它表示一整个策略运行时，而不是单个 Handler；因此 H1 不暴露 Handler 列表、位置或 `HookInvocationId`。

**否决方案：** 把四个 closure 分别放进 `AgentLoopBuilder`。这会形成四套错误、取消和 deadline 包装，也无法成为 H2 runner 的单一替换边界。

### 2. 输入使用借用视图，decision 只拥有发生变化的数据

四个合同都接收迭代位置和最小借用输入；No-op decision 不 clone 历史或 Tool payload。建议的公开形状为：

```text
transform_context(
  TransformContextInput { iteration, context: &LoopContext }, control
) -> TransformContextDecision
  = Unchanged | Replace { context: LoopContext }

before_tool_call(
  BeforeToolCallInput { iteration, tool_call: &ToolCall }, control
) -> BeforeToolCallDecision
  = Continue
  | ModifyArguments { arguments: Value }
  | Block { reason: String }

after_tool_call(
  AfterToolCallInput { iteration, tool_call: &ToolCall, result: &ChatMessage }, control
) -> AfterToolCallDecision
  = Keep | ReplaceResult { result: Value }

prepare_next_turn(
  PrepareNextTurnInput { iteration, context: &LoopContext }, control
) -> PrepareNextTurnDecision
  = Continue | Stop | Inject { messages: Vec<ChatMessage> }
```

`before_tool_call` 只能修改 arguments；AgentLoop 保留原 `CallId` 和 Tool name。`after_tool_call` 只能替换 JSON result；AgentLoop 重新构造相同 `CallId` 的 Tool message。`prepare_next_turn` 的 Inject 只接受非空 User message 列表，拒绝其他 role、Tool call、reasoning 或 tool-call identity，避免伪造 committed Assistant/Tool 历史。

Block reason 必须非空；AgentLoop 将其编码为固定结构：

```json
{
  "error": {
    "code": "hook_blocked",
    "message": "<safe reason>"
  }
}
```

**否决方案：** 允许 Hook 返回任意 `ChatMessage` 或完整 `ToolCall`。这会让 Hook 改写 role、`CallId`、Tool name 和关联关系，使非法状态重新进入公共 API。

### 3. Context 变换和 Inject 都是模型请求视图，不是 Agent 历史

每次模型调用前，AgentLoop 从 committed `LoopContext` 和最多一批待消费的 Inject message 构造 request view，再调用 `transform_context`。Replace decision 只改变本次 `ChatRequest`；不回写 committed context，不产生 `MessageAppended`，也不进入 `LoopOutcome.new_messages`。

`prepare_next_turn::Inject` 产生的 User messages 只加入下一次模型请求 view，并在该次请求开始时消费一次。这样 Hook 内容不会伪装成最终用户写入的 Agent history。H3 将通过 journal 保存和恢复 Inject decision；H1 明确不承诺崩溃后的 decision 复用。

**否决方案：** 把 Inject message 当作普通 User message提交到 DurableEventSink。该方案虽然可以利用现有恢复，但会污染对话历史和 UI，并把 Hook 生成内容错误归属为用户输入。

### 4. 四个 Hook 放在现有外部动作与耐久边界之间

执行顺序固定为：

```text
每次模型迭代
  cancellation check
  consume pending Inject into request view
  transform_context
  LLM request + assistant durable commit

  对 finish_reason=tool_calls 的每个 ToolCall，顺序执行
    before_tool_call
      Block  -> 构造模型可见结果，不进入 ToolExecutor
      Continue/Modify -> ToolExecutor（现有 lookup/validate/approval/start/call）
    after_tool_call
    Tool result durable commit

  prepare_next_turn
    Continue -> iteration durable commit -> 下一迭代
    Inject   -> 保存为下一次 request-only 输入 -> iteration durable commit -> 下一迭代
    Stop     -> iteration durable commit -> LoopFinished(hook_stopped)
```

如果 provider 带 Tool call 但 finish reason 不是 `tool_calls`，继续使用现有不可执行错误结果，不调用 Tool Hook，因为 runtime 没有授权该 Tool cycle。Block 生成的结果会进入 `after_tool_call`，保证所有被授权 Tool cycle 最终形成的模型可见结果都经过同一结果变换边界。

`after_tool_call` 在 ToolExecutor 返回后、Tool result durable commit 前运行。此时 Tool 可能已经产生外部副作用；Hook 失败会 fail closed，并保留既有 `ToolExecutionStarted` 耐久事实。H3/H4 才解决跨崩溃的 result 与幂等问题。

**否决方案：** 在 `ToolExecutor` 内部直接调用所有 Hook。`transform_context` 与 `prepare_next_turn` 不属于 ToolExecutor；拆散注入会让 AgentLoop 和 ToolExecutor 各自拥有部分 Runtime，难以统一取消和错误语义。

### 5. AgentLoop 强制 deadline 与取消，Runtime 同时获得控制信息

增加轻量 `HookControl`，包含 cloned `CancellationToken` 和绝对 deadline。`LoopLimits` 增加有默认值的 Hook timeout，并提供 `#[must_use]` override。AgentLoop 使用一个内部 helper 对四个方法统一执行：

- 调用前已取消：不进入 Runtime，返回 loop cancellation；
- 调用中取消：停止等待 Hook，进入既有 `LoopCancelled` 路径；
- deadline 到期：返回 `AgentLoopError::Hook { point, failure: TimedOut }`；
- Runtime 返回安全 `HookFailure`：保留 Hook point 和类型化分类，不记录输入或内部错误正文；
- decision 结构违反合同：映射为 `HookFailure::InvalidOutput`。

Hook future 必须 cancellation-safe；在返回 decision 前不得启动无法安全放弃的外部副作用。远程重试和 invocation 幂等属于 H3/R3，不在 H1 假装提供。

**否决方案：** 只把 token/deadline 传给 Runtime而不由 AgentLoop 强制。这样一个错误实现可以无限阻塞核心循环，无法满足 H1 的统一超时合同。

### 6. 区分模型结束与 Hook 停止

新增 `LoopCompletionReason`：模型自然结束携带 `Model(FinishReason)`，`prepare_next_turn::Stop` 使用 `HookStopped`。`LoopOutcome` 改为暴露该类型；durable `LoopFinished.finish_reason` 对应稳定字符串，例如 `hook_stopped`。

这是 beta 期有意的破坏性修正。继续把 `FinishReason::ToolCalls` 或 `FinishReason::Stop` 用于 Hook Stop，会把执行策略伪装成 provider 行为，污染事件、指标和上层决策。

### 7. H1 不创建 Hook 观测或持久化的旁路

Hook decision 只通过返回值影响当前 AgentLoop。H1 不向 `TelemetryEventSink` 或 EventBus 增加 Hook payload，不借用 `DurableAgentEvent` 充当 journal，也不把 decision 编码进 metadata。这样 H3 可以在既有 `HookInvocationAddress` 基线上增加独立 Session/Turn journal，而无需从观察数据反推执行状态。

## Risks / Trade-offs

- **[风险] H1 decision 在进程崩溃后可能重新计算。** → 在 H3 完成前，不把自定义 Hook Runtime 接入承诺 Resume 的产品 composition；H1 只冻结执行合同和内核行为。
- **[风险] `transform_context` 可以复制或隐藏大量上下文，增加分配。** → 输入使用借用，No-op 不复制；只有明确 Replace 时承担 owned context 成本，后续以 profiling 决定是否增加 patch 表达。
- **[风险] Tool 已执行后 `after_tool_call` 失败会留下 started-only 状态。** → 维持 fail-closed，不伪造 Tool result；H3/H4 处理结果 journal 与幂等恢复。
- **[风险] request-only Inject 不出现在 Agent history，调试时不直观。** → 这是避免伪造用户历史的有意边界；未来仅通过安全的 Hook tracing/审计显示身份和分类，不记录内容。
- **[风险] 默认 No-op 掩盖未显式配置 Runtime。** → No-op 是 H1 明确的兼容行为；自定义 composition 测试必须显式注入 recording Runtime 并断言四个调用点。

## Migration Plan

1. 在 `stratum-agent` 增加合同类型、错误映射、No-op Runtime 和 re-export，不修改现有调用方。
2. 为 `AgentLoopBuilder` 增加默认 No-op 字段和可选注入方法，先证明现有 kernel 测试输出不变。
3. 依次接入 transform、before/after Tool 和 prepare-next-turn 边界，并为每个边界增加 decision 与失败矩阵测试。
4. 将 `LoopOutcome` 和仓库内 kernel 调用方切换为 `LoopCompletionReason`。
5. 运行 fmt、clippy、workspace tests，并更新 `TODO.md` 中 H1 的 builder 名称和 `stratum-agent/AGENTS.md` 的最终约定。

本项目仍处于 beta，本 change 只采用前向破坏性更新；不设计数据迁移、协议降级或回滚兼容路径。若实现验证失败，修正当前 change，不保留新旧 Hook Runtime 双轨。

## Open Questions

无阻塞问题。Handler chain、journal storage、Hook telemetry 内容和外部 wire protocol 均已明确留给 H2/H3/S2/R3，不能在实现 H1 时临时扩入。
