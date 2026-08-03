# Stratum Agent Kernel

Stratum agent 内核的领域语言：一个确定性 kernel 驱动模型/工具迭代，handler 在受控 Hook 点表达意图，事件流是唯一真相。

## 第一性模型

- **确定性 kernel**：给定事件流 + journal，当前状态（committed context、迭代号）完全可重建。
- **journal 固化非确定性**：一切非确定性（LLM 调用、handler 决策）被隔离在 Hook 点，决策记入 journal，崩溃后 replay 用 journal 替代重新调用。
- **事件流唯一真相**：append-only `DurableAgentEvent` 是唯一真相来源；任何派生物（如 `compact.jsonl`）只加速、不承载真相。

## Language

**Committed context**:
从事件流重建的持久对话基线（system prompt + 全部已提交消息）。只能被 kernel 提交的 durable 事件（`MessageAppended`、`TranscriptCompacted`）改写，handler 永远不能直接写。
_Avoid_: history、transcript（作"基线"义时）

**Request view**:
每次模型请求前由 kernel 现场拼装、用完即弃的请求体 = committed context + 本轮 ContextPatch + 一次性 Inject。不落盘、不回写、不进 `LoopOutcome.new_messages`。
_Avoid_: request context、当前上下文

**ContextPatch**:
handler 在 `transform_context` 返回的增量修改（`ReplaceSystemPrompt` / `DropHistory { upto }` / `RewriteHistory { upto, summary }`），只作用于本轮 request view。无论多少轮 patch，committed 基线永不收缩——这是 H5 durable 压缩存在的理由。
_Avoid_: context 修改、历史编辑

**迭代边界 (iteration boundary)**:
一个迭代内全部 tool cycle 的模型可见结果都耐久提交之后、下一次模型请求之前的唯一点。kernel 在此提交 `IterationCompleted`，也是 `prepare_next_turn` 的调用点。durable 压缩只允许发生在此：此时 tool_call/result 配对完整、已完成迭代的 journal 记录全部安全、崩溃窗口良定义。handler 不能在迭代中途请求压缩；紧急的结果级瘦身走 `ReplaceResult`。
_Avoid_: turn 边界、轮次结束

**ReplaceResult**:
`after_tool_call` 的 decision 变体：tool 结果提交前，用同一 CallId、同一 tool 角色替换 JSON body，原结果永不进入 committed。是预防性瘦身（臃肿不进入基线），与补救性的 `TranscriptCompacted`（事后改写基线）互补。
_Avoid_: 结果压缩、结果截断

**Journal**:
handler 决策的耐久记录，以 `HookInvocationPending` / `Completed` / `Failed` 事件变体住在事件流内部——没有第二个耐久边界，只有一个 fsync 顺序故事。replay 时用已 Completed 的记录替代重新调用 handler，非确定性（含 LLM 摘要）因此被固化，不二次生成。
_Avoid_: 日志、审计日志

**Hook 地址 (HookAddress)**:
journal 记录的结构性键：`(iteration, HookPoint, Option<CallId>)`。迭代级 Hook（transform_context、prepare_next_turn）由二元组唯一标识；tool 级 Hook 在同一迭代内可触发多次，靠 CallId 区分。地址与消息内容/下标无关，因此在压缩改写消息后全部保持有效。
_Avoid_: 消息下标寻址、hook id

**Inject**:
`prepare_next_turn` 的 decision 变体：只加入下一轮 request view、只消费一次的额外 User 消息。不落盘、不进 history、不进 `new_messages`。当前无真实 handler 使用，是预留能力。链式执行中被先到的 `Stop`/`Compact` 短路时静默丢弃——已确认的现状，有真实需求再考虑组合变体。
_Avoid_: 消息注入、上下文注入

**短路 (short-circuit)**:
链式 Hook 执行中，`Stop` 与 `Compact` 是平级终局 decision：任一 handler 返回即定案，后续 handler 不再被调用，已收集的 Inject 一并丢弃。handler 顺序因此是语义的一部分。
_Avoid_: 中断、提前返回

**崩溃窗口回放**:
journal 里 `Completed` 的 decision 永远回放其记录、永不重调 handler，即使其效果只部分落盘。压缩有两个窗口：W1（decision 已记、压缩事件未提交）用 journal 的 summary 直接执行压缩；W2（压缩事件已提交、迭代边界未提交）replay 从事件流应用压缩并经 `compacted_iterations` 跳过重复执行。LLM 摘要因此绝不二次生成。
_Avoid_: 重试、恢复

**Tool at-least-once**:
`ToolExecutionStarted` 已提交而结果未提交的调用，resume 时原样重执行。exactly-once 需要工具侧幂等，超出 kernel 职责；副作用工具的防护归组合方。
_Avoid_: exactly-once、幂等执行

**压缩检查点 (compact.jsonl)**:
filesystem 后端的派生索引，加速 resume，可由事件流完全重建——缺失、损坏、校验不匹配一律回退全量重放，**永不 fail closed**（fail closed 只留给真相流本身）。三条不变量：①边界后写——检查点在该次压缩的 `IterationCompleted` 落盘后才追加，故"压缩已提交而边界未提交"的窗口不存在检查点；②窗口自足——`window_start_line` 指向第一条保留消息的物理行，窗口自带保留后缀、prepare journal 记录、压缩事件与边界；③三项校验——窗口内必须找到 iteration/upto/digest 与检查点一致的 `TranscriptCompacted`。
_Avoid_: 压缩历史、快照（它不是真相）

**Replay 双模式**:
replay 应用 `TranscriptCompacted` 时：`upto <= 当前已重建长度`为全量流绝对 splice；`upto` 超出则为检查点窗口模式（已重建消息即保留后缀，summary 前置到 index 0）。窗口合法性由组合方在存储边界校验，kernel 对两种模式一视同仁。
_Avoid_: 增量重放、部分重放

**机制/策略分离**:
kernel 只提供机制（校验并执行 `Compact`），零策略：何时压缩由 handler 凭 `HookSnapshot.usage`（最近一次模型上报的用量）自决，摘要由 handler 自己调 LLM 生成。kernel 不内置摘要器、不制定触发阈值。策略迭代因此永远不需要改 kernel。
_Avoid_: 自动压缩、智能压缩

**压缩标记消息 (compaction marker)**:
压缩后替换 committed 前缀的 kernel 署名 system 消息：稳定前缀 `[stratum:transcript-compacted]` + 换行 + handler 摘要正文。无 tool 身份、无 reasoning content。前缀稳定是为了让 handler 在后续 Hook 快照里识别"已压缩"，也让 provider 兼容性可在测试中锁定。
_Avoid_: 摘要消息、压缩占位符
