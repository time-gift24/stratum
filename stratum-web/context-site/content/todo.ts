import type { TodoInitiative, TodoLedger } from "./model.ts"

function initiative(value: TodoInitiative): TodoInitiative {
  return value
}

const initiatives: readonly TodoInitiative[] = [
  initiative({
    id: "M0",
    area: "治理历史",
    title: "身份与协议基线",
    status: "已完成",
    goal: "在公共 API 与持久化格式扩展前统一 Session、Agent、Turn、Hook 与不可变版本身份。",
    dependencies: [],
    items: [
      {
        id: "M0.1",
        title: "SessionId 表示长期、与 Workflow 图无关的协作空间",
        status: "已完成",
      },
      {
        id: "M0.2",
        title: "AgentRuntimeContext 与 TurnId 的边界固定",
        status: "已完成",
      },
      {
        id: "M0.3",
        title: "HookInvocationId 与版本身份合同固定",
        status: "已完成",
      },
      {
        id: "M0.4",
        title: "旧 EventBus/message_seq 协议由 Postgres runtime 协议取代",
        status: "已取代",
      },
      {
        id: "M0.5",
        title: "Beta 数据与载荷直接拒绝，不提供迁移/降级",
        status: "已完成",
      },
    ],
    acceptance: [
      "Workflow 与 Hook 不依赖旧 RunId/EventSource",
      "身份与 Resume 语义有 accepted design record",
    ],
  }),
  initiative({
    id: "H1",
    area: "Agent DIY",
    title: "五个核心 Hook 合同",
    status: "已完成",
    goal: "用五个受控 Hook 点表达 context、Tool call、审批、Tool result 与下一迭代意图。",
    dependencies: ["M0"],
    items: [
      { id: "H1.1", title: "transform_context", status: "已完成" },
      { id: "H1.2", title: "transform_tool_call", status: "已完成" },
      { id: "H1.3", title: "decide_tool_call", status: "已完成" },
      { id: "H1.4", title: "after_tool_call", status: "已完成" },
      { id: "H1.5", title: "prepare_next_turn", status: "已完成" },
      {
        id: "H1.6",
        title: "No-op、取消、Deadline 与 telemetry 分离",
        status: "已完成",
      },
    ],
    acceptance: [
      "No-op 行为不变",
      "五个 Hook 有正常/错误/超时/取消测试",
      "Block 不执行 Tool",
    ],
  }),
  initiative({
    id: "H2",
    area: "Agent DIY",
    title: "有序执行器、工具校验与审批",
    status: "已完成",
    goal: "固定 ordered handler chain，并在 Tool 外部动作前完成 schema 校验、变换复验与审批。",
    dependencies: ["H1"],
    items: [
      {
        id: "H2.1",
        title: "Tool schema 编译缓存与原始参数校验",
        status: "已完成",
      },
      { id: "H2.2", title: "transform 后最终参数复验", status: "已完成" },
      {
        id: "H2.3",
        title: "handler 顺序、Block/Stop 短路与 Inject 合并",
        status: "已完成",
      },
      { id: "H2.4", title: "审批展示参数与实际执行参数一致", status: "已完成" },
    ],
    acceptance: [
      "重启前后 handler 顺序一致",
      "非法变换不进入审批或 Tool",
      "Tool Call 仍顺序执行",
    ],
  }),
  initiative({
    id: "H2.5",
    area: "Agent DIY",
    title: "Hook 输入公共信封",
    status: "已完成",
    goal: "所有 Hook 共享借用式 HookSnapshot，实现宽读窄写，并固定每个边界看到的 committed context。",
    dependencies: ["H1"],
    items: [
      {
        id: "H2.5.1",
        title: "HookSnapshot iteration/context/usage",
        status: "已完成",
      },
      {
        id: "H2.5.2",
        title: "五个 Hook 的 context 时点语义",
        status: "已完成",
      },
      {
        id: "H2.5.3",
        title: "after_tool_call 可见完整已提交历史",
        status: "已完成",
      },
    ],
    acceptance: [
      "公共字段只改 HookSnapshot 一处",
      "信封化后 Hook 行为测试不变",
    ],
  }),
  initiative({
    id: "H3a",
    area: "Agent DIY",
    title: "Hook journal 与恢复",
    status: "已完成",
    goal: "Pending/Completed/Failed 与业务 facts 共用 Postgres ledger，resume 重放已完成 decision。",
    dependencies: ["H2"],
    items: [
      {
        id: "H3a.1",
        title: "filesystem per-run journal",
        status: "已取代",
        note: "由统一 Postgres durable ledger 取代",
      },
      {
        id: "H3a.2",
        title: "Pending before call / Completed before effect",
        status: "已完成",
      },
      {
        id: "H3a.3",
        title: "resume 重放 decision 与 input digest fail closed",
        status: "已完成",
      },
      {
        id: "H3a.4",
        title:
          "replay 再次到达同 HookAddress 时，Pending 重试复用 HookInvocationId",
        status: "已完成",
      },
      {
        id: "H3a.5",
        title: "tracing、敏感字段审计与 deny-list 回归",
        status: "已完成",
      },
      {
        id: "H3a.6",
        title: "清理过渡 compatibility layer 与旧 journal 路径",
        status: "已完成",
      },
    ],
    acceptance: [
      "已自动覆盖 decide Pending/Completed、Tool Started/result、prepare Completed、IterationCompleted 与 compaction W1/W2 的既定 crash matrix",
      "已完成 Hook 不重做",
      "Extension 变化不改变旧 Turn",
      "五个 Hook 的穷举 crash-window 扩展归 P4a，不以宽泛表述冒充已完成",
    ],
  }),
  initiative({
    id: "H3b",
    area: "Agent DIY",
    title: "Postgres 统一执行层存储",
    status: "待讨论",
    goal: "Postgres 成为 Agent execution 唯一 truth；四张复数表承载 definition、state、ledger 与 compaction companion。",
    dependencies: ["H3a"],
    items: [
      {
        id: "H3b.1",
        title: "旧 PostgresDurableEventSink/AgentStore 方案",
        status: "已取代",
      },
      {
        id: "H3b.2",
        title: "concrete PostgresBackend + embedded baseline",
        status: "已完成",
      },
      {
        id: "H3b.3",
        title: "AgentRuntime-wide 无空洞 event_seq",
        status: "已完成",
      },
      {
        id: "H3b.4",
        title: "filesystem backend selector 与双后端 replay",
        status: "已取代",
      },
      {
        id: "H3b.5",
        title: "评估 per-handler 粒度 journal",
        status: "待讨论",
        note: "等待真实需求，不提前抽象",
      },
    ],
    acceptance: [
      "agents/agent_states/durable_events/transcript_compactions 四表",
      "PG/NATS/API/Web 真实门禁通过",
    ],
  }),
  initiative({
    id: "H5a",
    area: "Agent DIY",
    title: "上下文压缩机制",
    status: "已完成",
    goal: "提供 ReplaceResult 与迭代边界 Compact mechanism、typed marker、companion、replay 与 crash-window 闭环。",
    dependencies: ["H2.5", "H3a"],
    items: [
      { id: "H5a.1", title: "after_tool_call ReplaceResult", status: "已完成" },
      {
        id: "H5a.2",
        title: "prepare_next_turn Compact decision",
        status: "已完成",
      },
      {
        id: "H5a.3",
        title: "kernel pairing/cut/marker 校验",
        status: "已完成",
      },
      {
        id: "H5a.4",
        title: "TranscriptCompacted + Postgres companion/replay",
        status: "已完成",
      },
      { id: "H5a.5", title: "HookSnapshot 最近一次 usage", status: "已完成" },
    ],
    acceptance: [
      "压缩后 resume 等价",
      "不切断 Tool pair",
      "崩溃窗口不重复已完成工作",
    ],
  }),
  initiative({
    id: "H5b",
    area: "Agent DIY",
    title: "通用生产 Compaction 策略",
    status: "明确延期",
    goal: "用独立 proposal 冻结真实触发、summary、失败、版本演进、成本与质量策略；不得扩大 kernel 职责。",
    dependencies: ["H5a"],
    items: [
      {
        id: "H5b.1",
        title: "普通 Turn / 跨 Turn / Tool cycle 的触发边界",
        status: "待讨论",
      },
      {
        id: "H5b.2",
        title: "usage 度量与 provider 不报 usage 的语义",
        status: "待讨论",
      },
      {
        id: "H5b.3",
        title: "summary provider/model/budget/timeout/cancel/version",
        status: "待讨论",
      },
      {
        id: "H5b.4",
        title: "summary 失败与 durable commit 失败语义",
        status: "待讨论",
      },
      {
        id: "H5b.5",
        title: "cut、滞回、冷却、上限与无收益停止",
        status: "待讨论",
      },
      {
        id: "H5b.6",
        title: "handler chain 演进与旧 Turn resume",
        status: "待讨论",
      },
      {
        id: "H5b.7",
        title: "journal summary 与 companion summary 职责",
        status: "待讨论",
      },
      {
        id: "H5b.8",
        title: "敏感信息、成本、完整性与幻觉门槛",
        status: "待讨论",
      },
    ],
    acceptance: [
      "先形成独立 accepted proposal",
      "stock runtime 注册真实 handler 前完成版本与失败语义审查",
    ],
  }),
  initiative({
    id: "H5c",
    area: "Agent DIY",
    title: "Compaction 产品与故障验收",
    status: "明确延期",
    goal: "生产者证据与消费者证据分离；建立每个 commit/crash window 的 deterministic fixture 与 durable oracle。",
    dependencies: ["H5b", "P4a"],
    items: [
      {
        id: "H5c.1",
        title: "真实 handler → journal → kernel → companion 生产者证据",
        status: "待开始",
      },
      {
        id: "H5c.2",
        title: "seeded durable fixture 的 consumer/fallback/corruption/UI 证据",
        status: "待开始",
      },
      {
        id: "H5c.3",
        title: "journal/companion/commit ack/iteration/restart 单故障矩阵",
        status: "待开始",
      },
      {
        id: "H5c.4",
        title: "证据完成后纳入 Alpha 结束条件",
        status: "受阻",
        note: "依赖 H5b/P4a",
      },
    ],
    acceptance: [
      "direct DB seed 不冒充 producer E2E",
      "每例 fresh fixture + 单一故障 + durable oracle",
    ],
  }),
  initiative({
    id: "S1",
    area: "Agent DIY",
    title: "第一档运行时 Skill",
    status: "待开始",
    goal: "定义 Skill artifact/identity/capability，并以受信任 transform_context handler 接入通用 Agent。",
    dependencies: ["H1", "M0"],
    items: [
      {
        id: "S1.1",
        title: "SKILL.md 元数据、资源与能力声明",
        status: "待开始",
      },
      {
        id: "S1.2",
        title: "SkillId/不可变版本/SkillSet 顺序",
        status: "待开始",
      },
      {
        id: "S1.3",
        title: "路径/大小/编码/重复 ID/元数据发布校验与 runtime snapshot 固定",
        status: "待开始",
      },
      {
        id: "S1.4",
        title: "受信任上下文 handler 与 Tool 权限约束",
        status: "待开始",
      },
      { id: "S1.5", title: "API/Web 发布并挂载 Skill", status: "待开始" },
      {
        id: "S1.6",
        title: "评估 transform_context 可见 ToolSpec",
        status: "待讨论",
        note: "当前 LoopContext 只有 system prompt + messages，Tool schema 只在构造 ChatRequest 时注入。若 Skill 只需读取，优先增加 tools: &[ToolSpec]；若要动态裁剪才讨论替换 Vec<ToolSpec>。transform_tool_call / decide_tool_call / after_tool_call 的 ToolHookTarget 已携单 ToolSpec，不受该缺口影响",
      },
    ],
    acceptance: ["发布 Skill → AgentRuntime → Turn → 重启 → Resume，版本不变"],
  }),
  initiative({
    id: "S2",
    area: "Agent DIY",
    title: "Deno / Python 扩展宿主",
    status: "明确延期",
    goal: "在进程外运行不受信任 Script Extension，受统一版本化 Hook Wire Protocol 与资源权限约束。",
    dependencies: ["H3a"],
    items: [
      { id: "S2.1", title: "语言无关 Hook Wire Protocol", status: "待开始" },
      {
        id: "S2.2",
        title: "进程外 Extension Host 与 artifact digest",
        status: "待开始",
      },
      { id: "S2.3", title: "时间/内存/输出/文件/网络限制", status: "待开始" },
      { id: "S2.4", title: "取消、Deadline、健康与有界并发", status: "待开始" },
      {
        id: "S2.5",
        title: "按首个真实场景选择 Deno 或 Python adapter",
        status: "待讨论",
      },
      {
        id: "S2.6",
        title: "artifact digest 与 Hook Wire Protocol 兼容性校验",
        status: "待开始",
      },
      {
        id: "S2.7",
        title: "语言无关一致性套件与第二 adapter",
        status: "待开始",
      },
    ],
    acceptance: [
      "受信任本地模式与隔离生产模式明确分开",
      "第二 adapter 复用同一协议与测试",
    ],
  }),
  initiative({
    id: "R3",
    area: "Agent DIY",
    title: "Rust SDK 与 Hook Service",
    status: "明确延期",
    goal: "在 Script adapter 验证公共协议后，提供链接式受信任 Hook 与认证远程 Hook Service。",
    dependencies: ["H3a", "S2"],
    items: [
      {
        id: "R3.1",
        title: "不暴露 kernel internals 的 Rust SDK",
        status: "待开始",
      },
      { id: "R3.2", title: "组合根注册链接式 Hook", status: "待开始" },
      {
        id: "R3.3",
        title: "Hook Service helper、identity、version 与 health",
        status: "待开始",
      },
      {
        id: "R3.4",
        title: "Turn 启动时固定 endpoint/capability/scope 集合",
        status: "待开始",
      },
      { id: "R3.5", title: "认证传输与租户授权", status: "待开始" },
    ],
    acceptance: [
      "SDK 合同测试与参考 Hook Service",
      "Turn 启动时固定解析后的 endpoint/version",
    ],
  }),
  initiative({
    id: "W1",
    area: "Workflow",
    title: "图模型、编译器与运行状态",
    status: "待开始",
    goal: "定义不可变 ExecutionPlan、typed ports、变量池和可验证的 Workflow/Node/Wait 状态机。",
    dependencies: ["M0"],
    items: [
      {
        id: "W1.1",
        title: "WorkflowDefinition / ExecutionPlan / edges / values",
        status: "待开始",
      },
      {
        id: "W1.2",
        title: "起止节点/可达性/重复 ID/缺失引用/端口/非法环校验",
        status: "待开始",
      },
      {
        id: "W1.3",
        title: "Start/End/Agent/Tool/Condition/Transform 节点",
        status: "待开始",
      },
      {
        id: "W1.4",
        title: "变量池类型与大小限制",
        status: "待开始",
      },
      {
        id: "W1.5",
        title: "原子状态转换与持久化错误格式",
        status: "待开始",
      },
      { id: "W1.6", title: "graph/state property tests", status: "待开始" },
    ],
    acceptance: [
      "非法图与损坏状态 fail closed",
      "并发/Retry 前不提前定义 Attempt identity",
    ],
  }),
  initiative({
    id: "W2",
    area: "Workflow",
    title: "持久化队列引擎",
    status: "待开始",
    goal: "从 durable dependency state 构建有界 Ready Queue，支持重启、暂停、恢复与取消。",
    dependencies: ["W1", "P1"],
    items: [
      { id: "W2.1", title: "bounded Ready Queue / JoinSet", status: "待开始" },
      {
        id: "W2.2",
        title: "下游入队前原子提交节点 output 与 terminal",
        status: "待开始",
      },
      {
        id: "W2.3",
        title: "Coordinator 分配确定的状态与事件顺序",
        status: "待开始",
      },
      {
        id: "W2.4",
        title: "restart rebuild / durable pause-resume-cancel",
        status: "待开始",
      },
      { id: "W2.5", title: "Deadline、最大步数与未来 Retry", status: "待开始" },
    ],
    acceptance: [
      "线性/条件/有界并行均可重启",
      "queued 与 running 节点分别可取消",
      "重复命令幂等",
      "损坏 graph state fail closed",
    ],
  }),
  initiative({
    id: "W3",
    area: "Workflow",
    title: "Agent 节点、事件与运行 API",
    status: "受阻",
    goal: "把 Agent Turn 接入 Workflow variables 与 Session stream，同时保持直接 Agent 路径独立。",
    dependencies: ["W2", "S1"],
    items: [
      {
        id: "W3.1",
        title:
          "长期 SessionId 输入、WorkflowVersionId pin、AgentLocation 与 variable adapter",
        status: "受阻",
      },
      { id: "W3.2", title: "Agent outcome 投影变量池", status: "受阻" },
      {
        id: "W3.3",
        title: "Workflow CRUD/run/cancel/resume API",
        status: "受阻",
      },
      {
        id: "W3.4",
        title: "Session event replay 与 durable state",
        status: "受阻",
      },
      {
        id: "W3.5",
        title:
          "Session stream 保留 WorkflowVersion/Node/Agent/Turn/LLM/Tool 身份",
        status: "受阻",
      },
      {
        id: "W3.6",
        title: "direct Agent 与 Workflow 复用 Session/event/snapshot 合同",
        status: "受阻",
      },
    ],
    acceptance: [
      "Start → Skill Agent → Condition → End，覆盖 SSE/cancel/restart/resume",
    ],
  }),
  initiative({
    id: "W4",
    area: "Workflow",
    title: "可视化编排界面",
    status: "受阻",
    goal: "在稳定 graph schema/API 上构建版本化 Workflow Editor 与渐进式运行详情。",
    dependencies: ["W1", "W2", "W3"],
    items: [
      { id: "W4.1", title: "版本化 fixture editor", status: "受阻" },
      { id: "W4.2", title: "前后端连线/端口校验", status: "受阻" },
      { id: "W4.3", title: "draft/published 与 runtime state", status: "受阻" },
      {
        id: "W4.4",
        title: "持久语义完成后开放 pause/cancel/resume",
        status: "受阻",
      },
      {
        id: "W4.5",
        title: "测试运行输入与结果视图",
        status: "受阻",
      },
    ],
    acceptance: [
      "编辑与不可变发布版本明确分开",
      "所有控制都有后端 durable 语义",
    ],
  }),
  initiative({
    id: "P1",
    area: "平台基础",
    title: "存储合同",
    status: "待讨论",
    goal: "补齐租户分区、跨进程 CAS、资产分类、retention 与 uncertain-write 故障证据。",
    dependencies: ["M0"],
    items: [
      {
        id: "P1.1",
        title: "工作区/运行状态/immutable artifact 逻辑接口",
        status: "待讨论",
      },
      {
        id: "P1.2",
        title: "跨进程 CAS 与 tenant/project partition",
        status: "待讨论",
      },
      {
        id: "P1.3",
        title: "relational record 与 blob/object asset",
        status: "待讨论",
      },
      {
        id: "P1.4",
        title: "event/history/journal/artifact retention",
        status: "待讨论",
      },
      {
        id: "P1.5",
        title: "uncertain write/retry/fault injection",
        status: "受阻",
        note: "依赖 P4a",
      },
    ],
    acceptance: ["生产持久化边界有可重复的故障恢复证据"],
  }),
  initiative({
    id: "P2",
    area: "平台基础",
    title: "安全与治理",
    status: "待开始",
    goal: "建立认证授权、credential-aware Tool、artifact provenance、配额与安全审计。",
    dependencies: ["P1"],
    items: [
      { id: "P2.1", title: "认证与 tenant/project identity", status: "待开始" },
      {
        id: "P2.2",
        title: "作者/发布/执行/审批/查看/管理权限",
        status: "待开始",
      },
      {
        id: "P2.3",
        title:
          "opaque Secret reference、审批消费后 provider 注入与 fail-closed result transform",
        status: "明确延期",
      },
      {
        id: "P2.4",
        title: "artifact digest/signature/provenance",
        status: "待开始",
      },
      {
        id: "P2.5",
        title: "网络/文件/Tool/模型/Secret 审计与配额",
        status: "待开始",
      },
      {
        id: "P2.6",
        title: "Extension 能力声明与执行策略",
        status: "待开始",
      },
      {
        id: "P2.7",
        title: "Workflow 跨进程等待前的持久化审批",
        status: "待开始",
      },
    ],
    acceptance: [
      "credential-aware Tool 完成前不得注册到 runtime",
      "真实 credential 只在对应审批事实已消费后注入；原始结果未完成 durable-safe transform 时 fail closed",
      "高权限执行可审计且不记录 secret",
    ],
  }),
  initiative({
    id: "P3",
    area: "平台基础",
    title: "可观测性",
    status: "进行中",
    goal: "固定 span hierarchy，并补齐 metrics 与不含敏感 payload 的审计/用量信号。",
    dependencies: ["M0"],
    items: [
      {
        id: "P3.1",
        title:
          "Session → 可选 Workflow Node → Agent Turn → LLM/Tool/Hook spans",
        status: "进行中",
      },
      {
        id: "P3.2",
        title: "Hook latency/error/timeout/block metrics",
        status: "待开始",
      },
      { id: "P3.3", title: "queue/recovery/retry metrics", status: "待开始" },
      {
        id: "P3.4",
        title: "model usage 与 Tool result classification",
        status: "待开始",
      },
      {
        id: "P3.5",
        title: "Hook Block 与日志命中指标",
        status: "待开始",
      },
      { id: "P3.6", title: "发布/注册/审批/高权限审计", status: "待开始" },
    ],
    acceptance: ["label 无高基数 identity", "不记录 prompt/result/credential"],
  }),
  initiative({
    id: "P4",
    area: "平台基础",
    title: "可靠性与运维",
    status: "进行中",
    goal: "补齐 scheduler/worker health、backup/restore、migration 与持久化边界进程终止验证。",
    dependencies: ["P1"],
    items: [
      { id: "P4.1", title: "API health/readiness", status: "已完成" },
      {
        id: "P4.2",
        title: "Scheduler/Extension Worker health",
        status: "受阻",
      },
      {
        id: "P4.3",
        title: "queued node、running node 与 Hook Invocation graceful shutdown",
        status: "受阻",
      },
      {
        id: "P4.4",
        title:
          "definition/state/execution log/artifact backup-restore 与首个 schema migration",
        status: "待开始",
      },
      {
        id: "P4.5",
        title: "所有持久边界 process-termination tests",
        status: "受阻",
      },
    ],
    acceptance: ["部署协议/SDK 兼容性可校验", "备份恢复有真实演练"],
  }),
  initiative({
    id: "P4a",
    area: "平台基础",
    title: "确定性故障测试基建 PATCH",
    status: "明确延期",
    goal: "一次只注入一个故障，使用 test-only fixtures 形成可重复、可机器判定且不泄露 payload 的证据。",
    dependencies: ["P1"],
    items: [
      {
        id: "P4a.0",
        title: "独立 proposal 与 fixture 生命周期协议",
        status: "待开始",
        note: "先冻结一次只注入一个故障、fresh provision → inject → durable oracle → cleanup/rebuild 的可重复机器判定协议，再实现各 fixture",
      },
      {
        id: "P4a.1",
        title: "scripted LLM gateway",
        status: "待开始",
        note: "精确产生 text/tool/usage/error/slow/pending 序列；只暴露调用次数和安全边界信号，不记录 prompt/credential",
      },
      {
        id: "P4a.2",
        title: "observable isolated Tool",
        status: "待开始",
        note: "记录 CallId、调用次数与开始/完成边界，支持副作用前后暂停；不外推生产 Tool 的通用幂等语义",
      },
      {
        id: "P4a.3",
        title: "PG COMMIT/ack uncertainty proxy",
        status: "待开始",
        note: "区分未到 COMMIT、COMMIT 可能已到但 ack 丢失、commit 已确认三个窗口，并可精确切断/恢复",
      },
      {
        id: "P4a.4",
        title: "disposable malformed durable fixture builder",
        status: "待开始",
        note: "仅连接带测试标记的独立 DB；一次构造 missing/malformed companion、非法 pointer、unsupported version/payload、foreign identity 或 high-water gap，保存 oracle 后重建；入口不得编入 production binary",
      },
      {
        id: "P4a.5",
        title: "NATS loss/slow/retention/cursor fixture",
        status: "待开始",
        note: "每例独立 stream，覆盖 unavailable、slow publish、publish loss、bounded queue pressure、retention eviction、cursor expiry 与 generation rebuild",
      },
      {
        id: "P4a.6",
        title: "exact process controller 与 SIGTERM matrix",
        status: "待开始",
        note: "按 exact PID/container 启动、等待边界、暂停、终止并复用同一 PG/NATS 重启；禁止宽泛进程匹配或真实 provider 偶然慢响应",
      },
      {
        id: "P4a.7",
        title: "五个 Hook 的 Pending/Completed/effect crash-window 矩阵",
        status: "待开始",
        note: "覆盖被重试的 Pending、recording-path cancellation 的永久 Pending-only，以及各 decision continuation；不得把 Compact W1/W2 外推为通用效果补齐",
      },
      {
        id: "P4a.8",
        title: "安全 evidence collector",
        status: "待开始",
        note: "只记录 identity、event type/version/sequence、high-water、HTTP status/error code、cursor 与用户可见结果；输出可比对并可关联 CI",
      },
      {
        id: "P4a.9",
        title: "禁止 production failpoint/debug endpoint/second truth",
        status: "待开始",
        note: "任何测试控制面都不得成为绕过 auth/approval/capability/secret 等安全边界的管理入口；只存在于 test binary、proxy 或隔离容器 fixture",
      },
    ],
    acceptance: [
      "fresh fixture + single fault + durable oracle",
      "SIGTERM 分别证明 drain 内唯一 terminal 与重启后 running+unhosted 两种线性化结果",
      "不得伪造 cancellation；explicit resume 沿用原 identity，且不产生第二个 LoopStarted 或 terminal",
      "不记录 prompt/Tool payload/summary/provider body/secret",
    ],
  }),
]

export const todoLedger: TodoLedger = {
  title: "Stratum 工程待办",
  principles: [
    "先冻结公共合同，再进入实现；优先交付可运行、可持久化、可恢复的纵向切片。",
    "公共协议覆盖兼容性、非法输入与错误脱敏；持久化能力同期提供 crash/resume 证据。",
    "代码、网络、文件或 Secret 能力必须同期完成安全边界。",
    "每项实现使用独立 worktree 与非 main 分支；合入前归档对应 crate AGENTS.md。",
    "每个阶段完成前运行受影响单元/集成/异常/恢复测试，以公共 API 演示纵向场景，并检查 durable records、事件、日志与错误不泄露敏感信息。",
    "已经确认的架构决策同步到相关详细设计与 crate AGENTS.md；实现状态和验收证据必须同时更新。",
    "经 grilling 明确确认的待办才进入本 ledger；访谈草稿与临时推理不落盘。",
  ],
  coordination: {
    parallelTracks: [
      "Agent DIY 主线与 Workflow 主线可以并行；二者只通过已冻结的身份、事件与 Hook 合同交汇。",
      "P2 安全治理、P3 可观测性与 P4 可靠性可以伴随纵向功能切片推进，但不得绕过所属能力的验收边界。",
      "H5b 生产 Compaction 策略与 P4a 确定性故障基建可并行设计；H5c 必须等待二者都有可执行产物。",
    ],
    serialRules: [
      "H1 → H2 → H3a 是 Hook 行为、执行顺序与 durable recovery 的强制串行基础。",
      "W1 → W2 → W3 → W4 是 Workflow 图、持久引擎、Agent 接入与 UI 的强制串行主路径。",
      "S1 先证明受信任 Skill；S2 再定义隔离 Script Host；R3 最后提炼公共 Rust SDK 与远程服务合同。",
      "任何 credential-aware Tool 必须先完成 P2 的 typed secret/reference boundary，不能用普通 JSON 先行模拟。",
    ],
  },
  initiatives,
  deferredBoundaries: [
    "Ontology Metadata MVP：只实现 Ontology、Object Type、Property 与 Link Type；对象实例与 Memory 集成由独立 change 承担。",
    "Ontology 演进：Shared Property Type、Interface / 多继承、Action Type、Object Type Group、schema history / snapshot / branch、实时协作、OT / CRDT 与离线自动合并，均等待实际需求后再设计。",
    "Ontology 物理数据绑定：数据源、外键或 join binding 等对象数据层需求明确后再设计；认证、授权与多租户沿用平台基础统一方案，不随 Ontology Metadata MVP 提前实现。",
    "Ontology 前端：并行前端可以消费已冻结 HTTP 契约，但不构成本后端 change 的交付或验收依赖。",
    "scheduler：durable scheduling、lease/fencing、多实例 takeover、rolling deployment、automatic resume、durable cancel、Agent/Workflow Session 协调。",
    "Agent template 管理：catalog CRUD、版本浏览、发布/提升/回滚与既有 AgentRuntime upgrade。",
    "Rust 动态库、运行中 Skill/Extension 热替换、Marketplace，以及 Command、Renderer、Shortcut 与任意 UI Plugin Registry。",
    "并发 Tool 执行、动态修改共享 Tool Registry、模型主动安装 Skill。",
    "W2 前的 Loop/Iteration/Subgraph、多地域 Workflow 迁移、无真实调用方的通用远程文件系统。",
  ],
}
