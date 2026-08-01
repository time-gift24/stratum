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
系统必须（SHALL）支持从一个 run 的耐久事件流恢复 AgentLoop：由 `MessageAppended` 序列重建 committed context，由最大 `IterationCompleted` 确定迭代前沿并续跑。组合方必须（SHALL）在恢复时重新提供 system prompt 与 run 配置；kernel 不得（SHALL NOT）感知 Session、Turn 或 Store。带 `LoopFinished`、`LoopFailed` 或 `LoopCancelled` 终态事件的 run 不得（SHALL NOT）恢复。

#### Scenario: 重建 Committed Context
- **WHEN** 事件流包含 LoopStarted 与若干 MessageAppended
- **THEN** 恢复后的 committed context 与崩溃前字节一致，续跑从下一迭代前沿开始

#### Scenario: 终态 Run 拒绝恢复
- **WHEN** 事件流包含 LoopFinished、LoopFailed 或 LoopCancelled
- **THEN** resume 以类型化错误拒绝，不开始任何模型、Tool 或 Hook 动作

#### Scenario: 迭代前沿续跑
- **WHEN** 事件流最大 IterationCompleted 为 N 且后续有未完成的 assistant 或 tool 活动
- **THEN** AgentLoop 从迭代 N+1 的边界继续，已完成迭代的模型请求与 Tool 执行不重复

### Requirement: 恢复时 Tool 结果对账
恢复重建时，committed tool result 消息必须（SHALL）是紧邻前序 assistant `tool_calls` 的精确有序前缀；未知、重复、稀疏或乱序的 result 必须（SHALL）视为损坏的恢复历史并 fail closed。`ToolExecutionStarted` 后崩溃的调用按未知结果处理：恢复后重新执行该 Tool，系统不得（SHALL NOT）伪造其结果。

#### Scenario: 缺失后缀重新执行
- **WHEN** assistant 消息有三个 tool_calls 但只有前两个 result 已提交
- **THEN** 恢复后只重新执行第三个调用，前两个结果原样保留

#### Scenario: 乱序结果 fail closed
- **WHEN** committed result 不是前序 assistant tool_calls 的精确有序前缀
- **THEN** resume 以类型化错误 fail closed，不开始任何动作

