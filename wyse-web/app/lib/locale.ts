export const LOCALE_STORAGE_KEY = "wyse-locale" as const

export type Locale = "zh" | "en"
export type MessageKey = keyof typeof messages.zh

const supportedLocales = new Set<Locale>(["zh", "en"])

export const messages = {
  zh: {
    "hero.title": "构建类型安全的智能体",
    "hero.body": "面向 Agent、工具与可靠执行路径的 Rust-first runtime。",
    "hero.enter": "进入工作台",
    "nav.chat": "对话",
    "nav.orchestration": "智能体编排",
    "chat.sessions": "会话",
    "chat.recent": "最近使用",
    "chat.thread": "发布计划",
    "chat.title": "开始一段可靠的协作",
    "chat.body": "向运行时发出请求，并将工具调用与结果保留在同一条上下文中。",
    "chat.prompt": "给运行时发送消息",
    "chat.send": "发送",
    "orchestration.library": "编排库",
    "orchestration.agents": "智能体",
    "orchestration.workflows": "工作流",
    "orchestration.tools": "工具",
    "orchestration.title": "发布简报工作流",
    "orchestration.trigger": "触发",
    "orchestration.agent": "研究智能体",
    "orchestration.result": "提交结果",
    "locale.toggle": "切换显示语言",
    "locale.option.zh": "中文",
    "locale.option.en": "EN",
    "theme.toggle": "切换深色模式",
  },
  en: {
    "hero.title": "Build typed agents",
    "hero.body":
      "A Rust-first runtime for agents, tools, and reliable execution paths.",
    "hero.enter": "Open workspace",
    "nav.chat": "Chat",
    "nav.orchestration": "Agent Orchestration",
    "chat.sessions": "Sessions",
    "chat.recent": "Recent",
    "chat.thread": "Release brief",
    "chat.title": "Start a reliable collaboration",
    "chat.body":
      "Send requests to the runtime and keep tool calls with their results in one context.",
    "chat.prompt": "Message the runtime",
    "chat.send": "Send",
    "orchestration.library": "Orchestration library",
    "orchestration.agents": "Agents",
    "orchestration.workflows": "Workflows",
    "orchestration.tools": "Tools",
    "orchestration.title": "Release brief workflow",
    "orchestration.trigger": "Trigger",
    "orchestration.agent": "Research agent",
    "orchestration.result": "Commit result",
    "locale.toggle": "Change display language",
    "locale.option.zh": "中文",
    "locale.option.en": "EN",
    "theme.toggle": "Toggle dark theme",
  },
} as const satisfies Record<Locale, Record<string, string>>

export function isLocale(value: string | null): value is Locale {
  return value !== null && supportedLocales.has(value as Locale)
}

export function resolveLocale(
  stored: string | null,
  systemLanguage: string | undefined
): Locale {
  if (isLocale(stored)) return stored
  return systemLanguage?.toLowerCase().startsWith("en") ? "en" : "zh"
}
