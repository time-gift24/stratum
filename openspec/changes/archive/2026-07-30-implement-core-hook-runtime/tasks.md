## 1. Hook Runtime 公共合同

- [x] 1.1 在 `stratum-agent` 内新增职责分离的 `hook_runtime` 模块，分别放置接口、错误和 No-op 实现，并从 crate 公共 API 导出；本阶段不新增独立 crate 或外部依赖
- [x] 1.2 定义四个 Hook 的借用输入、类型化 decision、`HookControl` 和 `HookRuntime` 异步接口，为公共 API 补齐文档、常用 trait 与非法输出约束
- [x] 1.3 增加 `LoopCompletionReason`、类型化 Hook 错误映射和带默认值的 Hook timeout，并将仓库内 `LoopOutcome` 调用方前向切换到新的结束原因

## 2. AgentLoop 接入

- [x] 2.1 为 `AgentLoopBuilder` 增加 `Arc<dyn HookRuntime>` 注入能力和默认 `NoopHookRuntime`，验证未配置 Hook 时现有请求、事件、Tool 调用、消息与终态不变
- [x] 2.2 在每次模型请求前接入 `transform_context`，从 committed context 与待消费 Inject 构造 request-only view，保证替代 context 不写回历史或 outcome
- [x] 2.3 在 Tool 审批和执行前接入 `before_tool_call`，支持 Continue、仅修改 arguments 和生成固定 `hook_blocked` 结果的 Block，同时保留原 `CallId` 与 Tool name
- [x] 2.4 在模型可见 Tool result 提交前接入 `after_tool_call`，同时覆盖真实 Tool result 与 Block result，并在替换结果时保留 Tool role 和原 `CallId`
- [x] 2.5 在全部 Tool result 耐久提交后接入 `prepare_next_turn`，实现 Continue、`HookStopped` 和只供下一次请求消费一次的合法 User message Inject
- [x] 2.6 用统一执行 helper 为四个 Hook 强制调用前取消、执行中取消、绝对 deadline、类型化失败和非法 decision 校验，确保错误和事件不泄露 Hook 输入或内部错误正文

## 3. 行为与完整流程测试

- [x] 3.1 增加 recording Hook Runtime 测试辅助实现，覆盖默认 No-op 等价性、自定义 Runtime 注入和四个 Hook 的精确调用顺序
- [x] 3.2 分别覆盖 transform 的 Unchanged/Replace、before 的 Continue/Modify/Block、after 的 Keep/ReplaceResult、prepare 的 Continue/Stop/Inject，并断言所有身份与 request-only 边界
- [x] 3.3 为四个 Hook 各自覆盖正常返回、Runtime 失败、timeout、调用前取消和执行中取消矩阵，断言受影响的模型、Tool、message 或 iteration action 均 fail closed
- [x] 3.4 增加非法 Block reason 和 Inject payload 测试，以及非 `tool_calls` finish reason 不进入 Tool Hook、Block 不审批/不执行、after 失败不提交结果、Inject 只消费一次且不进入历史的回归测试
- [x] 3.5 使用 mock model、recording Hook Runtime、真实 AgentLoop 与测试 ToolExecutor 完成一条端到端内核流程：context 变换 → 参数修改 → Tool 执行 → 结果替换 → 下一轮注入 → Hook 停止，并核对模型请求、耐久事件、消息、调用顺序和最终 completion

## 4. 文档、质量门禁与归档准备

- [x] 4.1 更新 `TODO.md` 的 H1 描述，将接入点明确为 `AgentLoopBuilder`，并只在对应实现与测试完成后勾选 H1 条目
- [x] 4.2 按仓库要求将最终 Hook Runtime 模块职责、不变量和明确延期项归档到 `crates/stratum-agent/AGENTS.md`
- [x] 4.3 运行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets` 和 `cargo test --workspace --all-targets`，修复本 change 引入的失败
- [x] 4.4 运行 `openspec validate implement-core-hook-runtime --type change --strict --no-interactive` 与 `openspec validate --all --strict`，确认不存在阻止归档或污染 canonical specs 的无效、冲突或陈旧 change
