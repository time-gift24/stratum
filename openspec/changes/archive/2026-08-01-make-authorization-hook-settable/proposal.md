## Why

PR #38 评审中发现两个残留问题并已在代码中修正（commit `3bf87c8`）：`ToolExecutor::execute` 是一条公开的无闸门调度路径，且 `ToolHookTarget.authorization` 只是注册表静态声明的只读终判，无法表达 per-call 动态策略。本 change 是这次修正的归档追平，使 canonical spec 与已合入的代码行为一致。

## What Changes

- **BREAKING** `TransformToolCallDecision::ModifyArguments` 改为 `Modify(TransformToolCallModification)`：`arguments` 与 `authorization` 均为可选，`Modify` 全空（双 `None`）判 `HookFailure::InvalidOutput`。
- 新增 `AuthorizationOverride`（`PreAuthorize` / `Set { kind, danger }`）：注册表声明从终判降级为默认依据，`transform_tool_call` 可覆写，kernel 把生效授权搬运到 `decide_tool_call` 与 `after_tool_call` 而不解释它。
- **BREAKING** `ToolExecutor::execute` 从公共 API 收窄为 `pub(crate)` 纯机制：参数为已解析工具句柄与 decide 放行的最终 call，函数体只剩取消检查、`ToolExecutionStarted` 耐久提交与 dispatch；调度路径无授权概念、无重复校验。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `agent-hook-runtime`: transform 决策词汇扩展为可选参数修改与可选授权覆写；工具目标元数据中的授权为生效值（注册表默认可被 transform 覆写）；kernel 不解释授权值。

## Impact

- 已在 `crates/stratum-agent` 完成实现（`hook_runtime/runtime.rs`、`agent_loop/runner.rs`、`tool_executor/definition.rs` 与测试），本 change 只补 spec 追平与归档。
- 授权降级（`PreAuthorize`）成为可能，合同明确"覆写是 handler 的明示责任，kernel 不做合理性检查"。
