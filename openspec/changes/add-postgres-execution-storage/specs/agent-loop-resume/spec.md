# agent-loop-resume Specification Delta（add-postgres-execution-storage）

## ADDED Requirements

### Requirement: Resume 支持 Postgres 事件流后端
resume 重放必须（SHALL）以后端无关方式工作：组合方提供任一 `DurableEventSink` 读取器（filesystem 或 Postgres），kernel 重建逻辑不变。从 Postgres `durable_events` 表按 `turn_id` 有序读出的事件流，其 resume 行为必须（SHALL）与 filesystem 后端逐事件一致，包括 committed context 重建、迭代前沿、Tool 结果对账、Hook 查表与终态拒绝。

#### Scenario: Postgres 事件流正常恢复
- **WHEN** 一个 run 的事件全部经 Postgres sink 提交后进程重启
- **THEN** resume 从 durable_events 按 seq 有序重放，重建结果与 filesystem 后端处理同一事件序列的结果逐项相等

#### Scenario: 压缩基线跨后端一致
- **WHEN** 事件序列包含 TranscriptCompacted 且经 Postgres 后端持久化
- **THEN** resume 应用压缩得到与 filesystem 后端相同的压缩基线，journal 查表匹配结果一致
