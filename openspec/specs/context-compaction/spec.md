# context-compaction Specification

## Purpose
TBD - created by archiving change add-context-compaction. Update Purpose after archive.
## Requirements
### Requirement: Kernel 在迭代边界执行持久压缩
当 `prepare_next_turn` 返回携带 `upto` 与摘要的 Compact decision 且其 Completed 记录已耐久提交后，AgentLoop 必须（SHALL）在提交迭代边界前执行压缩：校验切割点合法，用 kernel 归属的 system 标记消息替换 committed context 的前缀 `[0, upto)`，并耐久提交 `TranscriptCompacted` 事件。压缩不得（SHALL NOT）伪造用户或助手消息，不得（SHALL NOT）回退迭代计数。

#### Scenario: 合法压缩执行
- **WHEN** Compact 的 upto 落在消息边界、不切断 tool 配对、不切入当前迭代消息
- **THEN** kernel 提交 TranscriptCompacted，committed 前缀被摘要标记消息替换，下一次迭代从压缩基线构造请求

#### Scenario: 非法切割点 fail closed
- **WHEN** upto 为 0、越界、切断 tool_call/tool_result 配对或切入当前迭代已提交消息
- **THEN** AgentLoop 以 `HookFailure::InvalidOutput` fail closed，不提交压缩、不提交迭代边界

#### Scenario: 伪造角色被拒绝
- **WHEN** Compact 携带的 summary 不是 system 角色，或带有 tool_calls / tool_call_id
- **THEN** AgentLoop 以 `HookFailure::InvalidOutput` fail closed

#### Scenario: 切割点使用 committed 坐标
- **WHEN** Handler 计算 Compact 的 upto
- **THEN** upto 以 prepare_next_turn 快照展示的 committed context 下标为准；request-only patch（如 DropHistory）的视图坐标不得用于 Compact，压缩后 Handler 必须从新的 snapshot 重新计算下标

#### Scenario: 压缩标记可识别
- **WHEN** 压缩完成后的下一次 transform_context 调用
- **THEN** 快照 context 的首条消息是 kernel 归属的压缩标记消息，Handler 可据此识别已发生的压缩

### Requirement: TranscriptCompacted 是事件流的一等事实
`DurableAgentEvent` 必须（SHALL）包含 `TranscriptCompacted` 变体，携带切割点、摘要消息与压缩发生的迭代号。事件日志必须（SHALL）保留全部原始消息；压缩只影响重建视图。同一 run 的多次压缩必须（SHALL）按事件顺序依次应用。

#### Scenario: 日志保留与视图收缩
- **WHEN** 压缩已提交
- **THEN** 事件日志仍包含被压缩的原始消息，而重建的 committed context 以摘要标记消息替代该前缀

#### Scenario: 多次压缩按序应用
- **WHEN** 一个 run 发生两次压缩
- **THEN** 重放按事件顺序应用两次替换，最终视图与崩溃前一致

### Requirement: 压缩不改变 Journal 寻址
压缩不得（SHALL NOT）改变 Hook invocation 的地址与 digest 语义：地址仍为 `(iteration, HookPoint, Option<CallId>)`，tool hook digest 仍哈希 ToolCall，context hook digest 仍为地址本身。压缩前提交的 journal 记录在压缩后必须（SHALL）保持可匹配。

#### Scenario: 压缩前后 Journal 匹配一致
- **WHEN** 压缩发生后 resume
- **THEN** 压缩前已 Completed 的 Hook 记录仍按其地址与 digest 正常复用

### Requirement: 压缩崩溃窗口由 Journal 回放闭合
当 Compact decision 的 Completed 已提交但 `TranscriptCompacted` 尚未提交时进程停止，resume 必须（SHALL）从 journal 回放该 decision 并以记录的摘要执行压缩，不得（SHALL NOT）重新调用 Handler 生成摘要。

#### Scenario: 摘要只生成一次
- **WHEN** 进程在 Completed(Compact) 之后、TranscriptCompacted 之前崩溃并恢复
- **THEN** resume 应用 journal 中记录的摘要完成压缩，Handler 不被再次调用

