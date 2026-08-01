# Stratum 开发路线图

> 文档状态：讨论草案
>
> 架构总览：[`ARCH.md`](ARCH.md)

本文只记录开发节奏、依赖关系、验收条件和延后事项。总体技术架构见 [`ARCH.md`](ARCH.md)。

## 1. 执行原则

- 每个阶段先冻结依赖它的公共合同，再进入实现。
- 优先交付可运行、可持久化、可恢复的纵向切片。
- 公共协议必须覆盖兼容性、非法输入和错误脱敏测试。
- 涉及持久化的能力必须在同一阶段提供进程重启与 Resume 测试。
- 涉及代码执行、网络、文件或 Secret 的能力必须同时完成安全边界。
- 每项实现使用独立 worktree 和非 `main` 分支。
- PR 合入前，将最终实现约定归档到相关 crate 的 `AGENTS.md`。

## 2. 总体依赖

```mermaid
flowchart LR
    M0["M0：身份与协议基线"] --> H1["H1：Hook 核心合同"]
    H1 --> H2["H2：有序 Runner 与工具校验"]
    H2 --> H25["H2.5：Hook 输入公共信封"]
    H25 --> H3["H3：Hook 存储与恢复"]
    H3 --> H4["H4：Tool 幂等与恢复"]
    H3 --> H5["H5：上下文压缩"]
    H3 --> S2["S2：Deno / Python 扩展宿主"]
    H3 --> R3["R3：Rust SDK 与 Hook Service"]

    M0 --> W1["W1：工作流图与运行状态"]
    W1 --> W2["W2：持久化队列引擎"]
    W2 --> W3["W3：Agent Node 与运行 API"]
    W3 --> W4["W4：可视化编排界面"]

    H1 --> S1["S1：运行时 Skill"]
    S1 --> W3

    M0 --> P1["P1：存储、安全与可观测合同"]
    P1 --> H3
    P1 --> W2
```

M0 完成后，Agent DIY、Workflow 和平台基础三条线可以并行。语言运行时和远程 Hook Service 必须等待 H3。

## 3. 开发节奏

| 工作线 | 主要内容 | 启动条件 |
|---|---|---|
| A：Agent DIY | Hook、Skill、脚本扩展、Rust SDK | M0 完成 |
| B：Workflow | 图模型、调度器、节点、工作流 API | M0 完成 |
| C：平台基础 | 存储、安全、可观测性、运维 | M0 期间即可启动 |

每个迭代结束时完成：

1. 运行受影响 crate 的单元、集成、异常输入和恢复测试；
2. 通过公共 API 演示一个端到端场景；
3. 检查持久化记录、事件、日志和错误是否泄露敏感信息；
4. 将已经确认的架构决策同步到对应详细设计；
5. 更新相关 crate 的 `AGENTS.md`。

## 4. M0：身份与协议基线

**目标：** 在新增公共 API 和持久化格式前统一运行身份与版本模型。

- [x] 确认 `SessionId` 表示长期、共享且与 Workflow 图无关的协作空间。
- [x] 确认 Agent 可以直接在 Session 中运行，也可以作为带类型化 location 的 Workflow 节点运行；直接对话不是隐式 Workflow。
- [x] 定义 Agent 接收 `AgentRuntimeContext`、只创建 `TurnId` 的接口。
- [x] 定义 `HookInvocationId` 的语义地址；本阶段不引入 node execution 或 attempt 身份。
- [x] 定义 Agent、Workflow、SkillSet、ExtensionSet 和 Hook Handler 的不可变版本身份。
- [x] 统一 `docs/PROTOCOL.md` 与当前 `StreamEnvelope`、三类 `RuntimeEvent`、Agent `message_seq` 和不透明 `EventCursor`。
- [x] 确认 Hook journal 属于 Session/Turn 执行状态，与 `AgentStore` 对话历史和 EventBus 观察分离。
- [x] 完成 Skill、Script Extension、链接式 Rust Hook 和 Hook Service 的基础信任规则。

**验收条件：**

- [x] 身份、版本固定、Hook 错误和 Resume 语义形成已接受的设计记录。
- [x] Workflow 和 Hook 不依赖旧 `RunId`、`EventSource` 或顶层可选序号语义。
- [x] Beta 数据与载荷直接拒绝，不设计迁移、降级或回滚路径。

## 5. 工作线 A：Agent DIY

### H1：四个核心 Hook 合同

**依赖：** M0。

- [x] 定义 `transform_context` 输入与决策。
- [x] 定义 `before_tool_call` 的继续、修改和阻断决策。
- [x] 定义 `after_tool_call` 的结果替换决策。
- [x] 定义 `prepare_next_turn` 的继续、停止和注入消息决策。
- [x] 提供无处理器时保持现有行为的 No-op Runtime。
- [x] 通过 `AgentLoopBuilder` 注入单一 Hook Runtime。
- [x] 将取消和 Deadline 传入每次 Hook 调用。
- [x] 保持 Hook 决策与 EventBus 观察路径分离。

**验收条件：**

- [x] No-op Runtime 下现有 Agent 测试行为不变。
- [x] 四个 Hook 均有正常、错误、超时和取消测试。
- [x] Block 不执行 Tool，并生成模型可见的类型化结果。

### H2：有序执行器、工具校验与审批

**依赖：** H1。

- [ ] 在 `stratum-tools` 建立统一参数校验边界。
- [x] Hook 前校验模型生成的原始参数（kernel 在 `transform_tool_call` 前完成）。
- [x] transform 相位后重新校验最终参数（`hookify-tool-approval` 已在 kernel 编排中固化；完整 Hook Chain 后的复验随链实现自然继承）。
- [ ] 固化 ExtensionSet 中的处理器顺序。
- [ ] 实现顺序变换、Block 短路和 Stop 短路。
- [x] 决策型 Hook 失败时关闭当前操作并返回类型化错误。
- [x] 将内置授权与用户审批放在最终参数校验之后（审批已 hook 化为 `decide_tool_call` 相位的普通 Handler，`ToolApproval` 边界已移除）。
- [x] 保持本阶段 Tool Call 顺序执行。

**验收条件：**

- [ ] 重启前后处理器顺序一致。
- [x] 审批界面展示的参数与实际执行参数一致（由相位顺序结构性保证：decide 只接收复验后的最终参数，且 decide 不允许修改参数）。
- [x] Hook 修改后的非法参数不会进入审批或 Tool 执行（transform 结果必须过复验）。

### H2.5：Hook 输入公共信封

**依赖：** H1。**阻塞：** H3 输入摘要冻结、S1 handler 编写、S2 协议冻结（破坏性合同修订，越晚越贵）。

- [x] 定义借用公共信封 `HookSnapshot`（`iteration`、`&LoopContext`、`Option<TokenUsage>`，未来可扩展工具列表与预算），嵌入全部五个 Hook 输入。
- [x] 逐点钉死 `snapshot.context` 语义：该 Hook 边界时刻的 committed context；`transform_context` 含待消费 Inject；`after_tool_call` 不含未提交的当前 result。
- [x] 保持宽读窄写：decision 词汇不变，公共信息只读。
- [x] `after_tool_call` 经由信封获得完整历史，使结果级压缩等内容感知决策可行。
- [x] 同步 No-op、公共导出、recording 测试基建与全部 hook 测试。

**验收条件：**

- [x] 新增公共字段只改 `HookSnapshot` 一处即可被全部 Hook 点继承（以一次模拟新增字段验证）。
- [x] 信封化后所有既有 hook 行为测试保持不变。

### H3：Hook 存储与恢复

**依赖：** H2、P1。

- [ ] 等实际 Hook 能运行后，再确定 Hook 记录的存储后端和目录结构。
- [ ] Hook 记录归 Session/Turn，不写进 Agent 消息或 EventBus。
- [ ] 固定 ExtensionSet 和 Handler 版本与顺序。
- [ ] 保存每次 Hook 的 ID、输入摘要、决定和最终状态。
- [ ] 调用 Handler 前先保存 Pending，应用决定前先保存 Completed。
- [ ] Resume 时复用已经保存的决定；记录不匹配时停止。
- [ ] 重试 Pending Hook 时复用原来的 `HookInvocationId`。
- [ ] Tool 执行前保存最终参数或 Block 决定。
- [ ] Tool Message 提交前保存 `after_tool_call` 结果。
- [ ] 进入下一次迭代前保存 Stop/Inject 决定。
- [ ] 定义记录大小、保留时间和清理方式。

**验收条件：**

- [ ] 在每个保存边界模拟崩溃后都能正确恢复。
- [ ] 恢复时不会重新执行已经完成的 Hook。
- [ ] Extension 更新不会改变正在运行或恢复的 Turn。

### H4：Tool 幂等与恢复

**依赖：** H3。

- [ ] Hook 存储稳定后，再设计 Tool 的幂等规则。
- [ ] 为一次 Tool 执行定义稳定的幂等键。
- [ ] 保存 Tool 开始执行和执行结果。
- [ ] Resume 时复用已经保存的 Tool 结果。
- [ ] Tool 状态不确定时停止，不直接重复执行。

**验收条件：**

- [ ] 进程崩溃不会让 Tool 被静默执行两次。
- [ ] 无法确认 Tool 结果时 Fail Closed。

### H5：上下文压缩

**依赖：** H2.5、H3。

- [ ] 结果级压缩：确认由 `after_tool_call::ReplaceResult` 覆盖（唯一带写回语义的 decision，压缩结果直接耐久提交）；明确原始结果从 transcript 消失的审计权衡，原始留存依赖 H3 journal 或 handler 私有通道。
- [ ] `prepare_next_turn` 新增 `Compact` 意图 decision：hook 只表达"该压了"，不触碰历史，写回由 kernel 代执行。
- [ ] kernel 在迭代边界执行压缩：强制 tool_call/tool_result 配对完整、system prompt 保留、摘要使用 kernel 归属的归因标记，不得伪装成用户或助手消息。
- [ ] 压缩后的 transcript 成为新的 durable 基线（新增 transcript 改写类耐久事件），resume 从压缩基线恢复。
- [ ] 决定摘要算力归属：kernel 注入的 summarizer 边界（版本可固定在 runtime snapshot）还是 handler 自带 provider。
- [ ] 压缩触发依据来自 `HookSnapshot.usage`，阈值策略由组合侧配置。

**验收条件：**

- [ ] 压缩后 resume 重建的历史与压缩基线一致。
- [ ] 任何压缩结果都不切断 tool_call/tool_result 配对，切割点只在迭代边界。
- [ ] 崩溃于压缩提交前时恢复为未压缩基线（fail-safe），不重复执行已完成的工作。

### S1：第一档运行时 Skill

**依赖：** H1 的 `transform_context`、M0 的制品版本模型。

**可并行：** H2、H3。

- [ ] 定义 Skill 元数据、`SKILL.md`、可选资源和能力声明。
- [ ] 定义 `SkillId`、不可变版本或内容摘要、SkillSet 加载顺序。
- [ ] 发布时校验路径、大小、编码、重复 ID 和元数据。
- [ ] 在 Agent/Turn runtime snapshot 中固定 SkillSet。
- [ ] 为通用 Agent 提供受信任的 Skill 上下文处理器。
- [ ] Skill 只能使用已授权 Tool，不能自行扩权。
- [ ] API 和 Web 支持发布 Skill 并挂载到通用 Agent。
- [ ] 评估 `transform_context` 扩展：当前输入只有 `LoopContext`（system prompt + messages），不含工具 schema（schema 在构造 `ChatRequest` 时才注入）。若 Skill 需要按迭代动态裁剪工具列表，需让 `transform_context` 可见甚至可替换 `Vec<ToolSpec>`；只读可见则加一个 `tools: &[ToolSpec]` 借用字段即可。注意三个 Tool Hook 的 `ToolHookTarget.spec` 已携带单个工具的 schema，审批/校验路径不受影响。

**纵向切片：**

- [ ] 发布 Skill → 挂载通用 Agent → 执行 Turn → 重启 → Resume，并确认 Skill 版本不变。

### S2：第二档 Deno/Python 扩展宿主

**依赖：** H3、扩展宿主威胁模型。

- [ ] 定义一套语言无关的版本化 Hook Wire Protocol。
- [ ] 默认在 Agent 进程外运行 Extension Host。
- [ ] 加载前校验制品摘要和协议兼容性。
- [ ] 限制运行时间、内存、输出、文件和网络权限。
- [ ] 贯穿取消、Deadline、健康检查和有界并发。
- [ ] 建立语言无关的一致性测试套件。
- [ ] 按首个真实使用场景选择 Deno 或 Python 作为第一适配器。
- [ ] 第二适配器复用同一协议和测试套件。
- [ ] 区分受信任本地模式与隔离生产模式。

### R3：第三档 Rust SDK 与 Hook 服务

**依赖：** H3；公共协议已被至少一个 Script Adapter 验证。

- [ ] 定义不暴露 Agent 内部类型的公共 Rust SDK。
- [ ] 支持在应用组合根注册链接式受信任 Hook。
- [ ] 提供基于同一协议的 Hook Service Server Helper。
- [ ] 定义服务身份、版本、Endpoint、能力、作用域和健康状态。
- [ ] 在 Turn 启动时固定解析后的服务版本与 Endpoint 集合。
- [ ] 使用认证传输和租户/项目级授权。
- [ ] 提供 SDK 合同测试与参考 Hook Service。

## 6. 工作线 B：Workflow 编排

### W1：图模型、编译器与运行状态

**依赖：** M0。

- [ ] 定义版本化 `WorkflowDefinition` 与不可变 `ExecutionPlan`。
- [ ] 定义节点输入、输出、边和变量引用。
- [ ] 校验起止节点、可达性、重复 ID、缺失引用、端口兼容和非法环。
- [ ] 定义 Workflow、Node 和 Wait 状态；并发与重试出现前不定义 Attempt 身份。
- [ ] 定义变量池的类型与大小限制。
- [ ] 定义原子状态转换与持久化错误格式。
- [ ] 为图解析、校验和调度不变量添加 Property Test。

**首批节点：**

- [ ] Start
- [ ] End/Answer
- [ ] Agent
- [ ] Tool
- [ ] Condition
- [ ] 简单数据变换

### W2：持久化队列引擎

**依赖：** W1、P1。

- [ ] 从持久化依赖状态构建有界 Ready Queue。
- [ ] 使用 `JoinSet` 管理有界并发节点。
- [ ] 下游节点入队前原子提交节点输出和终态。
- [ ] 由 Workflow Coordinator 分配确定的状态与事件顺序。
- [ ] 重启后从持久化状态重建 Ready Queue。
- [ ] 实现持久化暂停、恢复和取消命令。
- [ ] 增加 Workflow/Node Deadline 与最大执行步数。
- [ ] 在幂等与 Attempt 身份明确后再加入 Retry。

**验收条件：**

- [ ] 线性图、条件分支和有界并行分支均支持重启恢复。
- [ ] 排队中和执行中的节点均可取消。
- [ ] 重复命令保持幂等。
- [ ] 损坏的 Workflow/Graph 状态 Fail Closed。

### W3：Agent 节点、事件与运行 API

**依赖：** W2、S1。

- [ ] Workflow Runtime 接收长期 `SessionId`，并固定其 `WorkflowVersionId`。
- [ ] 将 `AgentLocation::WorkflowNode` 传入 Agent Runtime。
- [ ] 将工作流变量适配为 Agent Turn 输入。
- [ ] 将 Agent 输出投影到变量池。
- [ ] 在 Session stream 中保留 Workflow version、Node、Agent、Turn、LLM 和 Tool 身份。
- [ ] 提供 Workflow 创建、发布、运行、查询、取消和恢复 API。
- [ ] 提供 Session 事件重放与 Workflow 持久化状态读取。
- [ ] 保持直接 Agent 路径独立于 Workflow 图，同时复用 Session、事件和 snapshot 合同。

**纵向切片：**

- [ ] Start → 带 Skill 的 Agent → Condition → End，覆盖 SSE、取消、重启和 Resume。

### W4：可视化编排界面

**依赖：** W1 的稳定图 Schema 与 API。

**可并行：** W2、W3。

- [ ] 使用版本化 Fixture 构建 Workflow Editor。
- [ ] 前后端同时校验连线与端口类型。
- [ ] 区分可编辑草稿与不可变发布版本。
- [ ] 展示节点运行状态和渐进式执行详情。
- [ ] 提供测试运行输入与结果视图。
- [ ] 仅在后端具备持久化语义后开放暂停、取消和恢复控制。

## 7. 工作线 C：平台基础

### P1：存储合同

- [ ] 分离 Agent 工作区、运行状态和不可变制品的逻辑接口。
- [ ] 定义生产环境跨进程 CAS 要求。
- [ ] 定义租户/项目分区与授权边界。
- [ ] 区分需要关系查询的记录与 Blob/Object 制品。
- [ ] 定义事件、历史、Hook 执行日志和制品的保留策略。
- [ ] 为不确定写入、重试和故障恢复提供注入测试。

### P2：安全与治理

- [ ] 在接入层加入认证与租户/项目身份。
- [ ] 定义作者、发布者、执行者、审批者、查看者和管理员权限。
- [ ] 定义 Secret 引用与执行时解析。
- [ ] 定义制品摘要、签名和来源记录。
- [ ] 定义 Extension 能力声明与执行策略。
- [ ] 审计网络、文件、Tool、模型和 Secret 访问。
- [ ] 在工作流等待跨进程前实现持久化审批。
- [ ] 加入速率、并发、CPU、内存、时长和输出配额。

### P3：可观测性

- [ ] 固化 Session → 可选 Workflow Node → Agent Turn → LLM/Tool/Hook 的 Span 层级。
- [ ] 记录 Hook 延迟、错误、超时、Block 和日志命中指标。
- [ ] 记录队列深度、就绪等待、节点耗时、恢复和 Retry 指标。
- [ ] 记录模型用量与 Tool 结果分类，不记录敏感载荷。
- [ ] 为发布、注册、审批和高权限执行提供审计记录。
- [ ] 在应用组合根接入 OpenTelemetry Exporter。

### P4：可靠性与运维

- [ ] 为 API、Scheduler、EventBus、Store 和 Extension Worker 提供健康检查。
- [ ] 为排队节点、运行节点和 Hook Invocation 实现优雅关闭。
- [ ] 验证定义、状态、执行日志和制品的备份恢复。
- [ ] 在首个生产持久化格式变更前建立数据迁移机制。
- [ ] 在所有持久化边界增加进程终止故障测试。
- [ ] 校验部署中的协议和 SDK 版本兼容性。

## 8. 并行与串行关系

### 可以并行

- H1 与 W1：M0 完成后分别推进。
- S1 与 H2/H3：`transform_context` 合同冻结后推进。
- P1/P2 与 Hook、Workflow 合同设计同步推进。
- W4 与 W2/W3：图 Schema 冻结后使用 Fixture 开发。
- Deno 与 Python Adapter：共享协议和一致性测试冻结后并行。
- 各工作线的可观测性：身份和 Span 命名冻结后同步接入。

### 必须串行

- Tool 参数修改 → 最终校验 → 用户审批 → Tool 执行。
- Hook 输入公共信封（H2.5）冻结后，才能冻结 H3 输入摘要与 S2 Hook Wire Protocol。
- Hook 存储和版本固定完成后，才能接入 Script/Rust 远程执行。
- Hook 存储完成后，才能实现 Tool 幂等与恢复。
- 上下文压缩只能在迭代边界切割，禁止在 tool cycle 中间改写历史。
- Session 与版本身份基线确定后，才能固化 Workflow 持久化协议。
- 节点状态可持久化后，才能实现 Queue Resume。
- 出现真实并发/重试需求并明确从属操作幂等语义后，才能加入 Loop 与 Retry 身份。
- 隔离和能力模型通过评审后，才能执行不受信任的 Script Extension。

## 9. 延后事项

- Rust 动态共享库加载；
- 运行中的 Skill 或 Extension 热替换；
- Extension Marketplace 与自动远程安装；
- Command、Renderer、Shortcut 和任意 UI Plugin Registry；
- 并发 Tool 执行；
- 运行期间动态修改共享 Tool Registry；
- 模型主动安装 Skill；
- W2 验证前的 Loop、Iteration 和 Subgraph；
- 多地域 Workflow 迁移；
- 未出现真实调用方前的通用远程文件系统和挂载路由。
