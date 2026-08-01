## 1. stratum-core：journal 事件与 decision 记录

- [x] 1.1 `DurableAgentEvent` 新增 `HookInvocationPending` / `HookInvocationCompleted` / `HookInvocationFailed` 三个变体（snake_case 稳定序列化 + `event_type()` 投影），地址形状为 `(iteration, HookPoint, Option<CallId>)`
- [x] 1.2 为五个 Hook decision 定义 tagged serde 表示 `HookDecisionRecord`（全部内联小载荷，无溢出存储形式）
- [x] 1.3 修正 usage 文档语义：`IterationCompleted`/`LoopFinished`/`LoopFailed`/`LoopCancelled` 的 usage 字段从"累计"改为"最近一次模型响应上报"（字段名不变）

## 2. stratum-agent：usage 修正、patch 与 journal 写入点

- [x] 2.1 `HookSnapshot.usage` 改为最近一次响应上报值：kernel 移除 run 级累计器，模型响应时更新 `latest_usage`；快照构造读取该值；`HookSnapshot` 与 Hook 合同文档同步
- [x] 2.2 `TransformContextDecision::Replace` 改为 `Patch(ContextPatch)`（`ReplaceSystemPrompt` / `DropHistory { upto }` / `RewriteHistory { upto, summary }`）；kernel 将 patch 应用到 request view 并校验 `upto` 不越界、落消息边界、不切断 tool_call/result 配对，非法判 `HookFailure::InvalidOutput`（0-based 下标、左闭右开）
- [x] 2.3 `execute_hook` helper 接入 journal：调用前提交 `HookInvocationPending`（同一逻辑调用重试复用原 invocation id），decision 校验通过后、应用受影响动作前提交 `HookInvocationCompleted`，类型化失败提交 `HookInvocationFailed`
- [x] 2.4 载荷级 digest：Tool Hook 对 canonical `ToolCall` sha256；`transform_context`/`prepare_next_turn` 以 `(iteration, point)` 地址为 digest；usage 与历史不参与
- [x] 2.5 受影响动作边界对齐：decide 的 Completed 先于 `ToolExecutionStarted`，after 的先于 result commit，prepare 的先于迭代边界，transform 的先于模型请求

## 3. stratum-agent：resume 重建

- [x] 3.1 实现事件流重放：`MessageAppended` 序列重建 committed context，最大 `IterationCompleted` 定迭代前沿，终态事件拒绝 resume，组合方重新提供 system prompt 与配置
- [x] 3.2 Tool 结果对账：committed result 必须是前序 assistant `tool_calls` 精确有序前缀，未知/重复/稀疏/乱序 fail closed，缺失后缀重跑；`ToolExecutionStarted` 后崩溃按未知结果重跑该 Tool
- [x] 3.3 resume 路径的 Hook 查表：digest 匹配的 Completed 复用、Pending 原身份重试、Failed 重现、不匹配 fail closed

## 4. stratum-infra：filesystem 后端

- [x] 4.1 `FilesystemDurableEventSink`：per-run 目录、events.jsonl 追加写 + fsync、线程/任务安全
- [x] 4.2 事件读取器：逐行解析、容忍截断尾行、返回完整事件序列

## 5. 测试

- [x] 5.1 journal 写入顺序：五个 Hook 点各自 Pending→Completed/Failed 的相对顺序与受影响动作的先后（Completed 必先于动作）
- [x] 5.2 usage 语义：快照携带最近一次上报值；混合上报与从不上报；kernel 无累计
- [x] 5.3 patch 语义与校验：三种 patch 应用到 request view 的结果、越界/切断配对判 InvalidOutput、不写回 committed、不出现在 new_messages
- [x] 5.4 resume 矩阵：在 Hook 前后、started 前后、result commit 前后、迭代边界分别模拟崩溃重启，断言事件流、decision/patch 回放与终态一致；审批 Completed 后恢复不再问人
- [x] 5.5 digest：匹配复用、不匹配 fail closed、Pending 原身份重试、Failed 重现
- [x] 5.6 对账：缺失后缀重跑、乱序 fail closed、started 后崩溃重跑 Tool
- [x] 5.7 filesystem 后端：追加写可重读、截断尾行容忍

## 6. 文档、质量门禁与校验

- [x] 6.1 归档 `crates/stratum-agent/AGENTS.md`（journal 写入点、resume 前沿、usage 语义、patch 合同）与 `crates/stratum-infra/AGENTS.md`（filesystem 后端布局）
- [x] 6.2 更新 `TODO.md`：H3 已完成条目勾选，sqlite 拆为 H3b 子阶段
- [x] 6.3 运行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`
- [x] 6.4 运行 `openspec validate implement-hook-journal-resume --type change --strict --no-interactive` 与 `openspec validate --all --strict`
