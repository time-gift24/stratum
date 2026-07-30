## Why

H1 建立了单一 `HookRuntime` 边界，但工具审批仍以独立的 `ToolApproval` trait 寄生在 `ToolExecutor` 内部，形成第二个策略注入点，并且审批发生在 Hook 变换之前，无法满足 H2"审批所见即所执行"的验收条件。Hook 层定位是内部使用的无侵入主循环改造手段，审批作为最典型的执行决策，应当完全收敛为 Hook，使 `ToolExecutor` 只剩机制（lookup、validate、started、call）。

## What Changes

- **BREAKING** 将 H1 的 `before_tool_call` 拆名为两个 Hook 点：`transform_tool_call`（只允许 Continue / ModifyArguments）与 `decide_tool_call`（只允许 Execute / Block）。审批成为 `decide_tool_call` 相位的一个普通 Handler：Approve 映射 Execute，Reject 映射 Block。
- 从 `stratum-agent` 公共 API 移除 `ToolApproval` trait、`ToolApprovalRequest` 与 `ToolExecutor` 的审批构造参数；`ToolExecutor` 不再发起或记录审批交互。legacy Agent 自有审批路径（`loop.rs` 的 `ActiveApprovalGuard`）不受影响。
- **BREAKING** 富化 Tool Hook 输入：`transform_tool_call`、`decide_tool_call` 与 `after_tool_call` 的输入在 `iteration` 与 `tool_call` 之外携带工具授权元数据（`ToolKind`、`DangerLevel`）与 `ToolSpec`（供审批界面等展示）。
- 执行顺序固定为：lookup 与授权元数据 → 原始参数校验 → `transform_tool_call` → 最终参数复验 → `decide_tool_call` → `ToolExecutionStarted` → 调用，保证审批展示的参数与实际执行参数一致。
- **BREAKING** Hook deadline 从单一 `hook_timeout` 改为按 HookPoint 配置；`decide_tool_call` 默认无 deadline（仅受取消约束），以容纳人肉审批的分钟到小时级等待，其余 Hook 点保留 fail-closed 超时。
- 新 AgentLoop 路径不再提交 `ToolApprovalRequested` / `ToolApprovalResolved` 耐久事件；审批交互通道（如何问人）归审批 Handler 实现私有。崩溃于审批通过后、执行前的恢复语义为重复提示（fail-safe），去重与审计由后续 hook journal（H3）承载。
- 非目标：多 Handler 顺序链与短路（H2 后半）、hook journal 与恢复去重（H3）、`stratum-tools` 统一校验边界重建、legacy Agent、API/SSE/Web 审批交互协议、前端审批界面。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `agent-hook-runtime`: `before_tool_call` 拆分为 `transform_tool_call` 与 `decide_tool_call`；Tool Hook 输入富化授权元数据与 `ToolSpec`；deadline 按 HookPoint 配置且 `decide_tool_call` 默认无 deadline；工具审批收敛为 decide 相位 Handler，`ToolExecutor` 不再承担审批。

## Impact

- 主要影响 `crates/stratum-agent`：`hook_runtime` 合同与 No-op、`agent_loop` 的 Tool 调用编排、`tool_executor`（删除审批路径）、`LoopLimits`、crate 公共 API 导出与全部相关测试。
- `stratum-core` 的 `ApprovalDecision`、`ApprovalId`、`DurableAgentEvent::ToolApprovalRequested/Resolved` 保留给 legacy 路径使用，不删除。
- 仓库内 `ToolExecutor::new` 与 `ToolApproval` 调用方（含测试基建）需要前向切换；项目处于 alpha，采用前向破坏性更新，不保留双轨。
- 后续 H3 的 hook journal 需要为 `decide_tool_call` 记录 Pending/Completed，以消除崩溃恢复时的重复审批提示。
