## Why

长对话的上下文膨胀是 agent 产品的核心约束。H1–H3a 已经把前置全部铺好：`HookSnapshot.usage` 携带最近一次模型上报（当前 context 规模的直接信号）、`ContextPatch` 提供 request-only 的轻量摘要、journal 能回放非确定性决定。但 request-only 的 patch 永远不收缩 committed 基线——历史无限增长，每轮重放一遍隐藏逻辑，崩溃后全部丢失。H5 补上最后一块：handler 表达压缩意图，kernel 在迭代边界执行配对安全的 **durable 基线改写**，resume 直接从压缩基线恢复。

## What Changes

- **BREAKING** `PrepareNextTurnDecision` 新增 `Compact { upto, summary }` 变体：handler 携带自己生成的摘要（LLM 调用是 handler 的事），kernel 不引入摘要组件、不制定触发策略（handler 用 `snapshot.usage` 自行判断）。
- kernel 在迭代边界执行压缩：校验 `upto`（0-based 左闭右开、不切断 tool_call/result 配对、不切入当前迭代的消息），用 kernel 归属的 system 标记消息替换 committed 前缀，并耐久提交新的 `TranscriptCompacted` 事件；随后正常提交迭代边界。
- `DurableAgentEvent` 新增 `TranscriptCompacted` 变体（additive）。事件日志保留全部原始消息（审计不丢）；重建视图应用压缩。
- resume：重放应用 `TranscriptCompacted` 得到压缩基线；崩溃于 prepare Completed 与压缩提交之间时，从 journal 回放 decision 完成压缩——摘要的非确定性（LLM 生成）由 journal 固化，不会二次生成。
- filesystem 后端新增派生检查点索引 `compact.jsonl`：每次压缩落盘后追加一行（迭代号、事件行号、摘要 digest），resume 从最近检查点开始重放；索引永远可由事件流重建，损坏或缺失回退全量重放，不承担真相职责。
- 多次压缩按序应用；journal 地址（iteration/point）与载荷级 digest 不受消息改写影响，压缩前后保持一致。
- 非目标：自动触发策略（阈值归组合方/handler）；kernel 内置 LLM 摘要器；结果级压缩（已由 `after_tool_call::ReplaceResult` 覆盖）；事件日志的物理清理（H3b）；UI 压缩标记渲染（Web 侧后续）。

## Capabilities

### New Capabilities

- `context-compaction`: 迭代边界的 durable 上下文压缩——意图 decision、kernel 执行的不变量、`TranscriptCompacted` 事件与重放语义。

### Modified Capabilities

- `agent-hook-runtime`: `prepare_next_turn` 决策词汇新增 `Compact`；压缩期间各 Hook 点的行为约束。
- `agent-loop-resume`: 事件流重放应用 `TranscriptCompacted`；压缩窗口的崩溃恢复。

## Impact

- 主要影响 `crates/stratum-agent`（decision 变体、压缩执行与校验、重放）与 `crates/stratum-core`（新事件变体、decision record 形状）。
- `HookDecisionRecord` 增加 Compact 表示；journal 对已 Completed 的 Compact 回放即应用压缩。
- legacy Agent、Web、API 不变；压缩标记消息进入历史后对 provider 的兼容性（system 角色消息）在测试中固化。
