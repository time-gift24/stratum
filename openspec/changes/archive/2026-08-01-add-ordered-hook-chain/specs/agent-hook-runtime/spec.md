# agent-hook-runtime Delta（add-ordered-hook-chain）

## ADDED Requirements

### Requirement: Hook Handler 链按序执行
系统必须（SHALL）提供 `HookHandler`（与五个 Hook 点同形、默认 No-op、携带不可变 `HookHandlerVersionId`）与实现 `HookRuntime` 的有序 `ChainHookRuntime`。链语义按点定义：`transform_context`、`transform_tool_call`、`after_tool_call` 必须（SHALL）顺序变换——前一 Handler 的输出视图是后一 Handler 的输入；`decide_tool_call` 必须（SHALL）在第一个 `Block` 处短路；`prepare_next_turn` 必须（SHALL）在 `Stop` 处短路并按 Handler 顺序合并多个 `Inject` 的消息。任一 Handler 失败或返回非法 decision 必须（SHALL）使整个 Hook 点 fail closed。

#### Scenario: 顺序变换线程化
- **WHEN** transform 链中第一个 Handler 修改了 Tool 参数
- **THEN** 第二个 Handler 看到的是修改后的参数，链结束后 kernel 对最终参数复验

#### Scenario: Block 短路
- **WHEN** decide 链中第二个 Handler 返回 Block
- **THEN** 后续 Handler 不被调用，该调用按 Block 处理

#### Scenario: Stop 短路丢弃已收集 Inject
- **WHEN** prepare 链中前一 Handler 返回 Inject、后一 Handler 返回 Stop
- **THEN** 决策为 Stop，已收集的 Inject 消息被丢弃

#### Scenario: Inject 有序合并
- **WHEN** prepare 链中两个 Handler 分别返回 Inject
- **THEN** 决策为单个 Inject，消息按 Handler 顺序拼接

#### Scenario: Handler 失败 fail closed
- **WHEN** 链中任一 Handler 返回类型化失败或非法 decision
- **THEN** 整个 Hook 点以该失败结束，后续 Handler 不被调用

### Requirement: 链版本固定并可恢复校验
`ChainHookRuntime` 必须（SHALL）在构造时按声明顺序固定 Handler 序列，并从有序 Handler 版本计算 `ExtensionSetVersionId`。AgentLoop 必须（SHALL）把该版本随 `LoopStarted` 耐久提交；resume 时事件流中的版本与当前注入 runtime 报告的版本不一致必须（SHALL）fail closed。未提供版本的 runtime 必须（SHALL）视为无固定链，跳过校验。

#### Scenario: 重启后顺序一致
- **WHEN** 组合方以相同顺序构造同名 Handler 链并 resume
- **THEN** 计算的链版本与事件流中的版本一致，恢复继续

#### Scenario: 链版本不匹配 fail closed
- **WHEN** resume 时注入链的版本与事件流记录的版本不同（顺序、成员或 Handler 版本变化）
- **THEN** resume 以类型化错误拒绝，不开始任何模型、Tool 或 Hook 动作
