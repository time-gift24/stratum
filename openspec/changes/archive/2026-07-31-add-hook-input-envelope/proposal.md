## Why

H1/H2 冻结的五个 Hook 输入遵循"最小够用"原则，但实践已两次撞到天花板：`after_tool_call` 缺少对话历史，做不了内容感知的结果级压缩；所有 Hook 缺少 `TokenUsage`，做不了预算触发。Hook 层是内部信任层，读侧应当环境化——与其每次撞墙补一个字段，不如一次性建立公共信封，让未来扩展只改一处。这是破坏性合同修订，必须赶在 H3 冻结 journal 输入摘要、S1 开始编写 Skill handler、S2 冻结 wire protocol 之前完成。

## What Changes

- **BREAKING** 新增借用公共信封 `HookSnapshot`（`iteration`、`&LoopContext`、`Option<TokenUsage>`），嵌入全部五个 Hook 的输入结构，替代各输入中零散的 `iteration` / `context` 字段。
- 逐点钉死 `snapshot.context` 语义：该 Hook 边界时刻的 committed context；`transform_context` 含待消费 Inject；`after_tool_call` 不含当前未提交的 result。
- `snapshot.usage` 为本次 run 截至该边界累计的 `TokenUsage`；provider 未上报时为 `None`。
- `after_tool_call` 经由信封获得完整对话历史，结果级压缩等内容感知决策成为可能。
- 保持宽读窄写：五个 decision 词汇、写回语义、身份与配对不变量全部不变，公共信息只读。
- 非目标：新增任何 Hook 点或 decision 变体（上下文压缩属于 H5）；改变 journal/wire 协议（H3/S2）；工具列表进信封（待 S1 评估后再定）；legacy Agent。

## Capabilities

### New Capabilities

无。

### Modified Capabilities

- `agent-hook-runtime`: 五个 Hook 输入统一嵌入 `HookSnapshot` 公共信封；`after_tool_call` 获得完整历史；快照逐边界的 context/usage 语义冻结。

## Impact

- 主要影响 `crates/stratum-agent`：`hook_runtime` 输入结构、No-op、`agent_loop/runner.rs` 的快照构造与 usage 累计、公共导出与全部 hook 测试。
- `stratum-core::TokenUsage` 复用，不新增类型或依赖。
- 仓库内 `HookRuntime` 实现方（No-op、测试 recording runtime）前向切换；alpha 阶段不保留兼容层。
- 为 H3（统一 input digest）、S1（handler 编写）、H5（压缩触发依据）定型输入形状。
