# agent-hook-runtime Delta（add-context-compaction）

## MODIFIED Requirements

### Requirement: Prepare Next Turn 控制下一次模型迭代
当一个被授权的 Tool cycle 的全部模型可见结果已耐久提交后，AgentLoop 必须（SHALL）调用 `prepare_next_turn`。Decision 必须（SHALL）是 Continue、Stop、Inject 非空 User message 列表，或携带切割点与摘要的 Compact（其执行语义由 context-compaction 能力定义）。

#### Scenario: 继续下一迭代
- **WHEN** prepare-next-turn decision 是 Continue
- **THEN** AgentLoop 提交当前 iteration 完成边界并开始下一次模型迭代

#### Scenario: Hook 主动停止
- **WHEN** prepare-next-turn decision 是 Stop
- **THEN** AgentLoop 提交当前 iteration 和 LoopFinished，并以 `LoopCompletionReason::HookStopped` 结束，而不把该结果伪装成 provider FinishReason

#### Scenario: 注入下一次请求消息
- **WHEN** prepare-next-turn decision 注入一条或多条合法 User message
- **THEN** 这些消息只加入下一次模型 request view、只消费一次，并且不产生 durable Agent message、不进入 Agent history 或 LoopOutcome.new_messages

#### Scenario: 拒绝非法 Inject
- **WHEN** Inject 为空，或包含非 User role、Tool call、reasoning content 或 tool-call identity
- **THEN** AgentLoop 返回 `HookFailure::InvalidOutput`，不开始下一次模型请求

#### Scenario: Compact 触发持久压缩
- **WHEN** prepare-next-turn decision 是携带合法切割点与 system 摘要消息的 Compact
- **THEN** AgentLoop 先提交该 decision 的 Completed 记录，再执行 durable 基线改写，随后提交迭代边界并开始下一次模型迭代

#### Scenario: 压缩后快照即为新基线
- **WHEN** 压缩完成后的下一次迭代调用 transform_context 或 prepare_next_turn
- **THEN** HookSnapshot 的 context 是压缩后的 committed 基线
