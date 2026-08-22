/**
 * 落地页文案字典。zh 为形状权威（typeof 推导类型），en 必须逐 key 对齐。
 * 语气纪律（PRODUCT.md）：直接、克制、可信；不拟人化 AI，不伪造数据。
 * 注意：zh 不加 as const——PageContent 需要宽松的 string 类型供 en 对齐。
 */

const zh = {
  skipToContent: "跳到主要内容",
  nav: {
    mechanism: "机制",
    depth: "架构",
    quickstart: "自托管",
    github: "GitHub",
  },
  hero: {
    eyebrow: "RUST-FIRST AGENT RUNTIME",
    titleA: "把任务交给",
    titleAccent: "Agent",
    titleB: "，它会把事做完",
    sub: "对话发起，Runtime 推进。审批、取消、恢复都在你手里，过程随时可以查看。",
    pillPlaceholder: "描述一个任务，Stratum 开始执行…",
    pillSubmit: "开始",
    moreSuggestions: "更多",
    suggestions: ["调研一个主题并给出报告", "整理这个代码库的结构", "每天定时抓取行业动态"],
    scrollHint: "下潜 · 看它怎么做事",
  },
  mechanism: {
    eyebrow: "MECHANISM · 机制",
    titleA: "你让渡的是执行，不是",
    titleAccent: "control",
    items: [
      {
        title: "持续推进",
        body: "Agent 自主推进多步任务。你交代目标，它负责走完中间的每一步。",
      },
      {
        title: "按需审批",
        body: "受控操作在执行前停下来等你批准。哪些操作算受控，由你定义。",
      },
      {
        title: "取消与恢复",
        body: "随时叫停，随时从断点继续。执行状态持久化，叫停不等于丢失。",
      },
      {
        title: "渐进透明",
        body: "思考与工具调用默认折叠，需要时逐层展开——细节永远在，但不打扰。",
      },
    ],
  },
  depth: {
    eyebrow: "UNDER THE SURFACE · 深处",
    titleA: "深处没有魔法，只有",
    titleAccent: "engineering",
    facts: [
      {
        title: "Rust-first 内核",
        body: "类型安全、可观测、默认安全的执行内核，按能力分层的 workspace 架构。",
      },
      {
        title: "Postgres 执行账本",
        body: "每一次状态变化都是一条持久化事件。账本是唯一执行真相，可回放、可审计。",
      },
      {
        title: "事件溯源恢复",
        body: "从任一事件序列号重建现场。断点续跑是精确恢复，不是从头重跑。",
      },
      {
        title: "全链路遥测",
        body: "OpenTelemetry trace 贯通 HTTP 请求、turn 执行与 LLM 调用，延迟与失败有据可查。",
      },
    ],
    traceCaption: "一个 turn 的执行轨迹 · 合成演示数据",
    traceRunning: "RUNNING",
    traceApproval: "AWAITING APPROVAL",
    traceSynced: "LEDGER SYNCED",
    quickstartTitle: "自托管，三条命令",
    quickstartNote: "需要 Docker 与 Rust 工具链",
  },
  cta: {
    eyebrow: "OPEN SOURCE · SELF-HOSTED",
    titleA: "开源，自托管，",
    titleAccent: "yours",
    sub: "你的数据、你的模型、你的规则。",
    primary: "在 GitHub 上查看",
    secondary: "快速开始",
  },
  footer: {
    tagline: "Rust-first Agent Runtime 与工作流编排系统",
    rights: "运筹 Stratum",
    gallery: "组件库",
  },
  gallery: {
    back: "返回首页",
    eyebrow: "STRATUM SITE · UI",
    title: "组件与样式",
    sub: "底座的一切：色板、字体、组件，全部以纸墨主题实拍。",
    sections: {
      colors: "色彩",
      type: "排版",
      buttons: "按钮",
      cards: "卡片",
      chips: "状态标签",
      prompt: "对话盒",
      terminal: "终端卡",
      trace: "执行轨迹",
      toggle: "语言切换",
      ink: "熔墨背景",
    },
  },
  conversation: {
    title: "对话",
    newChat: "新对话",
    history: "历史对话",
    historyEmpty: "暂无历史对话",
    resources: "资源",
    emptyTitle: "从描述一个任务开始",
    emptyNote: "Runtime 尚未接入此站点 —— 当前为界面预览",
    menu: "菜单",
    navAria: "站点导航",
    modelLabel: "模型",
    models: [
      { name: "跟随 Agent 配置", hint: "使用 Agent definition 的模型" },
      { name: "快速模型", hint: "低延迟场景" },
      { name: "长上下文模型", hint: "大文档与长任务" },
    ],
    searchToggle: "联网",
    suggestionsLabel: "建议",
    send: "发送",
    stop: "停止",
    charsUnit: "字",
    collapse: "收起侧栏",
    expand: "展开侧栏",
  },
};

export type PageContent = typeof zh;

const en: PageContent = {
  skipToContent: "Skip to content",
  nav: {
    mechanism: "Mechanism",
    depth: "Architecture",
    quickstart: "Self-host",
    github: "GitHub",
  },
  hero: {
    eyebrow: "RUST-FIRST AGENT RUNTIME",
    titleA: "Hand the task to an ",
    titleAccent: "agent",
    titleB: ", and it actually finishes",
    sub: "Start in conversation. The runtime keeps going. Approval, cancel and resume stay in your hands, with the process visible whenever you want it.",
    pillPlaceholder: "Describe a task. Stratum starts working…",
    pillSubmit: "Go",
    moreSuggestions: "More",
    suggestions: ["Research a topic into a report", "Map this codebase's structure", "Watch industry news daily"],
    scrollHint: "Dive · see how it works",
  },
  mechanism: {
    eyebrow: "MECHANISM",
    titleA: "You delegate execution, not ",
    titleAccent: "control",
    items: [
      {
        title: "Steady progress",
        body: "Agents drive multi-step tasks on their own. You set the goal; they walk every step between.",
      },
      {
        title: "Approval on demand",
        body: "Guarded operations pause for your approval before running. You define what counts as guarded.",
      },
      {
        title: "Cancel and resume",
        body: "Stop anytime, resume from the exact checkpoint. Execution state is persisted, not lost.",
      },
      {
        title: "Progressive transparency",
        body: "Thinking and tool calls stay folded by default, expandable layer by layer — always there, never noisy.",
      },
    ],
  },
  depth: {
    eyebrow: "UNDER THE SURFACE",
    titleA: "No magic down here, only ",
    titleAccent: "engineering",
    facts: [
      {
        title: "Rust-first kernel",
        body: "A type-safe, observable, safe-by-default execution kernel in a capability-layered workspace.",
      },
      {
        title: "Postgres ledger",
        body: "Every state change is a persisted event. The ledger is the single source of truth — replayable and auditable.",
      },
      {
        title: "Event-sourced recovery",
        body: "Rebuild the scene from any event sequence number. Resuming is precise recovery, not a rerun.",
      },
      {
        title: "End-to-end telemetry",
        body: "OpenTelemetry traces run through HTTP, turn execution and LLM calls. Latency and failure leave evidence.",
      },
    ],
    traceCaption: "Execution trace of one turn · synthetic demo data",
    traceRunning: "RUNNING",
    traceApproval: "AWAITING APPROVAL",
    traceSynced: "LEDGER SYNCED",
    quickstartTitle: "Self-host in 3 commands",
    quickstartNote: "Requires Docker and the Rust toolchain",
  },
  cta: {
    eyebrow: "OPEN SOURCE · SELF-HOSTED",
    titleA: "Open source. Self-hosted. ",
    titleAccent: "Yours.",
    sub: "Your data, your models, your rules.",
    primary: "View on GitHub",
    secondary: "Quickstart",
  },
  footer: {
    tagline: "A Rust-first agent runtime and workflow orchestration system",
    rights: "Stratum",
    gallery: "Components",
  },
  gallery: {
    back: "Back to home",
    eyebrow: "STRATUM SITE · UI",
    title: "Components & tokens",
    sub: "Everything in the foundation: palette, type and components, rendered live in the paper-ink theme.",
    sections: {
      colors: "Colors",
      type: "Typography",
      buttons: "Buttons",
      cards: "Cards",
      chips: "Status chips",
      prompt: "Prompt box",
      terminal: "Terminal card",
      trace: "Trace ribbon",
      toggle: "Language toggle",
      ink: "Molten ink",
    },
  },
  conversation: {
    title: "Conversation",
    newChat: "New conversation",
    history: "History",
    historyEmpty: "No conversations yet",
    resources: "Resources",
    emptyTitle: "Start by describing a task",
    emptyNote: "Runtime not connected to this site yet — UI preview",
    menu: "Menu",
    navAria: "Site navigation",
    modelLabel: "Model",
    models: [
      { name: "Follow agent config", hint: "Uses the Agent definition's model" },
      { name: "Fast model", hint: "Low-latency tasks" },
      { name: "Long-context model", hint: "Large docs and long tasks" },
    ],
    searchToggle: "Search",
    suggestionsLabel: "Suggest",
    send: "Send",
    stop: "Stop",
    charsUnit: "chars",
    collapse: "Collapse sidebar",
    expand: "Expand sidebar",
  },
};

export type Content = { zh: PageContent; en: PageContent };

export const content: Content = { zh, en };
