# agent-loop-resume Specification

## Purpose
TBD - created by archiving change implement-hook-journal-resume. Update Purpose after archive.
## Requirements
### Requirement: Filesystem 耐久事件后端
系统必须（SHALL）提供生产级 filesystem `DurableEventSink`：按 run 一个目录，事件以 JSONL 追加写并 fsync 落盘。系统必须（SHALL）提供事件读取器，逐行解析并容忍崩溃留下的截断尾行。

#### Scenario: 追加写落盘
- **WHEN** AgentLoop 通过 filesystem sink 提交耐久事件
- **THEN** 事件以一行 JSON 追加到该 run 目录的 events.jsonl 并在返回前 fsync

#### Scenario: 容忍截断尾行
- **WHEN** 读取器遇到 events.jsonl 末尾的半行（崩溃残留）
- **THEN** 忽略该尾行并返回此前全部完整事件，不报错

### Requirement: AgentLoop 从事件流恢复执行
系统必须（SHALL）支持从一个 run 的耐久事件流恢复 AgentLoop：由 `MessageAppended` 序列重建 committed context，应用途中的 `TranscriptCompacted` 事件将前缀替换为摘要标记消息，由最大 `IterationCompleted` 确定迭代前沿并续跑。组合方必须（SHALL）在恢复时重新提供 system prompt 与 run 配置；kernel 不得（SHALL NOT）感知 Session、Turn 或 Store。带 `LoopFinished`、`LoopFailed` 或 `LoopCancelled` 终态事件的 run 不得（SHALL NOT）恢复。

#### Scenario: 重建 Committed Context
- **WHEN** 事件流包含 LoopStarted 与若干 MessageAppended
- **THEN** 恢复后的 committed context 与崩溃前字节一致，续跑从下一迭代前沿开始

#### Scenario: 终态 Run 拒绝恢复
- **WHEN** 事件流包含 LoopFinished、LoopFailed 或 LoopCancelled
- **THEN** resume 以类型化错误拒绝，不开始任何模型、Tool 或 Hook 动作

#### Scenario: 迭代前沿续跑
- **WHEN** 事件流最大 IterationCompleted 为 N 且后续有未完成的 assistant 或 tool 活动
- **THEN** AgentLoop 从迭代 N+1 的边界继续，已完成迭代的模型请求与 Tool 执行不重复

#### Scenario: 重放应用压缩事件
- **WHEN** 事件流在 MessageAppended 序列中包含 TranscriptCompacted
- **THEN** 重放按事件顺序应用压缩，恢复后的 committed context 是以摘要标记消息为前缀的压缩基线

### Requirement: 恢复时 Tool 结果对账
恢复重建时，committed tool result 消息必须（SHALL）是紧邻前序 assistant `tool_calls` 的精确有序前缀；未知、重复、稀疏或乱序的 result 必须（SHALL）视为损坏的恢复历史并 fail closed。`ToolExecutionStarted` 后崩溃的调用按未知结果处理：恢复后重新执行该 Tool，系统不得（SHALL NOT）伪造其结果。

#### Scenario: 缺失后缀重新执行
- **WHEN** assistant 消息有三个 tool_calls 但只有前两个 result 已提交
- **THEN** 恢复后只重新执行第三个调用，前两个结果原样保留

#### Scenario: 乱序结果 fail closed
- **WHEN** committed result 不是前序 assistant tool_calls 的精确有序前缀
- **THEN** resume 以类型化错误 fail closed，不开始任何动作

### Requirement: 压缩检查点索引加速恢复
filesystem 后端必须（SHALL）在该次压缩的 `IterationCompleted` 落盘后向 `compact.jsonl` 追加检查点（含压缩迭代号、窗口起始行、upto 与摘要 digest），窗口起始行必须（SHALL）是第一条保留消息的物理行，使窗口自带完整保留后缀、该迭代 prepare 的 journal 记录与迭代边界。resume 必须（SHALL）优先使用最新匹配的检查点从事件流中部开始重放；检查点索引必须（SHALL）是派生物——缺失、损坏或校验失败时回退全量重放，不得（SHALL NOT）因索引问题 fail closed；检查点写入失败必须（SHALL）降级为索引落后并告警，不得（SHALL NOT）使边界已提交的 run 失败。

#### Scenario: 从最近检查点快速恢复
- **WHEN** compact.jsonl 存在有效检查点且窗口起始行与 TranscriptCompacted 三项（iteration/upto/digest）校验匹配
- **THEN** resume 读取 LoopStarted 后从窗口起始行重放，窗口自带保留后缀与迭代边界，恢复结果与全量重放一致

#### Scenario: 索引损坏回退全量重放
- **WHEN** compact.jsonl 缺失、截断、内容损坏或检查点与事件流校验不匹配
- **THEN** resume 回退为从事件流开头全量重放，不因索引问题拒绝恢复

#### Scenario: 压缩后边界未提交不产生检查点
- **WHEN** 进程在 TranscriptCompacted 落盘后、IterationCompleted 落盘前崩溃
- **THEN** 不新增检查点，resume 走全量重放或更早检查点，prepare 的 journal 记录完整可用，handler 不被重复调用，事件流不被污染

#### Scenario: 索引写入落后于事件不致死
- **WHEN** 进程在 IterationCompleted 落盘后、检查点追加前崩溃，或检查点写入本身失败
- **THEN** 索引保持落后状态，run 不失败，resume 或全量重放或从更早检查点开始，结果仍然正确

