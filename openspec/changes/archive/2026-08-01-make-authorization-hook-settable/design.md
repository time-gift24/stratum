## Context

`hookify-tool-approval` 把审批 hook 化后，`ToolHookTarget.authorization` 仍是注册表静态声明的只读终判：审批 handler 看到 `Some` 就必须问人，无法表达"这个 session 全部升级"、"CI 模式低危放行"等 per-call 动态策略。同时 `ToolExecutor::execute` 作为公开方法内部仍做授权查询与校验，是一条绕过 decide hook 的无闸门调度路径，且与 kernel 编排路径重复查询（授权 ×2、校验 ×3）。

## Goals / Non-Goals

**Goals:**

- 授权从注册表静态终判变为 Hook 可写的 per-call 变量，kernel 只搬运生效值。
- `ToolExecutor::execute` 收窄为 `pub(crate)` 纯机制，调度路径零授权概念。

**Non-Goals:**

- 不改变 decide 相位的 Execute/Block 词汇与"审批所见即所执行"保证。
- 不为授权覆写增加 kernel 侧合理性检查（含降级检查）。

## Decisions

### 1. 生效授权由 kernel 携带，与修改后参数同构

`transform_tool_call` 的 decision 扩展为 `Modify { arguments: Option<Value>, authorization: Option<AuthorizationOverride> }`。kernel 计算生效值（无覆写=注册表默认，`PreAuthorize`→`None`，`Set`→`Some`）后喂给 decide/after 的 `ToolHookTarget`，从不基于该值分支。这与 `ModifyArguments` 携带 `final_call` 进入 decide 是同一模式：kernel 搬运不透明数据，策略全在 handler。

**否决方案：授权变量放 runtime 内部穿线。** kernel 不可见则 H3 journal 无法记录，崩溃恢复后无法重放；跨 Hook 点状态靠 handler 私有内存携带正是 journal 要消灭的模式。

### 2. 覆写只发生在 transform 相位

decide 相位保持 `Execute | Block`，不能改参数也不能改授权——决策方看到的参数与授权就是实际生效的。`Modify` 双 `None` 判 `InvalidOutput`（应该用 Continue），防止无意空操作掩盖逻辑错误。

### 3. execute 收窄为 `pub(crate)` 纯机制

`execute(tool, tool_call, cancellation)`：只吃 `hook_lookup` 解析的工具句柄与 decide 放行的最终 call，函数体只剩取消检查、`ToolExecutionStarted` 耐久提交、dispatch。公开无闸门路径与重复查询一并消除；缺失工具与校验失败仍在 kernel 编排层转化为模型可见错误结果。

### 4. 降级是 handler 的明示责任

`PreAuthorize` 可以把危险工具标成免审批。内部信任层接受这一能力，合同明确 kernel 不做任何合理性检查；需要防降级的组合方应在 handler 链中自行实施。

## Risks / Trade-offs

- **[风险] 授权覆写被误用导致危险工具绕过审批。** → 合同明示 handler 责任；H3 journal 会记录生效授权，审计可追溯。
- **[权衡] `Option<Option<..>>` 语义用 `AuthorizationOverride` 枚举表达而非裸嵌套。** → 读性优先，alpha 期枚举可演进。

## Migration Plan

实现已随 PR #38 完成（alpha 前向破坏，无兼容层）。本 change 仅归档追平：更新 canonical spec 后归档。
