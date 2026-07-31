## Context

H1（已归档的 `implement-core-hook-runtime`）在 `stratum-agent` 建立了单一 `HookRuntime`：`transform_context`、`before_tool_call`、`after_tool_call`、`prepare_next_turn` 四个合同，`AgentLoop` 持有 `Arc<dyn HookRuntime>`，统一执行 helper 强制取消与单一 `hook_timeout`。但 `ToolExecutor` 内部仍持有第二个策略边界 `ToolApproval`：它在 validate 之后、`ToolExecutionStarted` 之前发起审批交互，并提交 `ToolApprovalRequested`/`ToolApprovalResolved` 耐久事件。

这带来两个结构性问题：

1. 策略编排分裂：参数变换在 Hook 一侧，执行决策在 executor 一侧，`AgentLoop` 无法统一取消、deadline 和错误语义；H2 的"审批所见即所执行"验收条件在当前顺序（validate → 审批 → 执行，Hook 变换在审批之前已被绕过）下无法满足。
2. Hook 层的定位是**内部使用**的无侵入主循环改造手段，参数可以丰富；审批作为最典型的执行决策，继续留在 executor 里会让所有未来策略（限流、租户隔离、策略引擎）都面临"放 Hook 还是放 executor"的重复抉择。

事实核查结论：`ToolApproval` 仅被新 AgentLoop 路径使用；legacy Agent 在 `loop.rs` 有独立的 `ActiveApprovalGuard` 审批路径，不经过该 trait，本 change 不触碰。`ApprovalDecision` 只有 `Approve`/`Reject` 两个变体，映射到 Hook decision 无损耗。

## Goals / Non-Goals

**Goals:**

- 工具审批完全收敛为 Hook：从 `stratum-agent` 公共 API 移除 `ToolApproval`，审批以普通 Handler 形式存在于 `decide_tool_call` 相位。
- `before_tool_call` 按语义拆名：`transform_tool_call`（变换参数）与 `decide_tool_call`（执行决策），各自的 decision 词汇最小化。
- Tool Hook 输入富化：授权元数据（`ToolKind`、`DangerLevel`）与 `ToolSpec` 进入 `transform_tool_call`、`decide_tool_call`、`after_tool_call` 的借用输入。
- 固定执行顺序，使审批（decide）看到的参数一定是最终复验后、实际执行的参数。
- deadline 按 HookPoint 配置；`decide_tool_call` 默认无 deadline（仅取消），其余点保留超时保护。
- `ToolExecutor` 只剩机制：lookup、校验、`ToolExecutionStarted` 耐久、调用。

**Non-Goals:**

- 多 Handler 顺序链、Block/Stop 短路、Handler 版本固定（H2 后半，本 change 仍是单一已组合 Runtime）。
- Hook journal、崩溃恢复去重、审计（H3；本 change 只固定"重复提示是 fail-safe"的语义）。
- `stratum-tools` 统一校验边界重建、审批顺序之外的校验改动。
- legacy Agent、`stratum-api`、SSE/Web 审批交互协议、前端审批界面。
- 删除 `stratum-core` 的 `ApprovalDecision`、`ApprovalId`、`DurableAgentEvent::ToolApprovalRequested/Resolved`（legacy 路径仍在使用）。

## Decisions

### 1. 审批是 decide 相位的 Handler，不是独立 Hook 点，也不是 executor 组件

拆名后的两个 Tool 前置 Hook 点：

```text
transform_tool_call(
  TransformToolCallInput { iteration, tool_call: &ToolCall, tool: &ToolHookTarget }, control
) -> TransformToolCallDecision
  = Continue | ModifyArguments { arguments: Value }

decide_tool_call(
  DecideToolCallInput { iteration, tool_call: &ToolCall, tool: &ToolHookTarget }, control
) -> DecideToolCallDecision
  = Execute | Block { reason: String }
```

`ToolHookTarget` 携带授权元数据（`Option<(ToolKind, DangerLevel)>`）与 `ToolSpec`。审批 Handler 是 `HookRuntime` 组合内部的普通一员：它私有持有问人通道（mpsc、HTTP 回调、CLI prompt 均可），把 Approve 映射为 `Execute`、Reject 映射为 `Block { reason }`。kernel 不认识"审批"这个概念。

**否决方案 A：新增独立 `approve_tool_call` Hook 点。** 审批在语义上就是"问人后决定 Execute 还是 Block"，独立点会让 kernel 重新认识审批，违背消除策略分裂的目标；且第五个点的取消/deadline/错误语义与 decide 完全同构，是纯重复。

**否决方案 B：保留 `ToolApproval` trait 但移出 executor。** trait 的使用方只剩审批 Handler 一家，保留它只是把同样的边界换个位置，调用方仍需理解两套合同。直接删除，由审批 Handler 实现自行定义内部交互接口。

### 2. 相位之间由 kernel 插入最终参数复验

执行顺序固定为：

```text
lookup + 授权元数据（kernel/executor，缺失工具 → 现有错误结果，不进 Hook）
原始参数 validate
transform_tool_call  → Continue / ModifyArguments
最终参数 re-validate（复验失败 → 现有校验错误结果，不进 decide）
decide_tool_call     → Execute / Block
ToolExecutionStarted（durable）→ registry.call
after_tool_call（Keep / ReplaceResult，输入同样富化）
```

decide 相位**不允许**修改参数：decision 只有 `Execute | Block`。这是"审批所见即所执行"的结构性保证——审批之后没有任何环节能再改变参数，而不是靠约定。要改参数必须发生在 transform 相位，且改完必过复验。

Block 的模型可见结果沿用 H1 的固定结构 `{"error":{"code":"hook_blocked","message":...}}`，且仍然经过 `after_tool_call`。

**否决方案：decide decision 增加 Modify 变体。** 方便但破坏上述保证；被拒绝（用户已确认）。

### 3. 授权元数据与 ToolSpec 的获取责任在 kernel 一侧

`registry.authorization(name)` 与 `registry.specs()` 的查询发生在 Hook 之前，结果以借用形式进入 Hook 输入。理由：Hook 层是内部接口，参数可以丰富；授权元数据是审批 Handler 决定"要不要问人"的必要上下文；`ToolSpec`（description、input schema）供审批界面等展示方使用。

缺失工具的 lookup 失败维持现有行为（结构化错误结果），不进入任何 Tool Hook——runtime 没有该 Tool 的授权事实，Hook 无从决策。

**否决方案：Hook 输入只给 `ToolName`，元数据由 Handler 自查 registry。** 每个 Handler 重复注入 registry 依赖，且查询时机不受 kernel 控制，破坏单一编排边界。

### 4. deadline 按 HookPoint 配置，decide 默认无 deadline

`LoopLimits` 从单一 `hook_timeout: Duration` 改为按点配置（transform / decide / after / prepare 等），`HookControl` 形状不变，kernel 按点填入绝对 deadline。`decide_tool_call` 默认 `None`：不构造 deadline，只受 `CancellationToken` 约束——人肉审批的合理等待是分钟到小时级，任何兜底超时会误伤正常审批；恶意或僵死的审批 Handler 由取消（用户取消 turn）终止。其余 Hook 点保留有默认值的超时，维持 fail-closed。

**否决方案：单一全局 timeout 调大到小时级。** 所有 Hook 失去 fail-closed 保护，因噎废食。

**否决方案：Handler 自报所需 deadline。** kernel 无法验证自报值的合理性，统一强制合同出现窟窿。

### 5. 审批耐久事件退役，恢复语义为 fail-safe 重复提示

新 AgentLoop 路径不再提交 `ToolApprovalRequested`/`ToolApprovalResolved`。崩溃发生在审批通过后、`ToolExecutionStarted` 之前时，恢复后 decide Hook 重新执行 → 审批 Handler 再次问人。方向是 fail-safe 的：绝不出现"未重新确认就执行"。去重（复用已保存 decision）与审计由 H3 的 hook journal（Pending → Completed）承载，本 change 只为 journal 预留语义位置，不实现。

**否决方案：过渡期保留审批耐久事件，由 kernel 为"审批类 Handler"特殊提交。** kernel 需要重新识别哪个 Handler 是审批，策略知识回流内核，且 H3 journal 落地后还要再删一次。

### 6. 拆名是 beta 期前向破坏性变更

`HookRuntime` trait 从四个方法变为五个（`before_tool_call` 拆为两个）；`BeforeToolCallInput/Decision` 类型移除，新增两对输入/decision 类型；`HookPoint`（stratum-core）需要对应的新点标识。仓库内所有实现（No-op、测试 recording runtime）与调用方前向切换，不保留兼容层。

## Risks / Trade-offs

- **[风险] 审批 Handler 僵死且无 deadline，turn 只能等用户取消。** → 有意取舍：人肉等待本就没有正确超时值；组合侧（Web/CLI）负责提供取消入口，与现有 turn 取消同一通道。
- **[风险] 崩溃后重复审批提示影响体验。** → 方向 fail-safe，H3 journal 落地后消除；已在 spec 中明确为当前语义，避免被误当 bug 修掉。
- **[风险] `ToolHookTarget` 携带 `ToolSpec` 使 Hook 输入变大，激励 Handler 滥用 spec 做决策。** → 输入仍是借用视图，无分配成本；在 crate 文档中声明 spec 仅供展示与上下文参考，决策依据应是授权元数据。
- **[风险] 拆名使 H1 刚发布的合同立刻过时，下游（本仓库外暂无）需要迁移。** → alpha/beta 阶段接受前向破坏；change 内一次性改完，不留双轨。
- **[权衡] 审批交互的可观测性离开耐久事件流。** → Web 前端目前消费 `ToolApprovalRequested` 事件渲染审批 UI；新 kernel 路径尚未接入 Web（H1 范围外），legacy 路径事件不变，无产品面回归。未来新 kernel 接入 Web 时，审批 UI 数据源切换为审批 Handler 的私有通道协议，届时单独设计。

## Migration Plan

1. 在 `hook_runtime` 中拆名并新增 decide 合同，富化三个 Tool Hook 输入，No-op 与公共导出同步切换。
2. `LoopLimits` 改为按点 timeout 配置，执行 helper 支持"无 deadline 仅取消"。
3. `AgentLoop` 重排 Tool 调用编排：lookup/原始校验 → transform → 复验 → decide → started → call。
4. `ToolExecutor` 删除审批参数与审批路径（构造签名变更），保留机制职责。
5. 测试前向切换：H1 的 hook 测试矩阵映射到两个新点；新增"decide 看到最终参数""decide 无 deadline 仅取消""审批 Handler 端到端（模拟问人通道）"测试。
6. fmt、clippy、workspace tests 全量通过；更新 `TODO.md`（H2 相关条目）与 `crates/stratum-agent/AGENTS.md`。

alpha 阶段前向破坏：不设计迁移、降级或回滚路径。验证失败则修正本 change，不保留新旧审批双轨。

## Open Questions

- 审批 Handler 的标准组合实现（Web 通道协议）长什么样？→ 留给新 kernel 接入 Web 时的独立 change，本 change 只保证 kernel 侧无审批概念。
