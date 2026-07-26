## Why

当前 Agent runtime 会在内部创建短生命周期的 `RunId`，并发出 `EventSource` 可能与事件载荷相矛盾的事件。基于这些身份语义，无法安全定义 Hook 执行、版本固定和恢复行为，因此应当在引入 Hook runtime 与 Workflow engine 之前修正 beta 协议。

## What Changes

- **BREAKING** 以长生命周期的 `SessionId` 取代 `RunId` 及面向 run 的公开语义。Session 独立于任何 Workflow 图；随着时间推移，同一个 Session 可以包含多个 Agent 或多个 Workflow 版本，但当前版本仅允许一个活跃操作。
- **BREAKING** Agent 接收外部 runtime context，其中包含 Session 身份，以及 Agent 是直接运行还是作为 Workflow 节点运行。Agent 不再创建顶层 runtime 身份。
- **BREAKING** 删除 `EventSource`，将归属字段移入类型化的 `RuntimeEvent` 变体，使事件无法声明相互冲突的来源。
- 将已持久化完整消息的顺序明确命名为 `message_seq`，并使其只存在于已提交的 Agent 消息事件中；传输层 `EventCursor` 保持独立，二者都不成为 Hook 状态或通用的 Session 事件序号。
- 定义不可变的 Agent、Workflow、SkillSet 和 ExtensionSet 版本身份；Agent Turn 恢复时固定已解析的 Agent、Skill、Extension Handler 集合及顺序。
- 定义 `HookInvocationId`：它表示某个 Hook point 上的一次 Handler 调用；同时定义 fail-closed 错误、输入摘要、幂等性和恢复规则。
- Hook 执行日志在逻辑上与 Agent 对话历史和 EventBus 观测分离；本次变更不选择 Session 存储后端。
- 记录 Skill context、Script Extension、链接式 Rust Hook 和远程 Hook Service 的最小威胁模型。
- 明确推迟 Session 数据布局、Workflow 调度、多操作并发、Node 执行身份、attempt、重试、循环和子图设计。

## Capabilities

### New Capabilities

- `session-runtime-identity`：定义 Session 身份、Agent runtime 位置、归属、生命周期边界，以及当前单活跃操作不变量。
- `runtime-event-protocol`：定义 Session 作用域的 envelope、不依赖 `EventSource` 的类型化事件归属、已提交消息的必填 `message_seq`，以及消息序号与传输 cursor 的不同语义。
- `hook-execution-baseline`：定义 Turn 执行期间不可变的 Hook 输入、Handler 顺序与版本固定、Hook 调用身份、journal 边界、fail-closed 恢复行为和信任边界。

### Modified Capabilities

无。仓库当前没有已有的 OpenSpec capability spec。

## Impact

- 影响 `stratum-core` 的公开类型、`stratum-agent` 的 Agent 入口与事件发出、`stratum-store` 的持久化契约、`stratum-infra` 的事件路由、`stratum-api` 的 HTTP/SSE 投影、`stratum-agent-builtin` 的 REPL 组合，以及 `stratum-web` 的前端协议类型和 reducer。
- 现有 beta HTTP/SSE 载荷、保留事件、持久化 Agent 状态和持久化消息 envelope 明确不保持 runtime 兼容。部署时直接丢弃不兼容的 beta 数据并重新初始化；本 change 不设计离线迁移、双协议兼容或任何回滚路径。
- 本次基线变更不引入 Session 状态后端、Workflow scheduler、Hook 实现、Script host 或远程 Hook Service。
