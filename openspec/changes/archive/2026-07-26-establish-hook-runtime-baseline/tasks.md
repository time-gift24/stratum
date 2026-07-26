## 1. 核心身份与协议类型

- [x] 1.1 在 `stratum-core` 中增加 UUIDv7 `SessionId` 与 `HookInvocationId` newtype，并为 Agent、Workflow、SkillSet、ExtensionSet 和 Hook Handler 增加不同的不可变版本 newtype；实现解析、serde、常用 trait 和无效输入测试。
- [x] 1.2 增加类型化的 `AgentLocation` 和不可变的 `AgentRuntimeContext` 值，用于 Agent 直接执行与作为 Workflow 节点执行；不增加 node activation 或 attempt 身份。
- [x] 1.3 在 `StreamEnvelope` 中以 `SessionId` 替换 `RunId` 与 `EventSource`，删除顶层可选消息序号，并将 `RuntimeEvent` 重构为类型化的 Session、Node 和 Agent 事件族，各自携带必需身份。
- [x] 1.4 将归 Agent 所有的 LLM、approval、plan、lifecycle 和 message 事件移入 Agent 事件族；使已提交 `AgentEvent::Message` 必须携带 `message_seq: u64`，使其他事件无法携带消息序号；删除未使用的 run 与顶层 LLM 变体，并增加严格的 serde/wire shape 测试。

## 2. Agent Runtime Context 与 Session 语义

- [x] 2.1 修改 Agent Turn 入口，使调用方提供 `AgentRuntimeContext`，Agent 只创建 `TurnId`，恢复时保留原有 Session、Turn 与 location。
- [x] 2.2 更新 API host 与 REPL composition root，使其创建或恢复长期存在的 Session 身份，并将其传入每个直接运行的 Agent Turn。
- [x] 2.3 强制执行当前每个 Session 仅一个活跃操作的不变量，不引入通用 execution manager；测试冲突启动无法替换活跃状态。
- [x] 2.4 增加测试，证明后续 Turn 可以复用同一 Session，而不同 Agent 身份不会隐式共享对话历史。

## 3. 持久化 Turn 基线

- [x] 3.1 提升严格的 Agent state schema 版本，以 Session 身份与 Agent location 替换持久化 run 身份；拒绝并删除不受支持的 beta state，不增加双读、离线转换或回滚兼容代码。
- [x] 3.2 定义并持久化可恢复 Turn runtime snapshot，其中包含 Agent version、已解析的 `ModelConfig`、ToolSet fingerprint、SkillSet version、ExtensionSet version 和有序 Handler version。
- [x] 3.3 在恢复前校验固定的 snapshot；当固定 component 或 fingerprint 不可用或不匹配时，在 model、Tool 或未来 Hook 工作开始前 fail closed。
- [x] 3.4 更新 filesystem-store fixture 与重启测试，覆盖一个 Session 中的多个 Turn、location 保持、严格拒绝旧数据和不可变 snapshot 恢复。

## 4. Session 作用域事件与 Agent 历史

- [x] 4.1 将 `AgentStore` append 输入与已提交消息事件分型：append 输入不含序号，Store 分配下一个 `message_seq` 并返回可发布的已提交 `AgentEvent::Message`；保持其作用域为 `(AgentId, message_seq)`，不以 `Option` 表示提交阶段。
- [x] 4.2 以 Session 作用域的发布与订阅 API 替换 Agent 作用域的 EventBus 订阅 API，并接收 Session、Node 和 Agent 事件族。
- [x] 4.3 更新内存与 NATS 实现，按 `SessionId` 分区；覆盖 Session subject、replay、cursor 到期以及多 Agent Session stream 测试。
- [x] 4.4 保持 `EventCursor` 为不透明传输位置，并增加测试证明它独立于完整消息的 `message_seq` 和持久化恢复状态。
- [x] 4.5 更新 fixed-barrier history recovery，使用 `(AgentId, message_seq)` 对来自多个 Agent 的实时 Session 消息进行排序、过滤与去重，不把 `message_seq` 当作 Session 全局序号。

## 5. API、SSE 与 Web 投影

- [x] 5.1 更新 HTTP request/response 投影，分别暴露 Session 与 Agent 身份，并通过对应 Session context 解析现有 Agent message 操作。
- [x] 5.2 将保留式 SSE 订阅与重连行为改为使用 Session 身份，同时保留明确的 cursor 过期错误和安全的结构化 tracing 字段。
- [x] 5.3 重新生成或更新前端协议类型与 reducer，以适配新的 envelope 和事件族，包括 Agent direct 与 Workflow-node location 的差异。
- [x] 5.4 增加 API 纵向测试，并按 `stratum-web/AGENTS.md` 的无前端测试文件策略运行前端 typecheck、空测试集与生产构建；覆盖一个 Session 中的多个 Turn、同一 Session stream 中的多 Agent 事件、Agent 历史隔离、取消/恢复以及不受支持的旧载荷。

## 6. Hook 执行契约基线

- [x] 6.1 定义四个 Hook point 身份，以及将 Session、Agent、Turn、Hook point、Handler 位置/版本、operation identity 与 input digest 绑定的 Hook invocation 语义地址。
- [x] 6.2 为 pending、completed、failed、timed-out、cancelled、version mismatch、input mismatch、invalid output 和 unavailable pinned Handler 定义类型化 Hook invocation 状态与失败；不增加 journal 后端。
- [x] 6.3 增加单元测试，覆盖每个 Handler 不同的 invocation 身份、稳定的 pending 重试身份、completed result 复用校验、终态失败保持和不匹配时的 fail-closed 行为。
- [x] 6.4 记录 Hook journal state 属于 Session/Turn 执行状态、与 `AgentStore` 历史和 EventBus 观测保持分离，并将在 H3/P1 获得存储实现。
- [x] 6.5 记录并测试可执行的基线信任规则，覆盖 Skill 权限、Script 隔离描述、链接式 Rust runtime 兼容性、远程 Hook Service 身份/幂等性和敏感错误脱敏。

## 7. 文档与验证

- [x] 7.1 围绕 Session 身份、类型化事件归属、已提交 Agent 消息必填的 `message_seq`、不透明的 `EventCursor`、版本固定和 beta 不兼容性重写 `docs/PROTOCOL.md`，明确本阶段不设计或支持任何迁移、降级与回滚路径。
- [x] 7.2 使 `ARCH.md` 与 `TODO.md` 对齐已接受的 Session 模型，从当前基线删除 `NodeExecutionId` 和 `AttemptId`，并继续推迟 Session 存储与高级 Workflow 行为。
- [x] 7.3 将最终实现不变量归档到每个受影响 crate 的 `AGENTS.md`，包括 PR 合并前必须提醒归档的要求。
- [x] 7.4 运行 `cargo fmt --check`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`、受影响的前端检查/测试，以及 `openspec validate establish-hook-runtime-baseline --type change --strict --no-interactive`。

## 8. 完整纵向测试流程

- [x] 8.1 在 `stratum-api/tests/` 增加 Session runtime 全链路测试入口，使用临时 filesystem store、真实 API router/SSE 投影、内存 EventBus 与可控 mock LLM；测试辅助逻辑仅放在测试代码中，成功结束后清理隔离数据，失败时保留隔离目录供诊断。
- [x] 8.2 用可控 mock LLM 完成确定性主流程：创建一个长期 Session 和 Agent、先建立 Session SSE 订阅、提交第一个直接运行的 Turn、等待终态、读取 Agent 历史，并验证所有事件共享同一 `SessionId`、Agent 事件携带正确的 `AgentId`/`TurnId`/`AgentLocation::Direct`、已提交消息的 `message_seq` 连续且历史与实时事件按 `(AgentId, message_seq)` 合并后不重复。
- [x] 8.3 在同一主流程中重建 host 以模拟进程重启，使用原 Session 和 Agent 提交第二个 Turn；验证 `SessionId` 保持不变、生成新的 `TurnId`、原 Turn snapshot 可精确恢复、Agent 对话历史连续，并验证从保存的 `EventCursor` 重连只影响传输重放位置而不参与业务恢复。
- [x] 8.4 扩展确定性流程，依次验证同一 Session 的并发启动被拒绝且不覆盖活跃操作、第二个 Agent 在操作串行结束后可进入同一 Session 但拥有独立对话历史、一个 Session stream 可观测两个 Agent 的类型化事件，以及旧 `run_id`/`source` 载荷在 API 与持久化边界均被明确拒绝。
- [x] 8.5 增加默认 `#[ignore]` 的真实 DeepSeek 纵向验收测试：仅从 `DEEPSEEK_API_KEY` 读取凭据，使用仓库支持的默认 DeepSeek model，在隔离临时目录中通过真实 API composition 完成“创建 Session 与 Agent → 建立 SSE → Turn 1 完成 → 重建 host → 同一 Session 的 Turn 2 完成 → 查询历史与事件”的流程；只断言协议、身份、终态、持久化和恢复不变量，不断言模型回答文本。
- [x] 8.6 为真实 DeepSeek 测试提供单一、明确的本地命令（优先使用对应 crate 的 `Makefile` target）；命令继承 zsh 中已导出的 `DEEPSEEK_API_KEY`，不得把密钥写入参数、配置文件、fixture、快照、日志或测试失败输出，并断言临时持久化文件不含该密钥。
- [x] 8.7 分别运行确定性全链路测试和 opt-in DeepSeek 测试，记录使用的非敏感 model identity、两个 Turn 的 Session/Turn 关系、重启恢复结果、SSE 重连结果与历史去重结果；任何失败均必须保留隔离测试目录的路径以供诊断，但不得保留或打印 API key。
