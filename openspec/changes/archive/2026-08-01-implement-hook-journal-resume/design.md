## Context

M0 在 `stratum-core` 冻结了 `HookInvocationId`、`HookInputDigest`、`HookInvocationAddress`、`HookInvocationRecord<T>`、`HookResume::{Retry, Reuse}` 与 `hook-execution-baseline` spec，但该 spec 是 Session/Turn 形状的。H1/H2 把 Hook 变成了 `AgentLoop` 的真实控制流边界，同时刻意保持 kernel session-independent：它不知道 Session、Turn、Store。现状的硬缺口：

1. 新内核没有生产级 `DurableEventSink`——唯一实现是 legacy 的 `ScopedAgentEventSink`（EventBus 适配器），其余都是测试 mock。
2. 没有 resume：崩溃后循环状态无法重建，一次性决定（人的审批、外部策略、LLM 摘要）无法回放。
3. `HookSnapshot.usage`（H2.5）定义为 run 级累计值，讨论确认这是错误抽象：provider 每次调用都上报本次 usage，最新一次的 `input_tokens` 就是当前 context 规模——compact 触发要的信号；累计是账单视角，应由 handler 自负。

事实核查：`DurableAgentEvent` 现有变体为 `LoopStarted`、`MessageAppended`、`ToolApprovalRequested/Resolved`（legacy 用）、`ToolExecutionStarted`、`IterationCompleted`、`LoopFinished/Failed/Cancelled`；后三者的 usage 字段语义均为"累计"，需随修正一并处理。crate `AGENTS.md` 已规定"builder 不得接受第二个可能分裂耐久边界的 sink"。

## Goals / Non-Goals

**Goals:**

- 新内核第一个生产级耐久后端：filesystem per-run 目录。
- AgentLoop resume 最短切片：事件重放 → context/前沿/对账 → 续跑。
- hook-point 级 journal 合并进唯一耐久流，写入点、复用、重试、fail-closed 语义符合 M0。
- usage 语义修正为"最近一次响应上报"。

**Non-Goals:**

- per-handler 粒度 journal（等 H2 链）、sqlite（H3b）、远程 Handler 幂等键透传（S2/R3）、compact（H5）、P1 完整存储抽象、legacy Agent、任何 Web/API 组合接入与跨进程调度。

## Decisions

### 1. Journal 合并进 DurableEventSink，不新增第二 sink

`DurableAgentEvent` 新增三个变体：

```text
HookInvocationPending   { invocation_id, point, address, input_digest }
HookInvocationCompleted { invocation_id, decision }
HookInvocationFailed    { invocation_id, failure }
```

理由：journal 的 Completed 必须先于受影响动作的 durable（decide→started、after→result commit、prepare→iteration boundary），两个 sink 在崩溃面前无法保证这个时序；crate `AGENTS.md` 已有"单一 sink"原则。M0 的分离要求（不是对话历史、不从 EventBus 重建）在逻辑视图层面满足。

**否决方案：独立 HookJournal sink（trait 注入）。** 正是被否决过的"第二 sink 分裂耐久边界"。

### 2. 地址是 kernel 级最小形状，Session/Turn 绑定交给存储作用域

kernel 不感知 Session/Turn，invocation 地址 = `(iteration, HookPoint, Option<CallId>)`：tool 类 Hook 以 `CallId` 区分同迭代的多个调用，`transform_context`/`prepare_next_turn` 以 `(iteration, point)` 唯一确定。Session/Turn 归属由组合方通过存储作用域表达（filesystem 的 per-run 目录）。M0 的 `HookInvocationAddress` 不在 kernel 复用，避免把 Session/Turn 概念漏进内核。

### 3. 载荷级 digest，usage 与历史不参与

- Tool Hook：digest = sha256(canonical JSON of `ToolCall`)。
- `transform_context` / `prepare_next_turn`：digest = 地址本身（无专属载荷）。
- context 完整性由 resume 重建确定性免费保证（同一份事件流重放出字节相同的 context），不重复哈希；usage 是易变观测值，不参与。

**否决方案：对完整 request view 做 digest。** 每次 Hook 调用哈希整个对话历史，成本随对话长度线性增长，且不提供超出重建确定性之外的保证。

### 4. usage 语义修正：最近一次响应上报

`HookSnapshot.usage: Option<TokenUsage>` = 最近一次模型响应上报的 usage（尚无上报为 `None`）。kernel 只透传不累计。`IterationCompleted`、`LoopFinished`、`LoopFailed`、`LoopCancelled` 的 usage 字段语义同步从"累计"改为"最近一次上报"（alpha 前向修正，不改字段名）。transform_context 在某次模型请求前运行时，usage 是**上一轮**响应的值——这正是"当前 context 多大"的正确读数。

### 5. Resume 重建：组合方喂事件流，kernel 定前沿

组合方（filesystem 读取器）读出 run 的事件流交给 kernel 的 resume 入口，并重新提供 system prompt 与 run 配置（这些不属于事件流）。kernel 重放：

- `MessageAppended` 序列 → committed context（含 tool 结果对账：committed tool result 必须是紧邻前序 assistant `tool_calls` 的精确有序前缀；未知、重复、稀疏、乱序视为损坏的恢复历史，fail closed；缺失后缀重新执行）。
- 最大 `IterationCompleted{N}` → 迭代前沿；从 N+1 续跑。
- `LoopFinished/Failed/Cancelled` → 终态，拒绝 resume。
- journal 事件 → 恢复期间的 Hook 调用查表：digest 匹配的 `Completed` 复用 decision；`Pending` 以原 invocation 身份重试（更新同一 record，不新建）；`Failed` 重现类型化失败；地址/digest 不匹配 fail closed。

Tool 执行是 at-least-once 的既有立场不变：`ToolExecutionStarted` 后崩溃的 call 按未知结果处理，重跑该 tool（tool 实现需幂等，与 legacy 约定一致）。

### 6. Transform Context 用 patch 表达，journal 不需要 blob

`TransformContextDecision::Replace { context }` 允许 handler 提交整个替代 context，制造与 committed transcript 分叉的"影子历史"：分叉越大，真实状态越住在 journal 而非事件流里，resume 语义被倒置。实际场景盘点（compact、system prompt 刷新、历史裁剪、脱敏）全都只需要增量操作，因此改为：

```text
TransformContextDecision = Unchanged | Patch(ContextPatch)
ContextPatch = ReplaceSystemPrompt(String)
             | DropHistory { upto: usize }
             | RewriteHistory { upto: usize, summary: ChatMessage }
```

kernel 应用 patch 到 request view（仍不写回 committed），并校验：`upto` 不越界、落在消息边界、不切断 tool_call/result 配对，非法判 `InvalidOutput`。patch 是 request-only 的视图调整：被 Drop 的历史仍在事件流中，每个迭代由 handler 重新决定；产品级持久压缩属于 H5 的 kernel durable 改写，二者是"滤镜"与"改写"的分工。

journal 记录即 patch 本身（几个字节到一条摘要消息），resume = 事件流重建 + patch 回放。decision 载荷不再有巨型情况，**blob 机制取消**——`after_tool_call::ReplaceResult` 的单个工具结果直接内联 JSONL，将来 profiling 证明需要溢出存储时再加。

**否决方案：保留 Replace + 内容寻址 blob。** blob 治标——它让"影子历史可以无限大"变得便宜，而不是消除影子历史；patch 从 API 上让分叉只能以增量形式存在。

**否决方案：patch 支持任意插入/重排。** 表达力回归影子历史；配对与顺序不变量也无法结构化校验。

### 7. Filesystem 后端放 stratum-infra，形状最小

`<root>/<run_id>/events.jsonl`（追加写 + fsync）+ 读取器 `read_events(run_dir) -> Result<Vec<DurableAgentEvent>, _>`（逐行解析，尾行截断容忍——崩溃可能留下半行）。run 身份由组合方选择目录表达。不实现清理/保留策略（H3b 随 sqlite 一起定）。

**否决方案：先建 P1 存储抽象再落地。** P1 尚未启动，为 H3a 提前抽象违背克制原则；filesystem 后端就是 P1 将来要归纳的第一个真实实现。

## Risks / Trade-offs

- **[风险] 事件流重放与 journal 查表耦合出错，恢复路径产生与崩溃前不一致的执行。** → resume 集成测试矩阵：在每个耐久边界（Hook 前后、started 前后、iteration 边界）模拟崩溃重启，断言事件流与 decision 回放一致。
- **[风险] `DropHistory` 长期隐藏前缀后，H5 compact 摘要的是完整 committed 历史，压缩产物与模型实际视野存在漂移。** → 语义合法（隐藏 ≠ 删除），在 crate 文档记录该交互，H5 触发方据此决策。
- **[风险] patch 的 `upto` 索引语义在 handler 与 kernel 间理解不一致（含不含 tool result、从 0 还是从 1）。** → 以 committed `messages` 的 0-based 下标、左闭右开区间写入类型文档与 spec，校验测试覆盖边界。
- **[风险] usage 语义修正误导 handler 作者。** → 类型文档、spec 与 crate `AGENTS.md` 三处同步；H2.5 的 spec 场景同步 MODIFIED。
- **[权衡] hook-point 粒度使 runtime 内部多 handler 共享一条 record。** → H2 链落地时由 chain runner 扩展 per-handler record，地址结构预留 handler 位置的空间（注释记录，不实现）。
- **[权衡] 审批 handler 恢复后仍会重新问人（Pending 重试）。** → 这是 H2 确定的 fail-safe 语义；只有 Completed 已提交后才不再问。问人通道的状态恢复属于 handler 实现，不在内核。

## Migration Plan

1. `stratum-core`：三个 journal 事件变体 + `HookDecisionRecord` serde 表示 + usage 文档语义修正。
2. `stratum-agent`：usage 透传改最近一次；`TransformContextDecision` 改为 patch 表达并接入 kernel 校验；`execute_hook` helper 周围接入 journal 写入（Pending → 调用 → Completed/Failed）；resume 重建与前沿判定。
3. `stratum-infra`：filesystem sink + 事件读取器。
4. 测试：journal 写入顺序、resume 重建矩阵（各边界崩溃）、digest 复用/不匹配、usage 语义。
5. 门禁、`crates/stratum-agent` 与 `stratum-infra` 的 `AGENTS.md` 归档、`TODO.md` H3 条目更新。

alpha 前向破坏：usage 语义变化不做兼容；journal 事件为新增变体（additive）。

## Open Questions

- JSONL 尾行容忍与 patch 校验的具体边界在实现中定（记录于 crate 文档）。
- 组合方如何为 resume 选择 run 目录与 system prompt 来源，留给 Web/API 组合接入时定（本 change 只提供 kernel 与后端能力）。
