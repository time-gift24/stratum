## 1. Hook 合同拆分与富化

- [x] 1.1 在 `stratum-core` 为 `HookPoint` 增加 `transform_tool_call` 与 `decide_tool_call` 点标识，移除或替换 `before_tool_call` 点
- [x] 1.2 在 `hook_runtime` 中删除 `before_tool_call` 合同，新增 `transform_tool_call`（Continue / ModifyArguments）与 `decide_tool_call`（Execute / Block）两对借用输入与 owned decision，含非法输出校验（Block reason 非空；transform 不得携带 Block；decide 不得携带 Modify）
- [x] 1.3 新增 `ToolHookTarget` 借用视图（`ToolKind`、`DangerLevel`、`ToolSpec`），接入 `transform_tool_call`、`decide_tool_call`、`after_tool_call` 的输入，并补齐公共 API 文档与常用 trait
- [x] 1.4 `LoopLimits` 从单一 `hook_timeout` 改为按 HookPoint 配置（`#[must_use]` builder 方法），`decide_tool_call` 默认无 deadline；执行 helper 支持"无 deadline 仅取消"路径
- [x] 1.5 `NoopHookRuntime` 与 crate 公共导出前向切换到五个 Hook 方法

## 2. AgentLoop 编排重排

- [x] 2.1 Tool 调用流程重排为：lookup 与授权元数据 → 原始参数校验 → `transform_tool_call` → 最终参数复验 → `decide_tool_call` → `ToolExecutionStarted` → 调用；保证 decide 输入一定是复验后的最终参数
- [x] 2.2 decide 的 Block 生成固定 `hook_blocked` 模型可见结果且不提交 `ToolExecutionStarted`，该结果仍经过 `after_tool_call`
- [x] 2.3 缺失工具与非 `tool_calls` finish reason 维持现有错误结果行为，不进入三个 Tool Hook
- [x] 2.4 按 HookPoint 从 `LoopLimits` 取 deadline 填入 `HookControl`，decide 默认只传取消信号

## 3. ToolExecutor 移除审批

- [x] 3.1 从 `ToolExecutor` 删除 `ToolApproval` 构造参数、审批交互路径与 `ToolApprovalRequested` / `ToolApprovalResolved` 提交；`ToolExecutor::new` 构造签名变更
- [x] 3.2 从 crate 公共 API 移除 `ToolApproval`、`ToolApprovalRequest`、`AllowAllToolApproval` 等审批导出，清理 `tool_executor` 模块结构与错误类型中仅服务于审批的部分
- [x] 3.3 确认 legacy Agent（`loop.rs` 的 `ActiveApprovalGuard` 路径）不经过 `ToolApproval`，其审批行为保持不变

## 4. 测试

- [x] 4.1 recording Hook Runtime 与既有 hook 测试前向切换到两个新点，覆盖五个 Hook 的精确调用顺序与 No-op 等价性
- [x] 4.2 transform 分支测试：Continue / ModifyArguments / 复验拦截非法变换结果 / 非法 decision
- [x] 4.3 decide 分支测试：Execute / Block（含 `hook_blocked` 结果与 after 覆盖）/ decide 输入为最终参数 / 非法 decision
- [x] 4.4 富化输入测试：三个 Tool Hook 收到的授权元数据与 `ToolSpec` 和注册表一致；缺失工具不进 Hook
- [x] 4.5 deadline 矩阵：各点超时 fail-closed；decide 默认无 deadline 时长时间等待只在取消时退出
- [x] 4.6 模拟审批 Handler 端到端：批准 → 执行；拒绝 → `hook_blocked`；等待中取消 → loop cancellation；且全链路无 `ToolApprovalRequested` / `ToolApprovalResolved` 耐久事件
- [x] 4.7 ToolExecutor 机制测试更新：构造签名变更后 lookup、校验、started、调用行为不变

## 5. 文档、质量门禁与校验

- [x] 5.1 更新 `TODO.md`：H2 中审批相关条目对齐 decide 相位方案（含"审批所见即所执行"已由相位顺序保证的说明）
- [x] 5.2 更新 `crates/stratum-agent/AGENTS.md`：五个 Hook 点、相位顺序、decide 无默认 deadline、审批 Handler 化与 executor 纯机制职责
- [x] 5.3 运行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`，修复本 change 引入的失败
- [x] 5.4 运行 `openspec validate hookify-tool-approval --type change --strict --no-interactive` 与 `openspec validate --all --strict`
