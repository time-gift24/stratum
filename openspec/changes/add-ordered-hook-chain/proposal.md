## Why

H1 起 `HookRuntime` 一直是"单一已组合边界"的临时形态：多个策略 Handler 的顺序、短路语义和版本固定都藏在 runtime 实现内部，kernel 不可见。M0 的"三个 Handler 三个 invocation 身份"模型和 H3b 的 per-handler journal 都在等这个形态收口。同时工具参数校验目前是每个 Tool 自行实现的 ad-hoc 逻辑，`ToolSpec.input_schema` 声明了 schema 却没有统一的执行边界——Hook 链改完参数后，必须有一个权威校验保证"审批所见即所执行"。

## What Changes

- 新增 `HookHandler` trait：与五个 Hook 点同形的方法签名，默认 No-op 实现，Handler 只实现自己关心的点；每个 Handler 暴露不可变版本身份（`HookHandlerVersionId`）。
- 新增 `ChainHookRuntime`：持有**有序** `Vec<Arc<dyn HookHandler>>` 并实现 `HookRuntime`，作为链式 Runner。语义按点定义：
  - `transform_context` / `transform_tool_call` / `after_tool_call`：顺序变换——前一个 Handler 的输出视图是后一个的输入；
  - `decide_tool_call`：顺序执行，第一个 `Block` 短路，不再调用后续 Handler；
  - `prepare_next_turn`：`Stop` 短路；多个 `Inject` 的消息按 Handler 顺序合并；全部 Continue 则 Continue。
- 链的最终参数复验在整条 transform 链完成后执行（现状是单个 runtime 调用后），验收语义不变。
- ExtensionSet 顺序固化：链构造时按声明顺序计算 `ExtensionSetVersionId`（有序 Handler 版本摘要），随 `LoopStarted` 耐久提交；resume 时重放链版本必须匹配，不一致 fail closed——重启前后处理器顺序一致。
- `stratum-tools` 建立统一参数校验边界：以 `ToolSpec.input_schema` 为权威的 JSON Schema 校验（引入 `jsonschema` 依赖，workspace 统一管理），`BuiltinToolRegistry::validate` 切换到该边界；`Tool::validate` 保留为 schema 之外的工具自定义语义校验。
- **BREAKING**：`LoopStarted` 事件新增可选 `extension_set_version_id` 字段（serde default，旧日志兼容）；`BuiltinToolRegistry::validate` 的拒绝集合随 schema 校验变化。
- 非目标：per-handler 粒度 journal（H3b 评估项，链在 kernel 看来仍是一次 hook-point 调用）；远程/脚本 Handler（S2）；ExtensionSet 的持久化与分发；legacy Agent；Web 配置界面。

## Capabilities

### New Capabilities

- `tool-input-validation`: 以 `ToolSpec.input_schema` 为权威的统一 JSON Schema 校验边界及其在工具调用管线的位置。

### Modified Capabilities

- `agent-hook-runtime`: 单 Runtime 边界下新增有序 Handler 链语义（顺序变换、Block/Stop 短路、Inject 合并）、链版本固定与 resume 校验。

## Impact

- 主要影响 `crates/stratum-agent`（`hook_runtime` 新增 handler/chain 模块）、`crates/stratum-tools`（schema 校验边界）、`crates/stratum-core`（`LoopStarted` 事件字段）。
- 新增依赖 `jsonschema`（workspace 继承），理由：schema 驱动的统一校验是 H2 验收条件，无既有依赖可复用。
- journal 粒度保持 hook-point 级：链中途崩溃会重试整链，该权衡写入 H3b 评估项。
- 既有 `HookRuntime` 注入方（No-op、测试 recording runtime、自定义 runtime）不受影响：链只是 `HookRuntime` 的一个新实现。
