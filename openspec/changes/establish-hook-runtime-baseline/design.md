## Context

当前 runtime 把一次 Agent Turn 当作顶层 run。`Agent::run_turn` 创建 `RunId`，`StreamEnvelope` 携带该 run 身份；事件载荷可能描述一个 Agent，而 `EventSource` 却声明事件属于 run。`AgentStore` 将活跃 run 和 Turn 与长期存在的 Agent 消息历史一同持久化，EventBus 则只按 `AgentId` 路由。

这一模型对 Stratum 而言过于狭窄。Session 是长期存在、与图无关的核心资产：不同版本的 Agent 或 Workflow 可以随着时间推移在其中运行，同时 Session 保留共享状态。Agent 对话历史仍归某个 Agent 实例所有；Workflow 中的 Agent 节点不会仅仅因为和另一个 Agent 位于同一 Session，就继承后者的历史。当前 beta 允许直接破坏协议兼容，并且一个 Session 内至多只有一个活跃操作。

在引入 Script Extension 或远程 Hook Service 之前，Hook 行为必须具备可恢复性。因此，这一基线需要稳定的 Session 与 Turn 身份、不可变的 Handler 顺序、每次 Handler 调用独立的 invocation 身份，以及一个不与 Agent 历史或 EventBus 观测混淆的 journal 边界。

## Goals / Non-Goals

**目标：**

- 以长期存在、与图无关的 Session 身份取代面向 run 的身份。
- Agent 执行所需的 Session 与位置由组合边界传入。
- 删除 `EventSource`，使无效的事件归属组合无法通过类型表达。
- 保持持久化完整消息顺序与传输重放位置之间的现有区分。
- 定义可恢复 Hook chain 所需的不可变版本身份和 invocation 身份。
- 在加入 Hook 实现之前，定义 fail-closed 的 Hook 错误、幂等性与恢复规则。
- 为 Skill、Script、链接式 Rust Hook 和远程 service extension 建立最小信任边界。
- 保持设计足够小，只为 H1 解锁，不提前设计 Workflow 调度或 Session 存储。

**非目标：**

- 定义 Session 共享哪些数据，以及这些数据的 namespace、schema 或存储后端。
- 实现四个 H1 Hook 契约、Hook runner 或 Hook journal 后端。
- 实现 Workflow 编译、调度、重试、循环、子图或 Session 并行操作。
- 引入 `NodeExecutionId`、`AttemptId` 或通用执行层级。
- 在不同 Agent 身份之间共享对话历史。
- 保持与 beta 状态或 wire format 的 runtime 兼容。
- 设计或实现 beta 数据迁移、协议回滚、数据回滚、双读双写或新旧 runtime 混跑能力。

## Decisions

### 1. Session 是顶层关联与共享边界

`SessionId` 是由 host 或未来 Session runtime 持有的 UUIDv7 newtype。Session 的生命周期跨越单次 Agent Turn 和 Workflow 版本。某个 Agent Turn 完成或失败，不意味着 Session 完成或失败。

当前版本强制一个 Session 内至多存在一个活跃的 Agent 或 Workflow 操作。这样，在并发操作尚无真实调用方之前，不必提前引入另一层执行身份。未来支持 Session 多操作时，可以增加从属身份，而无需改变 `SessionId` 的含义。

`AgentId` 继续标识一个 Agent 实例及其对话历史。同一 Session 中的不同 Agent 不共享对话历史。未来可以由 Hook 实现将 Session 状态与结果提供给 Agent。

**考虑过的替代方案：** 把 `RunId` 改名为 `SessionId`，但仍然每个 Turn 创建一个 Session。否决该方案，因为它只是以误导性的名称保留 run 生命周期，无法表达长期共享资产。

### 2. Agent 执行接收最小外部 context

host 在 Turn 开始前提供不可变的 `AgentRuntimeContext`：

```text
AgentRuntimeContext
├── session_id
└── location
    ├── Direct
    └── WorkflowNode {
          workflow_version_id,
          node_id
        }
```

Agent 负责创建 `TurnId`，因为 Turn 是 Agent 本地的可恢复操作。Agent 不创建 `SessionId`。`AgentLocation` 使用 enum，而不是可选的 Workflow 字段，从而避免 direct 与 embedded Agent 执行形成字段填充不完整的状态。

此处不引入 node activation 或 attempt 身份。在尚无 Workflow 循环与重试时，`(SessionId, WorkflowVersionId, NodeId)` 足以定位唯一的活跃节点。未来的循环设计可以增加 `NodeActivationId`；未来的重试设计起初可以只使用 attempt counter。

**考虑过的替代方案：** 现在就增加 `NodeExecutionId` 和 `AttemptId`。否决该方案，因为二者并不能消除当前产品实际可能产生的任何歧义状态。

### 3. Runtime event 自身携带类型化归属

`StreamEnvelope` 改为以 Session 为作用域：

```text
StreamEnvelope
├── session_id: SessionId
├── timestamp
├── event: RuntimeEvent
└── metadata
```

删除 `EventSource`。每个 `RuntimeEvent` 变体包含该事件族所需的全部主要身份：

```text
RuntimeEvent
├── Session { event: SessionEvent }
├── Node {
│     workflow_version_id,
│     node_id,
│     event: NodeEvent
│   }
└── Agent {
      agent_id,
      turn_id,
      location: AgentLocation,
      event: AgentEvent
    }
```

Session 生命周期事件描述长期存在的资产本身；它们不会取代每个 Turn 的完成或失败事件。Agent 生命周期、LLM、审批、消息和 plan 事件继续归于 `AgentEvent`。Node 生命周期与输出事件继续归于 `NodeEvent`。删除未使用的顶层 LLM 事件族与 run 生命周期变体。

EventBus 按 `SessionId` 路由和订阅，使一个 stream 能够包含多个 Agent 或未来 Workflow 节点的事件。Agent 消息历史仍按 `AgentId` 查询。

**考虑过的替代方案：** 保留 `EventSource`，并根据载荷验证它。否决该方案，因为类型仍会允许矛盾状态，并重复表达路由身份。

### 4. 消息顺序与传输重放彼此独立

`message_seq` 是已提交完整 Agent 消息的必填 `u64` 字段，只存在于 `AgentEvent::Message` 变体内。它不属于 `StreamEnvelope`，也不是 `Option`。其作用域是 `(AgentId, message_seq)`；即使来自多个 Agent 的消息出现在同一个 Session stream 中也是如此。

未持久化消息使用独立的 append 输入类型，不作为 runtime event 发布。持久化边界分配下一个 `message_seq` 并返回已提交消息，只有该返回值才能构造并发布 `AgentEvent::Message`。因此，非消息事件无法携带消息序号，已发布的完整消息也无法缺少消息序号。

```text
NewAgentMessage
      │
      ▼
AgentStore::append_message
      │ 分配 message_seq
      ▼
AgentEvent::Message {
    message_seq: u64,
    message
}
```

`EventCursor` 仍是 `EventRecord` 返回的不透明传输位置。它只用于恢复保留的 Session 订阅，不得与 `message_seq` 比较，也不得作为持久化执行状态使用。

Session stream 允许交错多个 Agent 的事件，但不提供跨 Agent 的业务全序。consumer 必须使用 `(AgentId, message_seq)` 进行消息排序、分页与去重，不得把 `message_seq` 解释为 Session 全局序号。若需要观察传输到达顺序，只能使用不透明的 `EventCursor`。

此基线不为所有 Session 事件引入持久化全序。该决定属于未来的 Session 持久化契约。

### 5. 版本是不可变引用，并在可恢复 Turn 上固定

公开且经过校验的 newtype 用来区分逻辑身份与不可变发布版本：

- `AgentId` 标识 Agent 实例；`AgentVersionId` 标识不可变的 Agent 行为。
- `WorkflowVersionId` 标识不可变的图版本。
- `SkillSetVersionId` 标识不可变且有序的 Skill 集合。
- `ExtensionSetVersionId` 标识不可变且有序的 Handler 集合及其配置。

Session 不会永久固定到某一组版本。每个可恢复 Agent Turn 记录其已解析 Agent、model、tool、Skill set、Extension set 和精确 Handler 顺序的 snapshot。恢复时必须解析到同一 snapshot。同一 Session 中后续 Turn 可以选择更新的版本。

`ToolSetFingerprint` 记录 Turn 使用的、provider 可见的精确有序 spec、授权 metadata 与实现身份。实现初期可以只支持内置 tool，但重启时不得在相同 fingerprint 下静默替换行为。

使用具体 newtype 而不是通用 artifact/version wrapper，以免不同领域身份被意外混用。

### 6. HookInvocationId 标识一次 Handler 调用

一个 Hook point 可以运行一条有序 Handler chain。每次 Handler 调用都获得不同的 UUIDv7 `HookInvocationId`：

```text
Turn
└── before_tool_call
    ├── Handler A → invocation H1
    ├── Handler B → invocation H2
    └── Handler C → invocation H3
```

一次 invocation 的持久化语义地址包含 Session、Agent、Turn、Hook point、Handler 位置和不可变 Handler 版本，以及与该 Hook point 相关的 operation identity。journal 在该地址记录 invocation ID 与 input digest。UUID 是远程幂等键；语义地址则防止重启时为同一次逻辑 Handler 调用创建第二个 invocation。

### 7. Hook 恢复采用 fail-closed 与 journal-first

未来 Hook journal 遵循以下状态转换：

```text
Absent → Pending → Completed
                 ├→ Failed
                 ├→ TimedOut
                 └→ Cancelled
```

- 调用 Handler 之前持久化提交 `Pending`。
- Agent 执行受影响动作之前，持久化提交包含类型化 decision 的 `Completed`。
- 恢复时复用 `Completed` decision，不再次调用 Handler。
- 恢复时保留终态错误结果，不静默重试。
- 进程崩溃遗留的 `Pending` invocation 只有在复用相同 `HookInvocationId` 和不可变 Handler 版本时才能再次调用。
- 语义地址、版本或 input digest 的任何不匹配都会使 Turn fail closed。

对于同一个 invocation ID，Handler 必须无副作用或具有幂等性。在 Handler 已完成、但 journal 尚未提交这一崩溃窗口内，无法保证外部副作用 exactly-once。

journal 在逻辑上属于 Session/Turn 执行状态。它不加入负责 Agent 状态与对话历史的 `AgentStore`，也不从 EventBus 事件重建。本次变更只记录这一边界；在 H3/P1 出现具体实现之前，不提前引入新的存储 trait。

### 8. Hook 错误与观测保持分离

影响 decision 的 Hook 错误、超时、取消、无效输出、不兼容协议版本及不可用的固定 Handler，都会产生类型化失败。跨公开边界暴露的错误消息必须经过清理，不得包含 prompt、tool 参数、结果、secret、extension 输出或 host 路径。

EventBus 可以发布 Hook 执行的观测事件，但这些事件不能作出 decision、不能代替 journal 持久化，也不能改变恢复行为。

### 9. 明确 Extension 信任等级

| Extension 形式 | 信任等级 | 最小边界 |
|---|---|---|
| Skill | 仅数据，不可信内容 | 不可变版本、大小/路径/编码校验，不提升 Tool 或 Secret 权限 |
| Script Extension | 不可信代码 | 默认进程外运行，显式声明文件/网络/secret capability，限制时间/内存/输出/并发 |
| 链接式 Rust Hook | 完全可信的进程内代码 | 构建时或由 operator 安装，固定 runtime 兼容性，协作式取消与 deadline |
| Hook Service | 通过不可信传输访问的远程可信 service | 认证传输、tenant/project 授权、固定 service 身份/版本/endpoint、限制输入输出、invocation 幂等性 |

所有形式只接收 Hook 契约所需的最小输入。日志与 audit record 只包含身份和安全分类，不包含敏感载荷。

## Risks / Trade-offs

- **[风险] 破坏性改名会使 beta API 与持久化数据失效。** → 提升持久化 schema 版本，明确拒绝旧记录，并直接重置不兼容的 beta 数据与保留传输数据；不提供迁移或回滚路径。
- **[风险] Session 的生命周期可能超过当前建模的所有 version snapshot。** → 按可恢复 Turn 固定版本，绝不在 Session 全局固定。
- **[风险] 第一阶段的单活跃操作约束可能被误固化为 Session 的永久语义。** → 该约束只在第一阶段的组合与调度边界执行，不编码为 `SessionId` 的含义；未来引入并发操作时再增加从属 operation identity。
- **[风险] 再次调用 `Pending` Handler 可能重复副作用。** → 复用同一 invocation ID，并要求 Handler 无副作用或具备幂等性。
- **[风险] 只有版本身份而无 artifact 完整性保证时，可能解析到被修改的内容。** → 不可变 store 必须拒绝修改；可分发的 Script/Skill artifact 在 S1/S2 增加内容摘要。

## Migration Plan

本节只描述新协议的前向落地顺序，不承担数据迁移或回滚设计。

1. 引入新的 Session、version、location 和 Hook invocation newtype，并在公开边界替换 `RunId`。
2. 修改 Agent 入口，由 host 提供 `AgentRuntimeContext`；接受 Turn 之前持久化 Session 与 Turn snapshot 字段。
3. 以类型化 Session/Node/Agent 事件族替换 `EventSource` 以及 run/node 载荷变体，随后更新 Store 校验与 Agent 事件发出逻辑。
4. 将内存和 NATS 路由从 Agent subject 改为 Session subject；更新 SSE 和前端投影。
5. 提升严格的持久化 state/message schema 版本，拒绝旧 runtime 记录，删除不兼容的 beta 状态，并重建保留的 beta event stream。
6. 将 `docs/PROTOCOL.md`、`ARCH.md`、`TODO.md`、各 crate 的 `AGENTS.md` 与 fixture 更新为已接受的术语。

本 change 是纯前向的破坏性变更：不设计、不实现、不验证应用回滚、数据回滚、协议降级、旧数据恢复或新旧 writer 混合运行。任何实现方案和测试任务都不得为这些路径增加兼容代码。

## Open Questions

- 具体的 Session 状态后端和共享数据模型有意推迟到 P1 及后续 Hook 实现。
- 可分发 Skill 与 Script artifact 的规范摘要格式有意推迟到 S1/S2；本基线只要求不可变版本身份，后续再绑定完整性信息。
