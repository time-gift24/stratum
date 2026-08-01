## Context

H1 设计时明确否决了"四个 closure 分别注入"，选择单一 `HookRuntime` 作为组合边界，并把多 Handler 顺序推迟到 H2。现状（H3a 之后）：kernel 在五个 Hook 点各调用一次 runtime，journal 为 hook-point 粒度，`ToolHookTarget` 携带生效授权与 `ToolSpec`；参数校验是 `Tool::validate` 的 per-tool ad-hoc 实现，`ToolSpec.input_schema` 只是 provider 可见声明，没有执行侧的权威校验。

M0 的 `hook-execution-baseline` 要求：有序 ExtensionSet、每个 Handler 不可变版本、同一点多个 Handler 各有 invocation 身份。本 change 收口前两项的组织形态；invocation 粒度保持 hook-point 级（H3a 已定），per-handler 粒度留给 H3b 评估。

## Goals / Non-Goals

**Goals:**

- Handler 成为一等公民：有身份、有顺序、可独立实现单个 Hook 点。
- 链式 Runner 是 `HookRuntime` 的一个实现：kernel 零改动，取消/deadline/journal 语义原样继承。
- 五种点的链语义明确且可测试：顺序变换、Block 短路、Stop 短路、Inject 有序合并。
- 链版本可固定、可校验：重启前后处理器顺序一致。
- `stratum-tools` 有统一 schema 校验边界。

**Non-Goals:**

- per-handler journal、链内部分 Handler 崩溃的细粒度恢复（H3b）。
- Handler 的动态注册/热替换、远程 Handler、脚本 Handler。
- `Tool::validate` 的移除（保留为 schema 之外的语义校验层）。
- legacy Agent、Web 配置界面。

## Decisions

### 1. `HookHandler` 与五方法同形，默认 No-op

```text
trait HookHandler {
    fn descriptor(&self) -> HookHandlerDescriptor;   // 不可变版本身份
    async fn transform_context(...) -> Result<TransformContextDecision, HookFailure> { Ok(Unchanged) }
    async fn transform_tool_call(...) -> ... { Ok(Continue) }
    async fn decide_tool_call(...) -> ... { Ok(Execute) }
    async fn after_tool_call(...) -> ... { Ok(Keep) }
    async fn prepare_next_turn(...) -> ... { Ok(Continue) }
}
```

输入/输出类型与 `HookRuntime` 完全共享（`HookSnapshot`、专属载荷、decision 枚举）。默认 No-op 方法让 Handler 只实现关心的点。descriptor 携带 `HookHandlerVersionId`（stratum-core 已有）。

**否决方案：每个 Hook 点一个独立 trait。** 五个 trait 五套实现样板，且一个策略 Handler 横跨多点时（如审批 + 审计）要拆成多个类型，违背内聚。

### 2. `ChainHookRuntime` 实现 `HookRuntime`，kernel 零改动

链 Runner 持有 `Vec<Arc<dyn HookHandler>>`，按点实现链语义：

- **顺序变换**（transform_context / transform_tool_call / after_tool_call）：逐个调用，前一个的输出是后一个的输入视图。transform_tool_call 线程化当前 `ToolCall`（Cow）；transform_context 线程化应用了累计 patch 的 request view（Cow 物化）；after_tool_call 线程化当前 result。任一 Handler 返回失败 → 整个 Hook 点失败（fail closed）。
- **Block 短路**（decide_tool_call）：顺序调用，第一个 `Block` 立即返回，不再调用后续 Handler。
- **Stop 短路 + Inject 合并**（prepare_next_turn）：顺序调用；`Stop` 立即短路（已收集的 Inject 丢弃——Stop 意味着没有下一轮）；多个 `Inject` 的消息按 Handler 顺序拼接为一个 Inject；全部 Continue → Continue。
- 取消与 deadline：整个链调用仍是 kernel `execute_hook` 的一次调用，`HookControl` 原样透传给每个 Handler；链内部不新增超时概念。

**否决方案：链语义放 kernel，Handler 列表注入 builder。** kernel 重新认识 Handler 列表与顺序，破坏 H1 的单一组合边界；链作为 runtime 实现保持 kernel 无感。

**否决方案：decide 链收集全部 Block 再决定。** 审批类 Handler 有副作用（问人），短路是必须的；且第一个拒绝即定案符合直觉。

### 3. 链版本固定：构造即定序，LoopStarted 落版本

`ChainHookRuntime` 构造时按声明顺序固定 Handler 序列，计算 `ExtensionSetVersionId` = sha256(有序 HandlerVersionId 列表)（stratum-core 已有这两个 newtype）。`AgentLoopBuilder` 从 runtime 取到版本（`HookRuntime` 增加默认返回 `None` 的 `extension_set_version()` 方法），`LoopStarted` 事件新增可选 `extension_set_version_id` 字段（`#[serde(default)]`，旧日志可解析）。resume 时：事件流中记录的版本与当前注入 runtime 报告的版本不一致 → fail closed。这给出"重启前后处理器顺序一致"的机器校验。

**否决方案：每个 invocation record 携带完整 Handler 列表。** 冗余且属于 per-handler 粒度的 H3b 范畴；run 级一次固定足够当前粒度。

### 4. 统一校验边界：schema 为权威，Tool::validate 为补充

`stratum-tools` 新增 schema 校验模块：`validate_against_schema(spec, input) -> Result<(), ToolError>`，用 `jsonschema` crate 对 `ToolSpec.input_schema` 编译校验一次（registry 注册时编译缓存）并在调用时执行。`BuiltinToolRegistry::validate` 改为：schema 校验失败 → `InvalidArgument`；schema 通过后再调用 `Tool::validate` 做工具自定义语义校验（现状逻辑下沉为第二层）。kernel 的原始校验与 transform 链后复验不变——它们调的就是这个统一边界，链后复验天然覆盖"Hook 修改后的非法参数不进入审批或执行"。

**新依赖 `jsonschema`**（workspace 继承）：JSON Schema 校验手写不现实，它是该领域事实标准；许可证 MIT/Apache-2.0，在 deny.toml allow 列表内。

**否决方案：保留 per-tool ad-hoc 校验为唯一边界。** 每个 Tool 重复实现类型/必填检查，Hook 修改后的参数只能依赖各 Tool 自己的校验质量，"统一边界"名存实亡。

**否决方案：校验移到 kernel。** kernel 不认识工具 schema 的获取与缓存职责，stratum-tools 是 schema 的天然所有者。

### 5. 链内的非法 decision 处理

任一 Handler 返回的 decision 校验失败（如空 reason 的 Block、非法 Inject、越界 patch）→ 整个 Hook 点按 `HookFailure::InvalidOutput` fail closed，与单 runtime 行为一致。链不做"跳过坏 Handler 继续"的宽容模式。

## Risks / Trade-offs

- **[风险] 链中途崩溃重试整链，有副作用的 Handler 被重复调用。** → 已知权衡，hook-point 粒度 journal 的固有边界；写入 H3b 评估项；decide 短路语义下审批 Handler 已答过的部分由 H3a 的 Completed 复用兜底（整个点已完成才不重试）。
- **[风险] 顺序变换物化中间视图带来 clone 开销。** → Cow 化：无 Handler 修改时零拷贝；链短（个位数 Handler）时开销可忽略；profiling 后再优化。
- **[风险] schema 校验与 per-tool 校验的拒绝集合变化破坏既有行为。** → 用测试固化：每个内置 Tool 的既有非法输入用例在 schema 边界下仍然被拒绝。
- **[权衡] `HookRuntime` 增加 `extension_set_version()` 默认方法。** 为链版本上报开的口，默认 `None` 使既有实现无感；比新建 trait 继承层级简单。

## Migration Plan

1. `stratum-core`：`LoopStarted` 增加可选 `extension_set_version_id`（serde default）。
2. `stratum-tools`：schema 校验模块 + `BuiltinToolRegistry::validate` 切换 + `jsonschema` workspace 依赖。
3. `stratum-agent`：`HookHandler` trait、`ChainHookRuntime`、五点链语义、`HookRuntime::extension_set_version()`、builder/resume 的版本校验。
4. 测试：链顺序、短路、Inject 合并、版本固定与 resume 不匹配、schema 边界、全部门禁。
5. 归档 crate `AGENTS.md`（stratum-agent、stratum-tools），勾选 `TODO.md` H2 剩余条目。

alpha 前向破坏：校验拒绝集合变化不保留旧行为；`LoopStarted` 字段为 additive。

## Open Questions

- `ExtensionSetVersionId` 的持久化展示形式（hex 全文 vs 前缀）实现时定，写入 crate 文档。
