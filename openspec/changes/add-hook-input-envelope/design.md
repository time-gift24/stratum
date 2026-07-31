## Context

H1/H2 之后 `HookRuntime` 有五个方法，输入各自为政：`transform_context` 拿 `iteration + &LoopContext`，三个 Tool Hook 拿 `iteration + &ToolCall + &ToolHookTarget`（after 多一个 `&ChatMessage`），`prepare_next_turn` 拿 `iteration + &LoopContext`。两轮设计讨论（结果级压缩需要历史、压缩触发需要 TokenUsage）都因输入太窄而无法表达。Hook 层定位是内部信任层，写侧已经通过类型化 decision 收窄，读侧没有必要同样收窄。

另一个现实约束：H3 将冻结 journal 的 input digest，S2 将冻结 wire protocol，S1 将开始写 handler。输入形状在那之后改动会从"内部重构"升级为"协议破坏"，所以现在是最便宜的窗口。

## Goals / Non-Goals

**Goals:**

- 一个借用公共信封承载所有 Hook 的公共读侧状态，五个输入结构统一嵌入。
- 逐点钉死快照语义，消除"共享状态"在各边界处的歧义。
- `after_tool_call` 获得完整历史；所有 Hook 获得累计 `TokenUsage`。
- 扩展性集中：未来新增公共字段只改信封一处，五个输入自动继承。
- 宽读窄写：decision 词汇与安全不变量不变。

**Non-Goals:**

- 不新增 Hook 点、decision 变体或写回能力（压缩是 H5）。
- 不把工具列表放进信封（S1 评估后再定）。
- 不改 journal、wire protocol、legacy Agent、Web。

## Decisions

### 1. 信封是借用快照，不是 owned 状态

```text
HookSnapshot<'a> {
  iteration: u64,
  context: &'a LoopContext,
  usage: Option<TokenUsage>,
}
```

`#[non_exhaustive]`、`Debug + Clone + Copy`。借用意味着零分配、零 clone；`Copy` 让 handler 传递快照不受借用约束。`usage` 是 owned 小值（三个 u64），复制无负担。

**否决方案：每个 input 各自零散加字段。** 五个结构分别膨胀，扩展点分散，正是要避免的形状。

**否决方案：给 handler `&AgentLoop` 或回调接口。** 内核内部状态整体暴露，读侧失控，且无法在 H3 做确定性 digest。

### 2. 快照的 context 语义逐点钉死

`snapshot.context` 定义为"该 Hook 边界时刻的 committed context"，具体到点：

- `transform_context`：committed context + 本次待消费的一次性 Inject（即 request view 的基底，与现状一致）。
- `transform_tool_call` / `decide_tool_call`：含当前 assistant 消息与本 cycle 已提交 tool result 的 committed context。
- `after_tool_call`：同上，但**不含**当前未提交的 result（该 result 在专属载荷 `result` 字段中）。
- `prepare_next_turn`：含本 cycle 全部已提交结果（与现状一致）。

`TokenUsage` 为本次 run 截至该边界已累计的量；provider 从未上报时为 `None`。kernel 在每次模型响应后累加，构造快照时读取。

**否决方案：所有点统一给"最终 committed context"。** 在 tool cycle 中间不存在这样的稳态，只能逐边界定义。

### 3. 专属载荷留在各输入结构

输入 = 信封 + 点的专属载荷（`tool_call`、`tool`、`result`）。信封只承载"所有点都可能需要"的公共状态；点的本职工作数据不进信封，避免信封退化为 god-object。

### 4. 宽读窄写作为显式原则归档

decision 词汇、写回语义、身份（`CallId`/Tool name）、配对不变量全部不变。读侧放宽不改变任何写侧约束。此原则写入 crate `AGENTS.md`，作为后续所有 Hook 合同演进的默认立场。

### 5. alpha 前向破坏

五个输入结构的字段调整（移除散装 `iteration`/`context`，嵌入 `snapshot`）直接改，仓库内实现与测试前向切换，不留双轨。

## Risks / Trade-offs

- **[风险] 环境化输入诱导 handler 隐式依赖无关状态，行为难以推理。** → 用"宽读窄写"原则约束：读可以宽，但任何决策的*效果*仍由狭窄 decision 表达；H3 的 digest 会如实记录 handler 读了什么状态。
- **[风险] 快照 digest 变大，H3 journal 记录膨胀。** → digest 是哈希不是全文；且统一的快照 digest 比五种各异输入更规则。
- **[风险] `usage` 累计增加 kernel 状态。** → 三个 u64 的累加器，模型响应本就有 usage 上报，无新外部依赖。
- **[权衡] 工具列表暂不放进信封。** → S1 评估后再加，届时只改信封一处，正是本 change 建立的扩展路径的首次实战。

## Migration Plan

1. `hook_runtime` 定义 `HookSnapshot`，五个输入结构嵌入并移除散装字段，No-op 与导出同步。
2. `agent_loop/runner.rs` 在五个调用点构造快照，模型响应后累计 usage。
3. 测试基建（recording runtime）与全部 hook 测试前向切换，新增快照语义断言（各点 context 内容、usage 累计、after 不含未提交 result）。
4. 结构性验收：临时给 `HookSnapshot` 加一个字段，确认五个输入无需改动即继承（验证后保留该测试或移除临时字段，视形态而定）。
5. fmt、clippy、workspace tests；更新 `crates/stratum-agent/AGENTS.md`（宽读窄写原则与信封）并勾选 `TODO.md` H2.5 条目。

alpha 前向破坏，不设计兼容层与回滚路径。

## Open Questions

无阻塞问题。工具列表是否进信封留给 S1；压缩 decision 形状留给 H5。
