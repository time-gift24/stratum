## Context

H 系列讨论确立了上下文压缩的三层分工：结果级压缩已由 `after_tool_call::ReplaceResult` 覆盖；request-only 视图调整由 `ContextPatch`（含 `RewriteHistory`）覆盖；产品级的**持久压缩**一直没有承载者——`transform_context` 的 Replace 被否决（影子历史），patch 不写回。本 change 落地第三层。

关键前置（已就位）：`HookSnapshot.usage` 是最近一次模型响应上报（当前 context 规模信号）；`prepare_next_turn` 运行在迭代边界（tool 配对完整的安全切割点）；journal 的 Completed 在受影响动作前提交且 resume 可回放（非确定性决定固化的机制）；`AgentLoop::resume` 从事件流重建 committed context。

已知的交互注意点（H3a 设计记录在案）：handler 长期用 `DropHistory` 隐藏前缀时，压缩摘要的是完整 committed 历史，摘要产物与模型实际视野可能存在漂移——触发方应知晓。

## Goals / Non-Goals

**Goals:**

- handler 表达压缩意图并携带摘要；kernel 执行配对安全的 durable 基线改写。
- 压缩是事件流的一等事实：resume 从压缩基线恢复，崩溃窗口由 journal 回放闭合。
- 触发策略完全归 handler/组合方（`snapshot.usage` 是现成依据）。

**Non-Goals:**

- kernel 内置 LLM 摘要器或自动阈值策略。
- 结果级压缩（已覆盖）、事件日志物理清理（H3b）、Web 压缩标记 UI。
- legacy Agent、跨 Session 的压缩。

## Decisions

### 1. Compact 是 prepare_next_turn 的新 decision，摘要由 handler 携带

```text
PrepareNextTurnDecision = Continue | Stop | Inject { messages }
                        | Compact { upto: usize, summary: ChatMessage }
```

handler 自己决定何时压缩（读 `snapshot.usage`）、自己生成摘要（要 LLM 就自己调——handler 是受信进程内代码，持有 provider 是它的组合自由）。kernel 不引入 summarizer 组件，因此没有新的版本固定/配置面。

**否决方案：kernel 注入 `ContextCompactor` 边界。** 为"调一次 LLM 写摘要"新增 kernel 组件违反克制原则；且摘要 prompt/模型选择是产品策略，放 kernel 会把策略钉进内核。

**否决方案：复用 `ContextPatch::RewriteHistory` 让它可写回。** patch 的合同是 request-only，给它加写回会混淆两个层次的语义；Compact 是独立的持久操作。

### 2. kernel 执行压缩并强制不变量

`prepare_next_turn` 返回 Compact 且 decision 落 journal（Completed）后，kernel 在迭代边界执行：

1. 校验 `upto`：0-based、左闭右开 `[0, upto)`；不得越界；切割点不得落在 tool_call/result 配对中间；不得切入当前迭代已提交的消息（迭代起点由 kernel 跟踪）；`upto == 0` 为无效（无操作压缩是 handler 的逻辑错误）→ 以上违反均判 `HookFailure::InvalidOutput`，fail closed。
2. 校验 `summary`：kernel 归属的 system 角色标记消息；若 handler 提交了其他角色/带 tool_calls/tool_call_id 的消息，判 `InvalidOutput`——压缩不得伪造用户或助手发言。
3. 耐久提交 `TranscriptCompacted { upto, summary, .. }`（先于迭代边界事件），然后把 committed context 的前缀 `[0, upto)` 替换为 summary 标记消息。

**坐标系规则**：`Compact.upto` 永远以 `prepare_next_turn` 快照展示的 **committed context** 下标为准。`ContextPatch`（如 `DropHistory`）的 `upto` 是当次 request view 的坐标，两套坐标不得混用：handler 必须每次从当前 snapshot 现算下标，禁止缓存或跨点复用旧下标——压缩会移动所有后续位置。

**标记消息即合同**：压缩标记消息的文本模板由 kernel 拥有并写入 crate 文档，是稳定合同；`transform_context` 的 handler 可以通过首条消息识别"已压缩过"。

system 角色标记消息对 provider 兼容（OpenAI 风格 API 接受 mid-conversation system 消息），且对 UI 是天然的"已压缩"分隔符。摘要消息计入 `LoopOutcome.new_messages`——它确实是本 run 新增的 committed 消息，诚实呈现。

### 3. TranscriptCompacted 是事件流的一等事实

```text
TranscriptCompacted { upto: u64, summary: ChatMessage, compacted_iteration: u64 }
```

事件**日志保留全部原始消息**（审计不丢）；重建视图时应用压缩：`replay` 遇到 `TranscriptCompacted` 就把已重建前缀替换为 summary。多次压缩按事件顺序依次应用。崩溃窗口闭合：Completed(Compact) 已提交但 `TranscriptCompacted` 未提交 → resume 重放 journal 得到 decision，直接以记录的摘要执行压缩（不重新调 handler，摘要不会二次生成）。

### 4. 压缩不改变 journal 寻址与 digest

Hook 地址是 `(iteration, HookPoint, Option<CallId>)`，与消息内容无关；tool hook 的 digest 哈希 `ToolCall`，context hook 的 digest 是地址本身。消息前缀被改写不影响任何既有 journal 记录的匹配语义。迭代计数器持续推进，不回退。

### 4b. compact.jsonl 是派生检查点索引，不是第二真相

filesystem 后端维护派生检查点索引 `compact.jsonl`（可完全由事件流重建；缺失、损坏或校验失败一律回退全量重放，索引问题永不 fail closed）。检查点记录：`{ compacted_iteration, window_start_line, upto, summary_digest }`。三条不变量：

1. **边界后写**：检查点在该次压缩的 `IterationCompleted` 落盘后才追加（sink 记一笔待落压缩，边界落盘时 flush）。有检查点 ⟹ 边界已提交 ⟹ "压缩已提交而边界未提交"的崩溃窗口不存在检查点，只能走全量重放——该窗口的 journal 记录永远安全。
2. **窗口自足**：`window_start_line` 是**第一条保留消息的物理行**（按 `upto` 定位第 upto 个 `message_appended`，写检查点时扫文件一次，压缩低频可接受），不是 `TranscriptCompacted` 行。窗口 `[LoopStarted] + 自 window_start_line 起` 自带完整保留后缀、该迭代 prepare 的 journal 记录、压缩事件与迭代边界——resume 所需的一切都不在窗口之前。
3. **重放双模式**：replay 应用 `TranscriptCompacted` 时，`upto <= 当前 messages 长度`走绝对坐标 splice（全量流）；`upto > 当前长度`说明处于检查点窗口（当前 messages 已是保留后缀本身），直接前置 summary。窗口分支的正确性由规则 1/2 与检查点的三项校验（iteration/upto/digest）在 infra 边界保证；全量流中 `upto` 越界仍 `CorruptedCompaction` fail closed。

resume 快速路径：读 `LoopStarted`（链版本校验）→ 读最新检查点 → 校验 `window_start_line` 指向的行确为 `message_appended`、且窗口内能找到与检查点三项一致的 `TranscriptCompacted` → 匹配则从该行起重放，否则回退全量。

**否决方案：检查点指向 `TranscriptCompacted` 行。** 窗口丢失保留后缀（其物理位置在压缩事件之前）与 prepare 的 journal 记录——resume 上下文残缺、handler 被重复调用、追加同地址 Pending 污染事件流使后续全量重放永久 fail closed。窗口必须自足。

**否决方案：compact.jsonl 作为压缩历史的权威记录。** 压缩历史已经以 `TranscriptCompacted` 事件存在于真相流；再写一份权威历史制造双真相，正是单 sink 原则要消灭的。索引只加速，不承载真相。

### 5. 压缩后各 Hook 点行为

压缩完成后的下一次迭代：`transform_context` 的快照 context 是压缩基线（committed 已改写）；`prepare_next_turn` 同理。无特殊豁免——压缩是 committed context 的普通新状态。

## Risks / Trade-offs

- **[风险] handler 生成的摘要质量差导致信息丢失。** → 策略与质量归 handler/组合方；kernel 只保证机制正确（配对、归因、可恢复）。将来可在组合层做受信任的摘要 handler。
- **[风险] DropHistory 隐藏与压缩摘要的来源漂移（已知记录在案）。** → crate 文档注明；触发方若以 view 为准生成摘要应自知。
- **[风险] 压缩标记消息干扰模型行为（system 角色在中间位置）。** → 标记文本明确说明"此前历史已压缩为摘要"；测试固化 provider 请求形状。
- **[权衡] 日志物理体积不缩小（事件全保留）。** → 视图收缩是本 change 的目标；日志清理由 H3b 的保留策略处理。

## Migration Plan

1. `stratum-core`：`TranscriptCompacted` 事件变体 + `HookDecisionRecord` 的 Compact 表示。
2. `stratum-agent`：`Compact` decision 变体与校验、kernel 压缩执行、重放应用、journal 回放闭合崩溃窗口。
3. 测试：校验矩阵（越界/切对/切入当前迭代/伪造角色）、压缩后视图、多次压缩、崩溃窗口回放、provider 请求形状。
4. 门禁、crate AGENTS.md 归档、TODO.md H5 勾选、openspec validate。

alpha 前向破坏：decision 新增变体是 additive；`#[non_exhaustive]` 枚举的下游 `_` 分支按宪法 fail closed 处理。

## Open Questions

- 压缩标记消息的具体文本模板实现时定（写入 crate 文档）。
