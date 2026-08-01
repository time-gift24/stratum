# agent-loop-resume Delta（add-context-compaction）

## MODIFIED Requirements

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

### Requirement: 压缩检查点索引加速恢复
filesystem 后端必须（SHALL）在 `TranscriptCompacted` 耐久提交后向 `compact.jsonl` 追加检查点（含压缩迭代号、事件流行号、upto 与摘要 digest）。resume 必须（SHALL）优先使用最新匹配的检查点从事件流中部开始重放；检查点索引必须（SHALL）是派生物——缺失、损坏或校验失败时回退全量重放，不得（SHALL NOT）因索引问题 fail closed。检查点写入必须（SHALL）发生在对应事件落盘之后。

#### Scenario: 从最近检查点快速恢复
- **WHEN** compact.jsonl 存在有效检查点且事件流对应行校验匹配
- **THEN** resume 读取 LoopStarted 后从检查点指示的事件行开始重放，被压缩前缀的事件不参与重放，恢复结果与全量重放一致

#### Scenario: 索引损坏回退全量重放
- **WHEN** compact.jsonl 缺失、截断或检查点与事件流校验不匹配
- **THEN** resume 回退为从事件流开头全量重放，不因索引问题拒绝恢复

#### Scenario: 索引写入落后于事件不致死
- **WHEN** 进程在 TranscriptCompacted 落盘后、检查点追加前崩溃
- **THEN** 索引保持落后状态，resume 或全量重放或从更早检查点开始，结果仍然正确
