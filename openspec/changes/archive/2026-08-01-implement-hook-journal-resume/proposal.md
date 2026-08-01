## Why

M0 冻结了 Hook journal 的身份与恢复合同，H1/H2 让 Hook 真正运行起来，但新 `AgentLoop` 内核至今没有生产级耐久后端（只有 legacy 的 EventBus 适配器与测试 mock），崩溃后无法恢复：循环状态要从头重跑，审批等一次性决定会被重新询问，恢复后执行路径可能与崩溃前分叉。H3a 交付最短纵向切片：filesystem 耐久后端 + AgentLoop resume + hook-point 级 journal，使"世界可重建、决定可回放"成为事实。

## What Changes

- 新增 filesystem 生产级 `DurableEventSink` 后端与事件读取器：per-run 目录、JSONL 追加写、原子落盘。
- **BREAKING** `TransformContextDecision::Replace { context }` 改为 `Patch(ContextPatch)`（`ReplaceSystemPrompt` / `DropHistory { upto }` / `RewriteHistory { upto, summary }`）：handler 不再能提交整个替代 context 制造"影子历史"，只能提交增量操作；kernel 校验 patch 不越界、不切断 tool_call/result 配对，非法判 `InvalidOutput`。decision 载荷恒为小数据，journal 不需要 blob 机制。
- `DurableAgentEvent` 新增三个 journal 变体：`HookInvocationPending` / `HookInvocationCompleted` / `HookInvocationFailed`，journal 合并进唯一耐久流，不新增第二 sink。
- kernel 在五个 Hook 点周围写入 journal：调用 runtime 前提交 `Pending`；应用 decision 前提交 `Completed`；类型化失败提交 `Failed`。粒度为 hook-point 级（一次 Hook 调用 = 一条 invocation record）。
- AgentLoop resume 最短切片：从事件流重建 committed context、迭代前沿与 tool 结果对账（result 必须是前序 assistant `tool_calls` 的精确有序前缀，缺失后缀重新执行）；终态（Finished/Failed/Cancelled）不可恢复。
- resume 时复用 digest 匹配的 `Completed` decision（不重新调用 runtime）；`Pending` 以原 invocation 身份重试（不创建第二个逻辑 invocation）；`Failed` 重现类型化失败；地址或 digest 不匹配 fail closed。
- **BREAKING** `HookSnapshot.usage` 语义从"run 级累计"改为"最近一次模型响应上报的 usage"（当前 context 规模的直接信号，供未来 compact 触发）；累计需求由 handler 自行承担。`IterationCompleted`/`LoopFinished` 等事件的 usage 语义同步修正。
- input digest 采用载荷级：Tool Hook 对 canonical `ToolCall` 做 sha256；`transform_context`/`prepare_next_turn` 以地址本身为 digest；usage 与完整历史不参与（context 完整性由 resume 重建确定性保证）。
- 非目标：per-handler 粒度 journal（H2 链）、sqlite per-session 后端（H3b）、远程 Handler 幂等键传递（S2/R3）、上下文压缩（H5）、P1 完整存储合同、legacy Agent、Web/API 组合接入。

## Capabilities

### New Capabilities

- `agent-loop-resume`: filesystem 耐久后端、事件重放、AgentLoop resume 前沿与 tool 对账、终态与恢复边界。

### Modified Capabilities

- `agent-hook-runtime`: 五个 Hook 点的 journal 写入与 resume 复用语义；`HookSnapshot.usage` 改为最近一次响应上报值；载荷级 input digest。

## Impact

- 主要影响 `crates/stratum-agent`（resume 重建、journal 写入点、usage 语义）、`crates/stratum-infra`（filesystem sink 与读取器）、`crates/stratum-core`（三个 journal 事件变体、decision 记录的 serde 表示）。
- `HookSnapshot.usage` 语义变化影响 H2.5 刚定型的合同；handler 作者需知累计不再是 kernel 职责。
- M0 的 `hook-execution-baseline` 要求不变，本 change 是其 hook-point 粒度实现；per-handler 场景在 H2 链落地后扩展。
- resume 需要组合方重新提供 system prompt 与 run 身份；kernel 保持 session-independent，不感知 Session/Turn/Store。
