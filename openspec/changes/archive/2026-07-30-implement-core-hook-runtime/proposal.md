## Why

M0 已经冻结 Hook 的身份、版本与恢复边界，但新的 session-independent `AgentLoop` 仍把模型上下文、Tool 调用和下一轮控制写死在循环中，尚无真正可调用的策略入口。现在需要先完成 TODO H1 的最小运行时合同，才能在不继续膨胀 Agent 内核的前提下接入 Skill，并为后续有序 Handler runner 与 journal 提供稳定边界。

## What Changes

- 在 `stratum-agent` 定义单一、可注入的 `HookRuntime`，包含 `transform_context`、`before_tool_call`、`after_tool_call` 和 `prepare_next_turn` 四个异步合同。
- 为四个 Hook 定义最小、类型化的输入与 decision：模型请求上下文变换；Tool 参数继续、修改或阻断；Tool 结果保留或替换；下一轮继续、停止或注入用户消息。
- 在 `AgentLoopBuilder` 提供 Hook Runtime 注入，并以 No-op Runtime 作为默认值，使未配置 Hook 的现有执行顺序、消息和 Tool 行为保持不变。
- AgentLoop 对每次 Hook 调用同时传递取消信号与 deadline，并把 Handler 失败、无效输出、超时和取消映射为类型化、脱敏且 fail-closed 的循环结果。
- 明确 Tool Block 不进入审批或 Tool 执行，但生成结构化、模型可见的 Tool 结果；Hook 修改不能改变 `CallId` 或 Tool 名称。
- **BREAKING** 区分模型 `FinishReason` 与 Hook 主动停止，避免把 `prepare_next_turn` 的 Stop 伪装成模型结束原因。
- 当前产品范围只覆盖新的 `AgentLoop` 内核及其测试合同；标记为 legacy 的 stateful `Agent`、`stratum-api`、SSE 和 `/chat` 暂不接入 Hook。
- 非目标：H2 的多 Handler 顺序、Tool 前后双重校验和审批重排；H3 的 Hook journal、Resume 复用和存储后端；Skill/Script/Rust Service adapter；任何 EventBus Hook 事件或前端配置界面。
- 本 change 不取代旧 change；它承接已归档的 `establish-hook-runtime-baseline`，实现其中明确推迟到 H1 的执行合同。当前没有需要替代的活跃 OpenSpec change。

## Capabilities

### New Capabilities

- `agent-hook-runtime`: 定义四个核心 Hook 的输入、decision、执行位置、No-op 行为、取消/deadline、错误和 AgentLoop 集成语义。

### Modified Capabilities

无。现有 `hook-execution-baseline` 继续约束未来 Handler identity、journal 与恢复；本 change 不改变其要求。

## Impact

- 主要影响 `crates/stratum-agent` 的公开 API、`AgentLoopBuilder`、循环控制流、Tool 调用边界和 kernel 测试。
- 复用 `stratum-core` 已有的 `HookPoint` 与 `HookFailure`，不新增 Cargo crate，不引入新的执行后端或存储 trait。
- `LoopOutcome` 的结束原因类型会发生 beta 期破坏性调整；当前仓库内调用方和测试需要同步更新。
- legacy Agent、Store、EventBus、API、Web 和持久化格式保持不变，因此本 change 不承诺 Hook decision 的进程重启恢复；该保证仍属于 H3。
