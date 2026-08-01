## 1. HookSnapshot 合同

- [x] 1.1 在 `hook_runtime` 定义借用公共信封 `HookSnapshot`（`iteration: u64`、`context: &'a LoopContext`、`usage: Option<TokenUsage>`），`#[non_exhaustive]` 并实现 `Debug`、`Clone`、`Copy`，补齐公共 API 文档
- [x] 1.2 五个 Hook 输入结构嵌入 `snapshot: HookSnapshot<'a>` 并移除散装的 `iteration` / `context` 字段，专属载荷（`tool_call`、`tool`、`result`）保留在各输入中
- [x] 1.3 `NoopHookRuntime`、crate 公共导出与文档注释前向切换，文档中钉死各点 `snapshot.context` 语义（transform_context 含待消费 Inject；after_tool_call 不含未提交 result）

## 2. AgentLoop 快照构造与 usage 累计

- [x] 2.1 kernel 在每次模型响应后累计 `TokenUsage`（provider 未上报则保持 `None`），构造快照时读取截至该边界的累计值
- [x] 2.2 五个 Hook 调用点按各自边界语义构造快照：transform_context 用 request view 基底（committed + 待消费 Inject），Tool Hook 用当前 committed context，prepare_next_turn 用含本 cycle 全部结果的 committed context
- [x] 2.3 确认快照构造零分配：复用既有 `Cow<LoopContext>` request view 与 committed context 引用，不为快照 clone 历史

## 3. 测试

- [x] 3.1 recording Hook Runtime 与全部既有 hook 测试前向切换到信封输入，No-op 等价性与五点调用顺序断言保持不变
- [x] 3.2 快照语义断言：各点 `snapshot.context` 内容符合边界定义（transform 含 Inject、Tool Hook 含已提交结果、after 不含未提交 result、prepare 含全部结果）
- [x] 3.3 usage 断言：provider 上报时快照携带累计值，未上报时为 `None`
- [x] 3.4 结构性验收：以编译期测试或字段继承断言证明新增公共字段只改 `HookSnapshot` 即可被五个输入继承

## 4. 文档、质量门禁与校验

- [x] 4.1 更新 `crates/stratum-agent/AGENTS.md`：归档 `HookSnapshot`、逐点 context 语义与"宽读窄写"原则
- [x] 4.2 勾选 `TODO.md` 的 H2.5 条目
- [x] 4.3 运行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`，修复本 change 引入的失败
- [x] 4.4 运行 `openspec validate add-hook-input-envelope --type change --strict --no-interactive` 与 `openspec validate --all --strict`
