import type {
  ConceptBoundaries,
  ConceptEntry,
  ContextManual,
  ManualChapter,
} from "./model.ts"

const checkedAtCommit = "3995205"

const evidence = {
  coreIds: {
    kind: "实现",
    path: "crates/stratum-core/src/lib.rs",
    symbol: "TurnRuntimeSnapshot",
    context: "pub agent_id: AgentId",
    note: "不可变定义身份、长期运行聚合身份与 Turn 固定快照",
  },
  durableEvent: {
    kind: "实现",
    path: "crates/stratum-core/src/agent_loop_event.rs",
    symbol: "DurableAgentEvent",
    context: "LoopStarted {",
    note: "kernel durable 事实与 ContextPatch/decision record 的类型化定义",
  },
  runner: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/runner.rs",
    symbol: "AgentLoop",
    context: "llm_provider: Arc<dyn LlmProvider>",
    note: "模型、Tool 与迭代边界的确定性推进",
  },
  resume: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/resume.rs",
    symbol: "replay_events",
    context: "let mut messages = Vec::new();",
    note: "durable ledger 重建、Tool 后缀对账与 compaction 双模式回放",
  },
  journal: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/journal.rs",
    symbol: "HookJournal",
    context: "entries: HashMap<HookAddress, JournalEntry>",
    note: "Hook 地址、Pending/Completed/Failed 与 decision 重放",
  },
  hooks: {
    kind: "实现",
    path: "crates/stratum-agent/src/hook_runtime/runtime.rs",
    symbol: "PrepareNextTurnDecision",
    context: "Compact {",
    note: "五个 Hook 点的只读快照与窄 decision 词汇",
  },
  chain: {
    kind: "实现",
    path: "crates/stratum-agent/src/hook_runtime/chain.rs",
    symbol: "ChainHookRuntime",
    context: "handlers: Vec<Arc<dyn HookHandler>>",
    note: "ordered handlers、变换合并与 Stop/Compact 短路",
  },
  apiHookComposition: {
    kind: "实现",
    path: "crates/stratum-api/src/turn.rs",
    symbol: "build_hook_runtime",
    context: "ChainHookRuntime::new",
    note: "stock API composition 当前只装配 Approval handler，不含 compaction/Inject producer",
  },
  contextPatchImplementation: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/runner.rs",
    symbol: "validate_context_patch",
    context: "apply_context_patch",
    note: "ContextPatch 的校验、临时 request-view 应用与不回写 durable baseline",
  },
  contextPatchBehaviorTest: {
    kind: "测试",
    path: "crates/stratum-agent/tests/agent_loop_journal.rs",
    symbol: "patches_adjust_the_request_view_without_touching_committed_state",
    context: "from the unpatched committed context",
    note: "Patch 只改变当前 request view，下一迭代从未修改的 committed context 重建",
  },
  invalidContextPatchTest: {
    kind: "测试",
    path: "crates/stratum-agent/tests/agent_loop_journal.rs",
    symbol: "invalid_patches_fail_closed_before_the_model_request",
    context: "struct InvalidPatchRuntime",
    note: "非法 ContextPatch 在模型请求前 fail closed 并先写安全失败事实",
  },
  replaceResultTests: {
    kind: "测试",
    path: "crates/stratum-agent/src/hook_runtime/chain.rs",
    symbol: "after_tool_call_threads_replacements_in_order",
    context: "AfterToolCallDecision::ReplaceResult",
    note: "多个 after_tool_call replacement 按 handler 顺序传递并只保留最终 typed result",
  },
  replaceResultIdentityTest: {
    kind: "测试",
    path: "crates/stratum-agent/tests/agent_loop_hooks.rs",
    symbol: "after_replace_result_preserves_role_and_call_identity",
    context: 'ChatMessage::tool(CallId::from("call-1")',
    note: "replacement 保留 Tool role 与 CallId，原始结果不进入 durable transcript",
  },
  afterHookCancellation: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/runner.rs",
    symbol: "HookInvocation::Cancelled => AfterToolCallDecision::Keep",
    context: "if let AfterToolCallDecision::ReplaceResult",
    note: "after_tool_call cancellation 降级 Keep，可留下 Pending-only",
  },
  prepareHookCancellation: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/runner.rs",
    symbol: "Cancellation degrades the decision to Continue",
    context: "HookInvocation::Cancelled => PrepareNextTurnDecision::Continue",
    note: "prepare_next_turn cancellation 降级 Continue 并提交 IterationCompleted",
  },
  injectReplayState: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/resume.rs",
    symbol: "pub(crate) struct ReplayState",
    context: "pub(crate) continuation: Option<ResumeContinuation>",
    note: "replay state 没有 pending Inject carry；只通过未关闭边界的 continuation 重放",
  },
  toolCycleBoundary: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/runner.rs",
    symbol: "async fn close_tool_cycle",
    context: "self.hook_runtime.prepare_next_turn(",
    note: "prepare_next_turn 只在完整 Tool cycle 收尾执行",
  },
  noToolBoundary: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/runner.rs",
    symbol: "if !tool_calls.is_empty()",
    context: "DurableAgentEvent::LoopFinished",
    note: "无 Tool 终局直接提交 IterationCompleted 与 LoopFinished",
  },
  injectBoundary: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/runner.rs",
    symbol: "PrepareNextTurnDecision::Inject { messages }",
    context: "state.pending_inject = Some(messages);",
    note: "Inject 只存在于 Tool cycle 收尾后的进程内 pending request view",
  },
  compaction: {
    kind: "实现",
    path: "crates/stratum-agent/src/agent_loop/compaction.rs",
    symbol: "COMPACTION_MARKER_PREFIX",
    context: "compaction_marker",
    note: "kernel-owned marker 与迭代边界压缩机制",
  },
  toolExecutor: {
    kind: "实现",
    path: "crates/stratum-agent/src/tool_executor/definition.rs",
    symbol: "ToolExecutor",
    context: "durable_events: Arc<dyn DurableEventSink>",
    note: "ToolExecutionStarted durable ack 先于外部 Tool 调用",
  },
  postgres: {
    kind: "实现",
    path: "crates/stratum-postgres/src/commands.rs",
    symbol: "append_event",
    context: "command: AppendEvent",
    note: "AgentRuntime-wide event_seq 分配与 compaction companion 原子写入",
  },
  postgresQueries: {
    kind: "实现",
    path: "crates/stratum-postgres/src/queries.rs",
    symbol: "decode_event_row",
    context: "let event_type: String",
    note: "严格 durable 解码、definition pin 与 companion 校验",
  },
  compactionTests: {
    kind: "测试",
    path: "crates/stratum-agent/tests/agent_loop_compaction.rs",
    symbol: "resume_closes_the_compaction_crash_window_from_the_journal",
    context: "journaled summary",
    note: "marker、cut、journal 重用与 W1/W2 崩溃窗口",
  },
  resumeTests: {
    kind: "测试",
    path: "crates/stratum-agent/tests/agent_loop_resume.rs",
    symbol: "resume_after_started_without_result_reexecutes_the_tool",
    context: "a started call with an unknown outcome re-executes",
    note: "已提交前缀复用、缺失后缀 at-least-once 与非法序列 fail closed",
  },
  postgresTests: {
    kind: "测试",
    path: "crates/stratum-postgres/tests/postgres_execution_storage.rs",
    symbol: "transcript_compaction_commits_atomically_and_validates_pointer",
    context: "The retained pointer must address a real earlier MessageAppended",
    note: "无空洞序号、原子 companion、损坏关系与 pointer fallback",
  },
  apiResume: {
    kind: "实现",
    path: "crates/stratum-api/src/http/turns.rs",
    symbol: "post_resume",
    context: "read_resume_slice",
    note: "resume 在装配 kernel 前读取 state、immutable definition、snapshot 与严格 ledger slice",
  },
  dispatcher: {
    kind: "实现",
    path: "crates/stratum-api/src/dispatcher.rs",
    symbol: "flush_durable",
    context: "io.scan(frontier.sequence(), target)",
    note: "PG product 投影、AgentRuntime scope 与 NATS realtime tail 的收敛边界",
  },
  webReconcile: {
    kind: "实现",
    path: "stratum-web/features/agent-conversation/recovery.ts",
    symbol: "reconcileConversation",
    context: "getAgentRuntimeHistory",
    note: "Web 以 PG view/history 推进 confirmed barrier，并在其上 rebase realtime frame",
  },
  runtimeDoc: {
    kind: "运维",
    path: "docs/runtime.md",
    symbol: "AgentRuntimeId",
    context: "Postgres 是 Agent 执行事实的**唯一**持久化存储",
    note: "Postgres 唯一真相、NATS short tail 与显式 resume 边界",
  },
  identitySpec: {
    kind: "规范",
    path: "openspec/specs/session-runtime-identity/spec.md",
    symbol: "AgentRuntimeId 标识长期运行聚合",
    context: "永久标识`agent_states`中的一个长期运行聚合",
    note: "不可变 Agent template version 与长期运行聚合的规范边界",
  },
  runtimeProtocolSpec: {
    kind: "规范",
    path: "openspec/specs/runtime-event-protocol/spec.md",
    symbol: "Durable Product Event 使用 AgentRuntime-wide Event Sequence",
    context: "`(AgentRuntimeId,event_seq)`",
    note: "PG、dispatcher、NATS 与 Web 收敛使用同一 runtime-scoped 顺序语义",
  },
  hookSpec: {
    kind: "规范",
    path: "openspec/specs/agent-hook-runtime/spec.md",
    symbol: "Hook 调用写入 Journal 记录",
    context: "`HookInvocationPending`",
    note: "Hook invocation durable boundary 与恢复复用要求",
  },
  resumeSpec: {
    kind: "规范",
    path: "openspec/specs/agent-loop-resume/spec.md",
    symbol: "恢复时 Tool 结果对账",
    context: "只重试缺失有序后缀",
    note: "Tool committed prefix、missing suffix 与 fail-closed 序列合同",
  },
  compactionSpec: {
    kind: "规范",
    path: "openspec/specs/context-compaction/spec.md",
    symbol: "Companion 只保存 Summary 与 Retained Frontier",
    context: "`transcript_compactions`不得",
    note: "compaction discriminator、companion、pointer 与 full replay 的规范关系",
  },
  agentVersionMaterialization: {
    kind: "实现",
    path: "crates/stratum-postgres/src/commands.rs",
    symbol: "create_agent_runtime",
    context: "FROM agents WHERE name = $1 AND version = $2",
    note: "name/tag 串行物化、canonical definition 复用与冲突拒绝",
  },
  agentVersionTests: {
    kind: "测试",
    path: "crates/stratum-postgres/tests/postgres_execution_storage.rs",
    symbol: "create_is_key_only_idempotent_and_versions_are_immutable",
    context: "same exact name/tag/definition",
    note: "同版本复用、同 tag 异定义冲突与不同 tag 新建的真实 PG 证据",
  },
} as const

const conceptBoundaries = {
  "deterministic-kernel": {
    deferred:
      "多实例 ownership、scheduler takeover 与 automatic resume 由独立调度 change 定义。",
    nonGoal:
      "durable ledger 单独不是完整运行定义；kernel 不读取 Postgres/NATS，不持有 AgentRuntimeId，也不决定 hosting。",
  },
  "journal-nondeterminism": {
    deferred:
      "通用副作用 Tool 的幂等协议与 exactly-once 体验等待真实 Tool 集成后设计。",
    nonGoal: "未提交的 LLM 候选输出不享有重放保证；crash 后允许再次调用模型。",
  },
  "event-stream-truth": {
    deferred: "核心资产 retention、删除与跨版本迁移策略尚未定义。",
    nonGoal:
      "durable_events 不替代 agents、agent_states 或 transcript_compactions；NATS、SSE、浏览器缓存和 telemetry 都不是 durable truth。",
  },
  "agent-template-version": {
    deferred:
      "版本浏览、提升、回滚和既有 runtime 升级等待 template 管理 change。",
    nonGoal: "version string tag 不排序、不自增，也不标识运行实例。",
  },
  "agent-runtime": {
    deferred:
      "多进程 placement、lease/fencing、自动接管和 durable cancel 等待 scheduler change。",
    nonGoal: "AgentRuntime 不是进程、Turn、Session 或不可变 Agent 定义的别名。",
  },
  "runtime-event-sequence": {
    deferred: "长期归档、冷热分层与序号资产 retention 仍属平台存储设计。",
    nonGoal:
      "公开 product event 不承诺数字连续；内部 durable facts 可以造成可见跳号。",
  },
  "committed-context": {
    deferred:
      "长期上下文收缩与 retention 由生产 compaction policy 和数据治理分别处理。",
    nonGoal:
      "streaming draft、request view 与未 ack 的候选消息不属于 committed context。",
  },
  "request-view": {
    deferred: "安全的 provider 请求诊断与脱敏采样等待 observability change。",
    nonGoal: "request view 不写 history、不进入 outcome，也不作为恢复缓存。",
  },
  "context-patch": {
    deferred: "可持久收缩的触发、摘要与质量策略等待 H5b/H5c。",
    nonGoal:
      "ContextPatch 不编辑 durable history，也不替代 TranscriptCompacted。",
  },
  "iteration-boundary": {
    deferred:
      "普通无 Tool 回复与跨 Turn 的生产 compaction 触发点等待 H5b 重新设计。",
    nonGoal:
      "任意消息位置、provider 响应结束和 Turn terminal 都不是该边界的替代品。",
  },
  "replace-result": {
    deferred:
      "通用敏感结果变换、credential-aware Tool 与 fail-closed redaction 尚未实现。",
    nonGoal:
      "已提交原始 Tool result 后再做展示层隐藏，不等于安全的 ReplaceResult。",
  },
  journal: {
    deferred: "journal retention、压缩与审计导出要在恢复合同稳定后另行设计。",
    nonGoal: "journal 没有独立 sequence、独立事务或独立事实来源。",
  },
  "hook-address": {
    deferred:
      "跨 Hook wire version 的地址演进与迁移规则等待 Extension 协议落地。",
    nonGoal:
      "消息下标、数据库行号、payload 内容和随机 invocation id 都不是结构地址。",
  },
  inject: {
    deferred:
      "当前无生产 handler 使用 Inject；真实用途和产品行为等待调用方出现。",
    nonGoal:
      "Inject 不是 durable user message，也不能出现在 history/new_messages；当前合同不保证所有崩溃窗口都至少消费一次。",
  },
  "short-circuit": {
    deferred: "更复杂的多 handler 仲裁只在出现真实冲突策略后设计。",
    nonGoal: "短路不能撤销已经 durable 的 journal 或此前完成的外部动作。",
  },
  "crash-window-replay": {
    deferred:
      "自动发现并恢复 unhosted running Turn 等待 scheduler/ownership change。",
    nonGoal: "恢复不猜测缺失事实、不静默修表，也不制造第二个 terminal。",
  },
  "tool-at-least-once": {
    deferred:
      "副作用 Tool 的统一幂等契约等待真实 Tool registry 和 service 边界。",
    nonGoal:
      "runtime 不承诺 Tool exactly-once，也不会把 Started 当作成功结果。",
  },
  "replay-modes": {
    deferred: "生产 compaction producer 与大历史性能验收等待 H5b/H5c。",
    nonGoal:
      "pointer fallback 不修复 companion、不删除原消息，也不改变 durable truth。",
  },
  "compaction-companion": {
    deferred: "摘要保留期、隐私删除和资产迁移不由当前 companion 机制决定。",
    nonGoal: "companion 不是独立事件，TranscriptCompacted 也不是原始消息删除。",
  },
  "mechanism-policy-separation": {
    deferred:
      "真实阈值、summary provider/model、失败预算和 chain 演进归 H5b/H5c。",
    nonGoal: "kernel 不选择压缩时机、摘要模型、token 阈值或摘要质量标准。",
  },
  "compaction-marker": {
    deferred: "更丰富的 marker 产品展示与摘要质量解释等待生产 producer 证据。",
    nonGoal:
      "marker 不承担恢复 pointer、不删除旧 history，也不替代 companion 的恢复元数据。",
  },
} satisfies Readonly<Record<string, ConceptBoundaries>>

type ConceptInput = Omit<ConceptEntry, "boundaries">

function concept(entry: ConceptInput): ConceptEntry {
  const boundaries = (
    conceptBoundaries as Readonly<Record<string, ConceptBoundaries>>
  )[entry.id]
  if (!boundaries) throw new Error(`missing concept boundaries: ${entry.id}`)
  return { ...entry, boundaries }
}

const firstPrinciples = [
  concept({
    id: "deterministic-kernel",
    term: "确定性 kernel",
    english: "Deterministic kernel",
    definition:
      "给定同一份 durable 固定的 Agent definition、model/Tool/Skill/Extension/handler identity，同一套可装配的 provider/Tool/Hook 实现与 LoopLimits，以及同一段严格解码的 durable 事件流，kernel 必须重建出相同的 committed context、迭代号与下一步操作。Hook journal 已包含在该事件流中；外部存储、HTTP、Session hosting 与调度语义留在组合层。",
    statuses: ["当前事实", "强制不变量"],
    avoid: [
      "把恢复理解成重新猜测",
      "在 kernel 中读取 Postgres",
      "让 handler 直接改基线",
    ],
    normal: {
      title: "同一事实，得到同一下一步",
      summary:
        "fresh 与 resume 都用 snapshot 固定的身份、同一套可装配组件和相同 LoopLimits 解释 durable facts；差别只在 facts 来自刚提交还是重放。",
      steps: [
        "加载并校验 pinned definition 与 TurnRuntimeSnapshot",
        "严格解码 LoopStarted 与后续事件",
        "按 event_seq 重建 committed context 和 journal",
        "校验 iteration、Tool 后缀与 terminal 状态",
        "产出唯一合法的下一 Operation",
      ],
    },
    failure: {
      title: "事实无法唯一解释",
      summary:
        "未知 durable variant、身份不一致或非法 Tool 后缀都不能靠默认分支继续。",
      steps: [
        "停止恢复",
        "返回 typed durable corruption/runtime incompatible",
        "不得启动 LLM、Tool 或 Hook",
      ],
    },
    recovery: {
      title: "修复离线事实，而非在线猜测",
      summary:
        "kernel 只报告不可恢复的不一致；数据修复或版本升级必须在运行路径之外完成。",
      steps: [
        "保存错误与 exact identity 证据",
        "隔离受影响 runtime",
        "使用受控迁移/运维修复后重新 resume",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "组合层绕开 typed replay",
        trigger:
          "新增恢复入口自行拼接消息，而非走既有 strict slice/prepare_resume",
        impact: "同一 ledger 可能在不同入口得到不同上下文",
        owner: "stratum-api / stratum-postgres",
        verification:
          "所有 resume 入口必须复用相同 strict decoder 与 prepared-resume seam",
      },
      {
        level: "已登记",
        title: "LoopLimits 尚未 durable pin",
        trigger:
          "运行中 Turn 跨版本 resume，而新 binary 改变 LoopLimits::default()",
        impact:
          "snapshot 不会检测该默认值漂移；同一 durable facts 可能面对不同执行预算",
        owner: "stratum-api / runtime snapshot",
        verification:
          "当前 stock 只依赖同 binary/default；引入可变 limits 前必须先定义 durable pin 与 resume 兼容检查",
      },
    ],
    invariant:
      "相同 snapshot identity、相同可装配组件与 LoopLimits + 相同 durable facts 必须产生相同恢复状态；任何无法解释的持久事实必须 fail closed。",
    evidence: [
      evidence.apiResume,
      evidence.runner,
      evidence.resume,
      evidence.resumeTests,
    ],
  }),
  concept({
    id: "journal-nondeterminism",
    term: "journal 固化非确定性",
    english: "Journaled nondeterminism",
    definition:
      "已提交的 assistant MessageAppended 与 handler decision 等非确定性结果会成为 durable fact；崩溃后重放这些已提交结果。模型已返回、但 assistant message 尚未提交的窗口没有可重放结果，resume 允许再次调用 LLM。Tool 外部副作用只有 Started/result 边界，仍是 at-least-once。",
    statuses: ["当前事实", "强制不变量", "潜在风险"],
    avoid: [
      "把 journal 当普通日志",
      "resume 时重调已完成 handler",
      "声称 Tool exactly-once",
    ],
    normal: {
      title: "decision 先落盘，再应用",
      summary:
        "HookInvocationCompleted 携带完整 decision；执行其效果前先获得 durable acknowledgement。",
      steps: [
        "写 Pending",
        "调用 handler",
        "写 Completed(decision)",
        "应用 decision",
      ],
    },
    failure: {
      title: "Completed 后进程崩溃",
      summary:
        "decision 已提交但效果可能尚未提交；resume 复用原 decision，不重新生成。具体效果是否补齐由该 Hook 点的 replay frontier 决定，Inject 在边界已提交后是已知例外。",
      steps: [
        "重启读取 Completed",
        "禁止重调 handler",
        "按对应 continuation 判断是否仍有未完成效果",
      ],
    },
    recovery: {
      title: "重放 decision",
      summary:
        "journal 的 structural address 找到原记录并禁止重调 handler；decision 的效果是否仍待应用，要由对应 Hook 点的 crash-window continuation 判断。",
      steps: [
        "按 HookAddress 定位",
        "journal 验证 address 与该 HookPoint 实际覆盖的 digest",
        "组合层另行验证 pinned extension/handler versions",
        "复用 decision",
        "按该 Hook 点的 replay frontier 判断继续、降级或结束",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "Tool 副作用重复",
        trigger:
          "ToolExecutionStarted 已提交、外部副作用已发生，但 tool result 未提交",
        impact: "resume 使用同一 CallId 重执行，外部副作用可能再次发生",
        owner: "Tool service / composition",
        verification:
          "副作用 Tool 以 CallId 建立幂等或去重；详见 P4a observable Tool fixture",
      },
    ],
    invariant:
      "已完成 Hook 的 decision 永不重新生成；Tool exactly-once 不属于 kernel 承诺。",
    evidence: [
      evidence.journal,
      evidence.durableEvent,
      evidence.toolExecutor,
      evidence.hookSpec,
    ],
  }),
  concept({
    id: "event-stream-truth",
    term: "Postgres 持久真相",
    english: "Postgres durable truth",
    definition:
      "Postgres 四张核心表共同构成唯一持久存储：agents 保存不可变定义，agent_states 保存运行 fence，durable_events 保存有序事实与 discriminator，transcript_compactions 保存 typed companion。ledger 是运行事实的顺序权威，但单独不足以恢复；NATS 只是可丢的短期实时 tail。",
    statuses: ["当前事实", "强制不变量", "恢复路径"],
    avoid: ["把 NATS 当恢复源", "把页面状态当事实", "在线修补坏 ledger"],
    normal: {
      title: "Postgres 先提交，NATS 后发布",
      summary:
        "核心命令的成功由 PG durable commit 决定；realtime 发布失败不会回滚业务事实。",
      steps: [
        "锁 exact agent_states row",
        "分配连续 event_seq",
        "提交 durable fact",
        "receipt 唤醒 dispatcher",
        "发布 product tail",
      ],
    },
    failure: {
      title: "NATS 丢失 product frame",
      summary: "浏览器可以暂时缺帧，但 PG high-water 与 history 仍完整。",
      steps: [
        "realtime 标为 degraded",
        "命令继续使用 PG",
        "客户端 reconcile 读取完整 (B,T] product window",
      ],
    },
    recovery: {
      title: "从 PG 冷恢复",
      summary:
        "cursor 过期、buffer reset 或进程重启后，重新读取 AgentRuntimeView 与 fixed-barrier history。",
      steps: [
        "无 cursor 订阅 short tail",
        "读取 view@T",
        "读取 through=T history",
        "重放 T 之后已缓冲 frame",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "核心资产暂无删除 API",
        trigger: "长期运行环境持续积累原始 message 与 durable event",
        impact: "存储增长、隐私保留与备份成本持续上升",
        owner: "平台基础 / 数据治理",
        verification: "在首个生产部署前冻结 retention、备份与删除策略",
      },
    ],
    invariant:
      "恢复必须由 state fence、immutable definition、pinned snapshot、可装配 runtime components 与严格 ledger slice 共同证明；NATS 与 UI 都不能创造 durable truth。",
    evidence: [
      evidence.postgres,
      evidence.postgresQueries,
      evidence.apiResume,
      evidence.dispatcher,
      evidence.webReconcile,
      evidence.runtimeProtocolSpec,
    ],
  }),
] as const

const identities = [
  concept({
    id: "agent-template-version",
    term: "Agent template 版本",
    english: "AgentId",
    definition:
      "agents 表中的一份不可变行为定义。作者用大小写敏感、无排序语义的 `(name, version string tag)` 命名；AgentId 是该定义行的 UUID，不表示对话或运行进程。",
    statuses: ["当前事实", "强制不变量"],
    avoid: ["Agent 实例", "会话 Agent", "自动递增版本", "AgentVersionId"],
    normal: {
      title: "多个 runtime 复用同一版本",
      summary:
        "两个 AgentRuntime 可以 pin 同一个 AgentId，但拥有独立 state、ledger、history 与 Turn；Session binding 也分别记录，SessionId 可以在不并发 running 的前提下被多个 runtime 顺序复用。",
      steps: [
        "读取 template name/tag",
        "严格比较 canonical definition",
        "复用同一 agents.id",
        "分别创建 agent_states.id",
      ],
    },
    failure: {
      title: "同 name/tag 指向不同定义",
      summary: "这不是新版本，而是作者重写了既有 tag 的含义。",
      steps: [
        "create transaction 检测 definition mismatch",
        "返回 agent_version_conflict",
        "不创建 runtime 或孤儿版本",
      ],
    },
    recovery: {
      title: "旧 runtime 继续 pin 旧定义",
      summary:
        "filesystem 当前文件变化不会改写已存在 runtime；恢复只按 state.agent_id 加载 immutable definition。",
      steps: [
        "读取 agent_states.agent_id",
        "加载 agents.id",
        "验证 LoopStarted snapshot.agent_id",
        "恢复原定义",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "版本治理尚未提供 UI/API",
        trigger: "作者需要浏览、提升、回滚或升级既有 runtime",
        impact:
          "目前只能靠当前 template 文件创建新 runtime，无法显式管理历史版本",
        owner: "未来 Agent template 管理 change",
        verification:
          "新增版本资源前保持 `/v1/agents/{id}` 与 runtime routes 的身份分离",
      },
    ],
    invariant:
      "一个 AgentId 永久对应一份 immutable canonical definition；tag 绝不用于排序或运行分区。",
    evidence: [
      evidence.coreIds,
      evidence.agentVersionMaterialization,
      evidence.agentVersionTests,
      evidence.identitySpec,
    ],
  }),
  concept({
    id: "agent-runtime",
    term: "AgentRuntime",
    english: "AgentRuntimeId",
    definition:
      "长期运行的对话聚合。它永久 pin 一个 AgentId，跨多个 Turn，并拥有自己的 model_config、Session/current Turn、审批、history、last_event_seq 与状态。",
    statuses: ["当前事实", "强制不变量"],
    avoid: ["AgentId 当运行实例", "RunId", "AgentVersionId"],
    normal: {
      title: "一个 runtime 跨多个 Turn",
      summary:
        "同一 AgentRuntime 先后执行 Turn A、Turn B；event_seq 在两者之间继续递增。",
      steps: [
        "idle runtime",
        "begin Turn A",
        "terminal A",
        "begin Turn B",
        "terminal B",
      ],
    },
    failure: {
      title: "共享 definition 的 runtime 被混流",
      summary:
        "若用 AgentId 分区 SSE/history，两个 runtime 会共享 cursor 与序号，造成事实串线。",
      steps: [
        "发现 frame.agent_runtime_id 不匹配",
        "关闭 stream",
        "无 cursor 冷启动",
        "仍不匹配则报告协议错误",
      ],
    },
    recovery: {
      title: "按 exact runtime 恢复",
      summary:
        "view、history、approval、resume、dispatcher 与 NATS subject 全部以 AgentRuntimeId 定位。",
      steps: [
        "读取 exact agent_states.id",
        "验证 pinned AgentId",
        "读取 runtime-wide ledger",
        "恢复 current Turn",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "新功能误用 AgentId",
        trigger:
          "新增路由、缓存 key、subject 或 dispatcher map 时只暴露 agent_id",
        impact: "共享 template 版本的 runtime 互相污染",
        owner: "API / infra / Web",
        verification:
          "所有运行态 identity 必须显式使用 AgentRuntimeId；AgentId 只作 definition fence",
      },
    ],
    invariant:
      "AgentRuntimeId 是运行聚合唯一地址；一个 runtime 只能 pin 一个 AgentId，且不可变。",
    evidence: [
      evidence.coreIds,
      evidence.postgres,
      evidence.runtimeDoc,
      evidence.identitySpec,
    ],
  }),
  concept({
    id: "runtime-event-sequence",
    term: "AgentRuntime-wide event sequence",
    english: "(AgentRuntimeId, event_seq)",
    definition:
      "每个 AgentRuntime 从 1 开始、跨 Turn 连续无空洞的 durable 事实顺序。公开 product 过滤内部 Hook/Tool 事实后允许数字跳号，不能用可见 gap 推断丢帧。",
    statuses: ["当前事实", "强制不变量", "失败模式"],
    avoid: [
      "agent-wide 未区分 definition/runtime",
      "per-Turn sequence",
      "message_seq",
    ],
    normal: {
      title: "内部事实与公开 product 共用 ledger",
      summary:
        "event_seq 41/42 可是内部 journal，43 才是公开 MessageAppended；Web 看到 40→43 是合法的。",
      steps: [
        "state row lock 分配 41",
        "提交内部 fact 41/42",
        "提交公开 fact 43",
        "公开 tail 只投影 43",
      ],
    },
    failure: {
      title: "high-water 内出现真实缺口",
      summary:
        "last_event_seq=43，但 durable_events 缺 42，说明原子 allocator 或存储已损坏。",
      steps: [
        "strict range 读取发现缺洞",
        "返回 durable_state_corrupt",
        "停止该次严格读取、恢复或 dispatcher projection",
      ],
    },
    recovery: {
      title: "保留证据并离线处理",
      summary: "真实缺口不是 NATS 丢包，不能靠 reconcile 填补。",
      steps: [
        "隔离 runtime",
        "保存 state/ledger oracle",
        "从备份或受控修复恢复",
        "重新验证连续性",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "把 event_seq 转成 JavaScript number",
        trigger: "Web 新代码用 Number/parseInt 比较十进制序号",
        impact: "超过安全整数后排序与去重错误",
        owner: "stratum-web",
        verification: "协议保持 decimal string，比较使用 BigInt helper",
      },
      {
        level: "已登记",
        title: "热 writer 不重扫完整历史",
        trigger:
          "绕过正式写入路径直接破坏旧 durable row，在 hosted Turn 已运行时制造 high-water 内缺口",
        impact:
          "writer 仍可能按 last_event_seq + 1 追加；严格 read/resume/dispatcher 会在随后读取时 fail closed",
        owner: "Postgres 运维 / P4a malformed fixture",
        verification:
          "禁止旁路 SQL 写生产 ledger；只在 disposable fixture 中注入缺口，并分别验证 writer 与 strict consumer 的真实边界",
      },
    ],
    invariant:
      "PG high-water 内 event_seq 必须连续；公开投影的跳号不等于 durable 缺口。",
    evidence: [
      evidence.postgres,
      evidence.postgresQueries,
      evidence.postgresTests,
      evidence.runtimeProtocolSpec,
    ],
  }),
] as const

const contextAndIteration = [
  concept({
    id: "committed-context",
    term: "Committed context",
    definition:
      "由 pinned immutable Agent definition 提供 system prompt，并由 durable event stream 重建全部已提交 message，二者合成持久对话基线。只有 kernel 提交的 MessageAppended 与 TranscriptCompacted 可以改写消息部分。",
    statuses: ["当前事实", "强制不变量"],
    avoid: ["history 当基线", "handler 直接写 context", "把临时 patch 回写"],
    normal: {
      title: "durable message 推进基线",
      summary:
        "用户消息、assistant final 与 Tool result 只有在 durable acknowledgement 后才进入 committed context。",
      steps: [
        "生成候选内容",
        "append durable event",
        "收到 ack",
        "更新内存 committed context",
      ],
    },
    failure: {
      title: "外部响应存在但 durable append 失败",
      summary:
        "未提交内容不能被当作已完成事实；Turn 按 typed failure 结束或等待恢复。",
      steps: [
        "保留 committed baseline",
        "不把候选内容加入 outcome/history",
        "返回存储错误",
      ],
    },
    recovery: {
      title: "只从已提交 facts 重建",
      summary: "重启时忽略进程内草稿和 request view，按 event_seq 重建基线。",
      steps: [
        "加载 pinned Agent definition 的 system prompt",
        "读取 replay slice",
        "应用 MessageAppended",
        "应用 TranscriptCompacted",
        "得到 committed context",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "组合层展示未提交草稿",
        trigger:
          "UI 或 outcome 把 streaming telemetry/request view 合并进 durable timeline",
        impact: "用户看到的内容可能在刷新后消失，或与严格 replay 的结果矛盾",
        owner: "stratum-api / stratum-web projection",
        verification:
          "draft 与 durable product 分层；只有 PG reconcile 或 durable final 能关闭最终内容",
      },
    ],
    invariant:
      "任何未 durable ack 的内容都不是 committed context；handler 永远不能越权写入基线。",
    evidence: [
      evidence.apiResume,
      evidence.runner,
      evidence.resume,
      evidence.durableEvent,
    ],
  }),
  concept({
    id: "request-view",
    term: "Request view",
    definition:
      "每次 LLM 请求前现场拼装、用完即弃的请求体：先从 committed context 派生并加入本轮一次性 Inject，再让 transform_context 读取这个 view 并应用 ContextPatch。它不落盘、不回写、不进入 LoopOutcome.new_messages。",
    statuses: ["当前事实", "非目标"],
    avoid: ["request context", "当前上下文", "把 request view 当 transcript"],
    normal: {
      title: "临时裁剪只影响一次调用",
      summary:
        "ContextPatch 可以缩短本次请求，但下一轮仍从未收缩的 committed context 重新构建。",
      steps: [
        "读取 committed context",
        "加入并消费一次性 Inject",
        "transform_context 读取含 Inject 的 snapshot",
        "校验并应用 ContextPatch",
        "调用 provider",
        "丢弃 request view",
      ],
    },
    failure: {
      title: "将 patch 误写入 durable history",
      summary: "临时策略会永久删除事实，破坏 replay 等价与审计边界。",
      steps: [
        "检测不允许的写路径",
        "拒绝 handler 直接提交 context",
        "只接受 decision",
      ],
    },
    recovery: {
      title: "重新计算，而不是恢复缓存",
      summary:
        "request view 没有恢复身份；resume 根据 journaled decision 与 committed facts 重建。ContextPatch 可重放；Inject 只有在 Tool cycle 边界仍待关闭时才会重新应用。",
      steps: [
        "恢复 committed baseline",
        "读取 Completed decision",
        "重新应用 patch；按 replay frontier 判断是否仍应应用 Inject",
        "发起尚未完成的请求",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "把临时视图暴露为 history",
        trigger: "新增调试/API 字段直接返回 provider request body",
        impact: "用户会误以为历史被永久裁剪，且可能扩大敏感载荷暴露",
        owner: "API / observability",
        verification:
          "公开 history 只来自 durable product；请求体只在受控诊断边界处理",
      },
    ],
    invariant: "request view 是一次性派生值，不是 durable asset。",
    evidence: [
      evidence.hooks,
      evidence.contextPatchImplementation,
      evidence.contextPatchBehaviorTest,
    ],
  }),
  concept({
    id: "context-patch",
    term: "ContextPatch",
    definition:
      "transform_context 返回的增量修改：ReplaceSystemPrompt、DropHistory、RewriteHistory 或 Composite。只作用于当前 request view，不改变 committed context。",
    statuses: ["当前事实", "强制不变量"],
    avoid: ["上下文写入", "历史编辑", "永久压缩"],
    normal: {
      title: "宽读窄写",
      summary:
        "handler 读取 HookSnapshot，但只能返回受限 decision；kernel 校验并应用。",
      steps: [
        "读取 committed context + pending Inject 派生的 request snapshot",
        "返回 typed patch",
        "kernel 验证 cut",
        "构造 request view",
      ],
    },
    failure: {
      title: "非法 cut 或复合冲突",
      summary:
        "Patch 超出历史范围或产生非法消息结构时，本次 Hook 操作 fail closed。",
      steps: [
        "校验 typed decision",
        "拒绝非法 patch",
        "按 Hook failure 终止受影响操作",
      ],
    },
    recovery: {
      title: "由 journal 重放同一 patch",
      summary:
        "已完成 transform_context 不重新调用 handler，直接重放 decision record。",
      steps: [
        "定位 HookAddress",
        "验证 iteration/HookPoint digest；它不覆盖完整 context/messages",
        "decode ContextPatch",
        "重新构造 request view",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "临时裁剪掩盖长期上下文增长",
        trigger:
          "连续 ContextPatch 降低单次 provider 输入，但 committed history 持续增长",
        impact:
          "团队可能误以为已经完成持久压缩，实际存储与 full replay 成本仍上升",
        owner: "Hook composition / H5b compaction policy",
        verification:
          "分别观测 request token 与 durable baseline；持久收缩只以 TranscriptCompacted 为证据",
      },
    ],
    invariant:
      "ContextPatch 永不改变 durable baseline；需要持久收缩必须走 TranscriptCompacted。",
    evidence: [
      evidence.durableEvent,
      evidence.contextPatchImplementation,
      evidence.contextPatchBehaviorTest,
      evidence.invalidContextPatchTest,
    ],
  }),
  concept({
    id: "iteration-boundary",
    term: "迭代边界",
    english: "Iteration boundary",
    definition:
      "成功越过本次 iteration frontier 时会以 IterationCompleted 收口；provider/Hook/Tool 失败或取消可以直接进入 LoopFailed/LoopCancelled，不保证先有该边界。只有完整 Tool cycle 的全部模型可见结果 durable 提交后，kernel 才在提交边界前调用 prepare_next_turn。无 Tool 的普通成功终局会直接提交 IterationCompleted 与 LoopFinished，不调用 prepare_next_turn。",
    statuses: ["当前事实", "强制不变量"],
    avoid: ["Turn 边界", "模型回复结束", "任意消息下标"],
    normal: {
      title: "Tool pair 完整后才越界",
      summary:
        "ToolExecutionStarted 与对应 role=tool result 都已提交，才允许 Compact/Stop/Inject。",
      steps: [
        "模型返回 tool calls",
        "顺序执行并提交每个 result",
        "prepare_next_turn",
        "提交 IterationCompleted",
        "进入下一请求",
      ],
    },
    failure: {
      title: "在 Tool cycle 中途压缩",
      summary:
        "cut 可能分开 tool_call/result，provider 下次请求会看到不合法的消息结构。",
      steps: [
        "kernel 拒绝非边界 Compact",
        "保留原 committed context",
        "返回 typed compaction error",
      ],
    },
    recovery: {
      title: "对账后回到唯一边界",
      summary:
        "resume 先恢复缺失 Tool result，再关闭 iteration；不从半截结构开始下一请求。",
      steps: [
        "校验 result 前缀",
        "重试缺失后缀",
        "提交结果",
        "恢复 prepare_next_turn/iteration boundary",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "普通无 Tool Turn 尚无生产 compaction 触发点",
        trigger: "未来策略希望跨纯聊天 Turn 自动压缩",
        impact:
          "现有 prepare_next_turn 只覆盖 Tool cycle 收尾，不能仅加 handler 解决",
        owner: "H5b production compaction proposal",
        verification: "先定义组合层触发边界，不扩大 kernel 职责后再实现",
      },
    ],
    invariant:
      "prepare_next_turn 的 durable compaction/Inject/Stop 决策只能发生在 Tool pairing 完整的成功收尾；纯文本成功终局同样提交 IterationCompleted，但失败/取消可直接 terminal，二者都不是该 Hook 边界。",
    evidence: [
      evidence.toolCycleBoundary,
      evidence.noToolBoundary,
      evidence.hooks,
      evidence.compactionTests,
    ],
  }),
  concept({
    id: "replace-result",
    term: "ReplaceResult",
    definition:
      "after_tool_call 的 decision：在 Tool result durable commit 之前，用相同 CallId 和 tool role 替换 JSON body。原始结果不会进入 committed context。",
    statuses: ["当前事实", "潜在风险"],
    avoid: ["结果压缩", "结果截断后仍保留原文", "事后 compaction"],
    normal: {
      title: "预防性瘦身",
      summary:
        "大结果在进入 durable transcript 前被替换为模型仍可使用的安全结构。",
      steps: [
        "Tool 返回原结果",
        "after_tool_call 检查",
        "返回 ReplaceResult",
        "提交替换后的 role=tool message",
      ],
    },
    failure: {
      title: "replacement 未能安全产生或提交",
      summary:
        "fresh typed handler 失败、超时或 Completed durability 失败时，不能假装 replacement 已生效；持久 journal 损坏则在恢复时 fail closed。",
      steps: [
        "返回 typed handler/storage error 或取消语义",
        "记录安全 typed error",
        "不把未确认的 replacement 当作 durable result",
      ],
    },
    recovery: {
      title: "重放已确认 replacement",
      summary:
        "Completed decision 与已提交 result 共同证明结果；已提交 result 不重做 Tool。",
      steps: [
        "读取 after_tool_call journal",
        "验证 role/CallId",
        "复用 replacement",
        "继续 iteration",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "原始结果不可审计",
        trigger: "handler 用 ReplaceResult 丢弃了业务上需要保留的原始输出",
        impact: "durable transcript 只保留替换值，无法事后重建原结果",
        owner: "handler / composition",
        verification:
          "需要原始留存时使用受控私有通道并定义 retention，不依赖 kernel transcript",
      },
    ],
    invariant:
      "replacement 保持同一 CallId/tool role；原始结果与替换结果只能有一个进入 committed context。",
    evidence: [
      evidence.hooks,
      evidence.replaceResultTests,
      evidence.replaceResultIdentityTest,
    ],
  }),
] as const

const hooksAndJournal = [
  concept({
    id: "journal",
    term: "Journal",
    definition:
      "HookInvocationPending / Completed / Failed 作为 DurableAgentEvent 变体住在同一 Postgres ledger，没有第二条提交顺序。Completed 保存 decision，resume 用它代替重调 handler。",
    statuses: ["当前事实", "强制不变量", "恢复路径"],
    avoid: ["日志", "审计日志", "独立 journal store"],
    normal: {
      title: "唯一 commit 顺序",
      summary:
        "Hook journal 与 Message/Tool/Iteration facts 共享 event_seq，因此崩溃点可精确判定。",
      steps: [
        "Pending seq=41",
        "Completed seq=42",
        "decision effect seq=43",
        "下一事实 seq=44",
      ],
      eventRows: [
        {
          seq: "41",
          type: "HookInvocationPending",
          scope: "internal",
          fact: "prepare_next_turn / digest=…00d91（展示缩写）",
          tone: "internal",
        },
        {
          seq: "42",
          type: "HookInvocationCompleted",
          scope: "internal",
          fact: "decision=Compact(summary)",
          tone: "internal",
        },
        {
          seq: "43",
          type: "TranscriptCompacted",
          scope: "product",
          fact: "upto=12 / retained=31",
          tone: "fact",
        },
        {
          seq: "44",
          type: "IterationCompleted",
          scope: "product",
          fact: "iteration=3",
          tone: "fact",
        },
      ],
    },
    failure: {
      title: "Pending 存在但无 Completed",
      summary:
        "handler 可能没调用、调用中崩溃、被取消或响应未持久化；没有 decision 可复用。取消竞争可以只留下 Pending，after_tool_call / prepare_next_turn 会分别降级 Keep / Continue。",
      steps: [
        "只有 replay continuation 再次到达同一 HookAddress 时才重试",
        "被重试时复用原 HookInvocationId",
        "写唯一 Completed/Failed",
        "recording-path cancellation 可按 Keep/Continue 前进并留下永久 Pending-only",
      ],
    },
    recovery: {
      title: "按状态分支恢复",
      summary:
        "Completed 复用 record；Pending 只有在 replay 再次到达该 HookAddress 时才用同 ID 重试；Failed 保持失败语义。已经越过 frontier 的 recording-path Pending 不会被无条件重访。",
      steps: [
        "strict decode record",
        "journal 匹配 address/digest",
        "组合层验证 pinned extension/handler versions",
        "选择 replay/retry/fail",
        "继续 kernel",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "journal 长期增长",
        trigger:
          "每个 Hook invocation 至少保留 Pending；正常完成或明确失败还会保留 Completed/Failed",
        impact: "长期 runtime 的存储、严格解码与审计成本持续上升",
        owner: "平台数据治理",
        verification:
          "恢复与审计语义冻结前不在线删除；后续以独立 retention change 验证",
      },
    ],
    invariant:
      "journal 不得拥有独立 sequence 或事务；只有 handler-produced decision 的 effect 必须排在 Completed 之后，取消允许 Pending-only。",
    evidence: [
      evidence.durableEvent,
      evidence.journal,
      evidence.afterHookCancellation,
      evidence.prepareHookCancellation,
      evidence.postgres,
    ],
  }),
  concept({
    id: "hook-address",
    term: "Hook 地址",
    english: "HookAddress",
    definition:
      "journal 的结构性键 `(iteration, HookPoint, Option<CallId>)`。迭代级 Hook 无 CallId，Tool 级 Hook 用 CallId 区分；不依赖消息下标，因此 compaction 后仍稳定。",
    statuses: ["当前事实", "强制不变量"],
    avoid: ["消息下标寻址", "hook id 当地址", "按 payload 搜索"],
    normal: {
      title: "稳定定位同一语义调用",
      summary:
        "iteration=3 的 prepare_next_turn 与 call_9 的 after_tool_call 在 replay 中得到相同地址。",
      steps: [
        "构造 structural address",
        "查询 journal",
        "验证 invocation identity",
        "复用或创建 record",
      ],
    },
    failure: {
      title: "重复地址或受覆盖输入的 digest 变化",
      summary:
        "Tool Hook 的 digest 绑定完整 ToolCall；同一 address 若出现不同 ToolCall 会 fail closed。Context Hook 当前只摘要 iteration/HookPoint，不会检测 context、messages 或 usage 漂移。",
      steps: ["拒绝插入/重放", "返回 journal mismatch", "停止外部动作"],
    },
    recovery: {
      title: "消息压缩后地址不漂移",
      summary:
        "compaction 改写 committed message 前缀，但 iteration/HookPoint/CallId 不变。",
      steps: [
        "应用 TranscriptCompacted",
        "保留 journal map",
        "按 address 找 Completed",
        "继续 replay",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "结构地址被错误复用",
        trigger:
          "新 kernel 路径为不同语义输入复用同一 iteration/HookPoint/CallId",
        impact: "resume 会遇到 digest mismatch，或错误复用另一项 decision",
        owner: "stratum-agent kernel",
        verification:
          "CallId 唯一；Tool Hook 严格校验 ToolCall digest，组合层 preflight 独立校验 pinned extension/handler versions",
      },
      {
        level: "已登记",
        title: "Context Hook digest 不覆盖上下文内容",
        trigger:
          "同一 iteration/HookPoint 的 committed context 或 usage 因实现漂移而改变",
        impact:
          "journal key/digest 仍可能匹配，无法单靠 record 证明 handler 看见完全相同的输入",
        owner: "stratum-agent Hook journal",
        verification:
          "依赖 snapshot/definition/preflight 维持组合一致；若未来要求内容级证明，需独立设计 canonical context digest",
      },
    ],
    invariant:
      "Hook 地址只由结构性身份组成，绝不由内容、数组下标或数据库行号派生。",
    evidence: [evidence.journal, evidence.coreIds, evidence.compactionTests],
  }),
  concept({
    id: "inject",
    term: "Inject",
    definition:
      "prepare_next_turn decision：给下一次 request view 添加一次性 User message。它不落盘、不进 history/new_messages；当前无生产 handler 使用。",
    statuses: ["当前事实", "明确延期", "非目标"],
    avoid: ["消息注入", "durable user message", "上下文持久写入"],
    normal: {
      title: "一次消费",
      summary:
        "多个 handler 的 Inject 按链顺序收集，在下一 request view 中消费后消失。",
      steps: [
        "prepare_next_turn 返回 Inject",
        "journal decision",
        "构造下一 request view",
        "消费一次",
        "不写 history",
      ],
    },
    failure: {
      title: "前序 Inject 被后续终局 decision 短路",
      summary:
        "同一 chain 后续 handler 返回 Stop/Compact 时，已收集 Inject 静默丢弃；这是已确认现状。",
      steps: [
        "收集 Inject A",
        "后续返回 Compact",
        "短路 chain",
        "丢弃 A",
        "执行 Compact",
      ],
    },
    recovery: {
      title: "仅在边界尚未提交时重放 Inject",
      summary:
        "Completed(Inject) 后若 IterationCompleted 尚未提交，resume 会重新关闭 Tool cycle 并复用 decision；一旦该边界已提交，pending_inject 没有 durable carry，崩溃后可能零次消费。",
      steps: [
        "读取 Completed Inject",
        "检查 replay frontier 是否仍需 CloseIteration",
        "需要时复用 decision 并重建下一 request view",
        "边界已提交则继续恢复，不重新注入",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "真实需求可能要求组合 decision",
        trigger: "handler 既要 Compact 又要保留下一轮 Inject",
        impact: "当前平级终局语义会丢弃 Inject",
        owner: "未来 Hook contract proposal",
        verification: "出现真实 handler 后再评估组合变体，当前不提前扩展",
      },
      {
        level: "已登记",
        title: "IterationCompleted 后崩溃可导致零次消费",
        trigger:
          "Inject decision 与 IterationCompleted 已 durable，但下一次 LLM 请求产生 durable assistant result 前进程退出",
        impact:
          "resume 不会从 replay state 恢复 pending_inject；该临时消息不会进入下一 request",
        owner: "未来 Inject contract proposal",
        verification:
          "stock composition 无 Inject handler；出现真实调用方前先冻结 durable pending-consumption 语义并加入 deterministic crash test",
      },
    ],
    invariant:
      "Inject 只影响下一次 request view，当前保证最多消费一次、但跨全部 crash window 可能零次；无真实调用方前不扩展合同。",
    evidence: [
      evidence.hooks,
      evidence.chain,
      evidence.journal,
      evidence.injectBoundary,
      evidence.injectReplayState,
      evidence.apiHookComposition,
    ],
  }),
  concept({
    id: "short-circuit",
    term: "短路",
    english: "Short-circuit",
    definition:
      "ordered Hook chain 中 Stop 与 Compact 是平级终局 decision；任一 handler 返回即定案，后续 handler 不调用，已收集 Inject 丢弃。handler 顺序因此属于运行时版本语义。",
    statuses: ["当前事实", "强制不变量", "潜在风险"],
    avoid: ["普通提前返回", "可交换 handler 顺序", "执行全部再投票"],
    normal: {
      title: "第一个终局 decision 获胜",
      summary: "链在 deterministic order 中运行，先到 Stop/Compact 即结束。",
      steps: [
        "handler A Continue",
        "handler B Inject",
        "handler C Stop",
        "丢弃 Inject",
        "不调用 D",
      ],
    },
    failure: {
      title: "部署时改变 handler 顺序",
      summary:
        "同一输入可能得到不同终局 decision；旧 running Turn 不能用新 chain 猜测恢复。",
      steps: [
        "resume 比对 ExtensionSetVersionId",
        "不匹配返回 runtime_unavailable",
        "禁止用当前 chain 重放",
      ],
    },
    recovery: {
      title: "使用固定版本链",
      summary:
        "LoopStarted snapshot 固定 extension/handler versions；组合层需能装配对应链。",
      steps: [
        "加载 pinned chain identity",
        "逐 handler 验证版本",
        "按原顺序重放",
        "否则 fail closed",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "未来 compaction handler 破坏旧 Turn resume",
        trigger: "生产 chain 新增 compaction handler，旧 chain 版本不再可装配",
        impact: "旧 running Turn 会 runtime_unavailable",
        owner: "H5b chain evolution",
        verification:
          "production compaction proposal 必须先定义旧版本链的保留/迁移策略",
      },
    ],
    invariant:
      "handler 顺序与版本是 snapshot 的一部分；终局 decision 短路不可被重排。",
    evidence: [evidence.chain, evidence.coreIds, evidence.resume],
  }),
] as const

const recovery = [
  concept({
    id: "crash-window-replay",
    term: "崩溃窗口回放",
    english: "Crash-window replay",
    definition:
      "本卡只描述 Compact 的两个已实现窗口：W1 是 Completed(Compact) 已提交但 TranscriptCompacted 未提交；W2 是 compaction 已提交但 IterationCompleted 未提交。resume 复用 summary decision，并按缺失事实补齐 Compact 效果；不能把这个保证外推到 Inject 等其他 Hook decision。",
    statuses: ["当前事实", "恢复路径", "强制不变量"],
    avoid: ["重试 summary", "崩溃后重新判断是否压缩", "补写第二个 marker"],
    normal: {
      title: "完整提交链",
      summary:
        "Completed(Compact) → TranscriptCompacted → IterationCompleted，三个边界顺序固定。",
      steps: [
        "journal summary decision",
        "kernel append compaction+companion",
        "commit iteration boundary",
      ],
      eventRows: [
        {
          seq: "68",
          type: "HookInvocationCompleted",
          scope: "internal",
          fact: "Compact(summary=s_3)",
          tone: "internal",
        },
        {
          seq: "69",
          type: "TranscriptCompacted",
          scope: "product",
          fact: "compacted_iteration=3",
          tone: "fact",
        },
        {
          seq: "70",
          type: "IterationCompleted",
          scope: "product",
          fact: "iteration=3",
          tone: "fact",
        },
      ],
    },
    failure: {
      title: "W1 / W2 任一点崩溃",
      summary:
        "W1 缺 compaction fact；W2 缺 iteration boundary，但摘要 decision 与/或 marker 已 durable。",
      steps: [
        "W1：重放 summary 并执行一次 compaction",
        "W2：应用既有 marker，用 compacted_iterations 跳过重复",
        "均不重调 summary LLM",
      ],
    },
    recovery: {
      title: "从最早未完成效果继续",
      summary: "replay 依据已存在 facts 判断边界，不依赖进程内 checkpoint。",
      steps: [
        "读取 journal/ledger",
        "识别 W1 或 W2",
        "执行缺失效果",
        "验证唯一 compaction/iteration",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "尚无 production summary handler",
        trigger: "希望在 stock runtime 自然制造 W1/W2",
        impact:
          "当前只能验证 kernel/storage mechanism，不能宣称生产 producer E2E",
        owner: "H5b/H5c",
        verification: "生产者证据与 seeded consumer 证据必须分开记录",
      },
    ],
    invariant:
      "Compact W1/W2 recovery 只能补齐尚未 durable 的效果，不能重新生成已 journaled summary；其他 decision 由各自 continuation 合同决定。",
    evidence: [evidence.compactionTests, evidence.resume, evidence.compaction],
  }),
  concept({
    id: "tool-at-least-once",
    term: "Tool at-least-once",
    definition:
      "ToolExecutionStarted 已提交而结果未提交的调用，在 resume 时使用同一 CallId 原样重执行。已存在合法 result 的调用绝不重做；缺失结果后缀按顺序补齐。",
    statuses: ["当前事实", "潜在风险", "非目标"],
    avoid: ["Tool exactly-once", "kernel 自动幂等", "生成新 CallId 重试"],
    normal: {
      title: "Started 与 result 成对",
      summary:
        "外部 Tool 只在 Started durable ack 后调用，结果以唯一 role=tool MessageAppended 提交。",
      steps: [
        "ToolExecutionStarted(call_9)",
        "tool.call(call_9)",
        "MessageAppended(role=tool, call_9)",
        "继续下一 call/iteration",
      ],
      eventRows: [
        {
          seq: "51",
          type: "ToolExecutionStarted",
          scope: "internal",
          fact: "call_id=call_9",
          tone: "internal",
        },
        {
          seq: "52",
          type: "MessageAppended",
          scope: "product",
          fact: "role=tool / call_id=call_9",
          tone: "fact",
        },
      ],
    },
    failure: {
      title: "Started 后、result 前崩溃",
      summary: "PG 只能证明调用允许开始，不能证明外部副作用是否发生。",
      steps: [
        "ledger 只含 Started",
        "resume 识别缺失后缀",
        "用 call_9 重执行",
        "只提交一个最终 result",
      ],
    },
    recovery: {
      title: "严格前缀对账",
      summary:
        "唯一、连续、有序的 result 前缀可复用；重复、稀疏、乱序、未知 CallId 均 fail closed。",
      steps: [
        "读取 expected calls",
        "验证 committed result 前缀",
        "重试缺失后缀",
        "跳过已完成 calls",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "不可幂等副作用重复",
        trigger: "第一次调用已改变外部世界，但 response/durable result 丢失",
        impact: "resume 重执行造成重复付款、发送或写入",
        owner: "Tool service / composition",
        verification:
          "使用 CallId 作为幂等键；无幂等能力的高风险 Tool 必须审批/补偿",
      },
    ],
    invariant:
      "同一缺失调用重试必须复用 CallId；kernel 只承诺 at-least-once，不声称 exactly-once。",
    evidence: [
      evidence.toolExecutor,
      evidence.resume,
      evidence.resumeTests,
      evidence.resumeSpec,
    ],
  }),
  concept({
    id: "replay-modes",
    term: "Replay 双模式",
    definition:
      "应用 TranscriptCompacted 时，`upto <= 已重建长度` 是全量流绝对 splice；`upto` 超出当前窗口则把已重建消息当保留后缀、summary 前置到 index 0。kernel 对两者使用同一事件语义。",
    statuses: ["当前事实", "恢复路径", "失败模式"],
    avoid: ["增量重放", "部分重放当不同事件", "在线修复 pointer"],
    normal: {
      title: "fast window replay",
      summary:
        "组合层用 retained pointer 从保留后缀读取较小窗口，kernel 以前置 marker 恢复等价 context。",
      steps: [
        "读取 latest companion",
        "验证 pointer 指向同 runtime message",
        "以 pointer 建 replay window",
        "应用 TranscriptCompacted window mode",
      ],
    },
    failure: {
      title: "pointer 指向错误事实",
      summary:
        "retained_from_event_seq 是加速提示；错误时不能把不完整窗口交给 kernel。",
      steps: [
        "storage boundary 验证失败",
        "放弃 fast path",
        "从 event_seq=1 full replay",
        "不写回 repair",
      ],
    },
    recovery: {
      title: "纯内存 full replay fallback",
      summary:
        "只要 discriminator/companion 核心 facts 完整，pointer-only 损坏不会阻塞恢复。",
      steps: [
        "保留损坏值作为证据",
        "读取完整 ledger",
        "绝对 splice 应用所有 compactions",
        "得到等价 context",
      ],
    },
    risks: [
      {
        level: "审阅提示",
        title: "full replay 资源增长",
        trigger: "永久 ledger 很长且 pointer 长期损坏",
        impact: "每次恢复 CPU/内存和 PG 读取成本升高",
        owner: "storage operations",
        verification:
          "监控 fallback，离线修复数据；不得为性能在线猜测或覆盖 pointer",
      },
      {
        level: "已登记",
        title: "full replay 接受 overshooting upto",
        trigger:
          "旁路 SQL 把 structurally valid companion 的 upto 改到超过当前 rebuilt message 长度",
        impact:
          "当前 kernel/API 会把它解释成 retained-window mode 并前置 summary，无法仅凭该字段判定语义损坏",
        owner: "P4a malformed fixture / future compaction validation",
        verification:
          "不要声称所有 semantic companion corruption 均已拒绝；在 disposable fixture 中固定预期后再决定是否增加 full-mode cut fence",
      },
    ],
    invariant:
      "两种合法模式结果必须等价；pointer 无效只降级性能。missing/incomplete/identity/type/summary decode 等已严格校验的 companion 损坏 fail closed；overshooting upto 仍是已登记边界。",
    evidence: [
      evidence.resume,
      evidence.postgresQueries,
      evidence.postgresTests,
      evidence.compactionSpec,
    ],
  }),
] as const

const compaction = [
  concept({
    id: "compaction-companion",
    term: "压缩 companion",
    english: "transcript_compactions",
    definition:
      "与 TranscriptCompacted discriminator 在同一事务提交的 typed companion，保存 summary、upto、compacted_iteration 与 retained_from_event_seq。原始 message 永久留在 ledger。",
    statuses: ["当前事实", "强制不变量", "失败模式"],
    avoid: ["压缩历史", "快照", "compact.jsonl", "删除原消息"],
    normal: {
      title: "discriminator 与 companion 原子共生",
      summary:
        "event row 固定空 payload；typed facts 位于 companion，二者通过同 runtime/event_seq 互相约束。",
      steps: [
        "分配 event_seq",
        "插入 TranscriptCompacted discriminator",
        "插入 companion",
        "更新 high-water",
        "单事务 commit",
      ],
    },
    failure: {
      title: "缺 companion 或非法关系",
      summary:
        "discriminator 存在但 companion 缺失、挂错 event type、summary 解码失败都属于 durable corruption。",
      steps: [
        "strict query/decoder 报错",
        "view/history/resume/dispatcher fail closed",
        "不调用外部能力",
        "不在线重建",
      ],
    },
    recovery: {
      title: "核心损坏没有在线 fallback",
      summary:
        "只有 retained pointer 这类加速字段可降级；summary/identity/discriminator 关系必须离线修复。",
      steps: [
        "隔离 runtime",
        "保存坏行证据",
        "受控恢复/迁移",
        "重新运行 strict checks",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "原始消息永久保留",
        trigger: "误把 compaction 当删除/隐私清理能力",
        impact: "敏感原文仍存在于 durable_events 与备份",
        owner: "数据治理",
        verification: "retention/delete 另行设计；UI marker 不得声称已删除历史",
      },
    ],
    invariant:
      "discriminator、companion 与 high-water 同事务；核心 companion 缺失/不一致一律 fail closed。",
    evidence: [
      evidence.postgres,
      evidence.postgresQueries,
      evidence.postgresTests,
      evidence.compactionSpec,
    ],
  }),
  concept({
    id: "mechanism-policy-separation",
    term: "机制 / 策略分离",
    definition:
      "kernel 只校验并执行 Compact，零触发与摘要策略。何时压缩、用什么 provider/model 生成 summary、失败如何处置都属于 handler/组合层。当前生产组合尚未注册 compaction handler。",
    statuses: ["当前事实", "明确延期", "非目标"],
    avoid: ["kernel 自动压缩", "智能压缩", "把测试 fixture 当生产策略"],
    normal: {
      title: "机制可测，策略尚未接线",
      summary:
        "测试 handler 可返回 Compact 验证 kernel/storage/replay；stock Docker 不会自然产生该 decision。",
      steps: [
        "test handler 产生 summary",
        "kernel journal decision",
        "执行 typed compaction",
        "验证 resume",
      ],
    },
    failure: {
      title: "直接 seed companion 冒充 producer E2E",
      summary:
        "seeded ledger 只能证明 consumer/storage，不证明真实触发、summary 调用或 chain version。",
      steps: [
        "标记 fixture=consumer-seeded",
        "单独记录 producer evidence",
        "H5b 前不得宣称生产 compaction",
      ],
    },
    recovery: {
      title: "未接策略时保持未压缩基线",
      summary:
        "没有 compaction handler 不影响正确性，只影响长期 context/token 成本。",
      steps: [
        "No-op Continue",
        "继续完整 committed context",
        "provider context limit 由现有错误处理",
        "等待独立 proposal",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "长上下文成本与上限",
        trigger: "AgentRuntime 历史持续增长且 provider 有 context limit",
        impact: "token 成本上涨，最终请求可能失败",
        owner: "H5b production compaction strategy",
        verification:
          "定义 usage、阈值、summary provider、失败语义与防风暴后再接线",
      },
    ],
    invariant:
      "没有策略时 kernel 仍正确；任何生产 compaction 声明必须有真实 handler producer 证据。",
    evidence: [
      evidence.hooks,
      evidence.compaction,
      evidence.apiHookComposition,
      evidence.compactionTests,
    ],
  }),
  concept({
    id: "compaction-marker",
    term: "压缩标记消息",
    english: "Compaction marker",
    definition:
      "压缩后替换 committed 前缀的 kernel 署名 system message：稳定前缀 `[stratum:transcript-compacted]`、换行、handler summary。它无 Tool identity，也不伪装成 user/assistant。",
    statuses: ["当前事实", "强制不变量"],
    avoid: ["摘要消息", "assistant 总结", "压缩占位符"],
    normal: {
      title: "summary 以 kernel provenance 进入基线",
      summary:
        "后续 provider 与 HookSnapshot 都能识别 marker，不把其内容归因给用户或模型。",
      steps: [
        "校验 summary",
        "构造 stable prefix system message",
        "splice committed prefix",
        "写 TranscriptCompacted",
      ],
    },
    failure: {
      title: "summary 伪装成其他 role",
      summary:
        "会污染对话归因与 Tool pairing；kernel 不接受 handler 自造 message role。",
      steps: [
        "decision 只携 summary body",
        "kernel 独占 marker shape",
        "非法 summary/shape fail closed",
      ],
    },
    recovery: {
      title: "从 typed event 重建 marker",
      summary:
        "恢复不信任历史 UI 文本，而由 TranscriptCompacted typed fields 重新创建 stable marker。",
      steps: [
        "strict decode companion",
        "构造 marker",
        "应用 full/window splice",
        "继续 replay",
      ],
    },
    risks: [
      {
        level: "已登记",
        title: "摘要质量与幻觉",
        trigger: "未来 handler 生成遗漏事实或错误 summary",
        impact:
          "被压缩前缀仍在 ledger，但后续模型只看 marker + retained suffix",
        owner: "H5b summary policy",
        verification:
          "定义质量门槛、不合格处置与人工语义验收；kernel 不替代策略判断",
      },
    ],
    invariant:
      "marker shape 与 provenance 由 kernel 固定；summary body 不能控制 role、CallId 或 stable prefix。",
    evidence: [
      evidence.compaction,
      evidence.durableEvent,
      evidence.compactionTests,
    ],
  }),
] as const

const chapters: readonly ManualChapter[] = [
  {
    id: "first-principles",
    navLabel: "第一性模型",
    title: "从事实出发，而不是从进程状态出发",
    thesis:
      "Stratum 把可重建性作为 kernel 的首要约束：非确定性先固化，snapshot 固定的身份、可装配的同版本组件与相同运行 limits 再和 durable facts 共同驱动唯一下一步。",
    visual: "first-principles",
    concepts: firstPrinciples,
  },
  {
    id: "identity-runtime",
    navLabel: "身份与运行",
    title: "定义身份与运行身份必须永远分开",
    thesis:
      "AgentId 固定不可变行为定义，AgentRuntimeId 固定长期运行聚合；事件、恢复与实时流只按后者分区。",
    visual: "identity-map",
    concepts: identities,
  },
  {
    id: "context-iteration",
    navLabel: "上下文与迭代",
    title: "持久基线、一次性视图与安全切割点",
    thesis:
      "临时 request view 可以灵活变化，但只有 durable facts 能推进 committed context；Tool pairing 完整后才越过迭代边界。",
    visual: "iteration-ledger",
    concepts: contextAndIteration,
  },
  {
    id: "hooks-journal",
    navLabel: "Hook 与 Journal",
    title: "宽读、窄写、先记录，再执行",
    thesis:
      "handler 只返回受限 decision；HookAddress 固定语义位置，Completed record 固定非确定性结果与短路选择。",
    visual: "hook-journal",
    concepts: hooksAndJournal,
  },
  {
    id: "crash-recovery",
    navLabel: "崩溃与恢复",
    title: "恢复是补齐缺失效果，不是重新猜一次",
    thesis:
      "每个 crash window 都由已提交边界定义；Tool 缺失后缀可重做，已完成 Hook 与 Tool result 不重做。",
    visual: "crash-recovery",
    concepts: recovery,
  },
  {
    id: "compaction-risk",
    navLabel: "压缩与风险",
    title: "机制已经耐久，生产策略仍明确延期",
    thesis:
      "compaction companion、replay 与 marker 已可证明；何时触发、如何摘要、如何演进 chain 属于独立后续设计。",
    visual: "compaction-modes",
    concepts: compaction,
  },
]

export const contextManual: ContextManual = {
  title: "Stratum 运行时现场手册",
  description:
    "面向人与 Agent 协同开发的静态领域地图：沿一次 AgentRuntime 的运行路线解释事实、不变量、失败、恢复与尚未解决的风险。所有 ID 与事件均为合成示例。",
  checkedAtCommit,
  chapters,
}
