const RECENT_AGENT_RUNTIMES_KEY = "stratum-recent-agent-runtimes-v1"
const MAX_RECENT_AGENT_RUNTIMES = 20

/** Local navigation metadata only; the runtime/view remains server truth. */
export type RecentAgentRuntime = {
  /** Conversation identity (`agent_states.id`). */
  agentRuntimeId: string
  /** Pinned immutable template-version identity (`agents.id`). */
  agentId: string
  agentName: string
  agentVersion: string
  title: string
  lastOpenedAt: string
}

export type StorageLike = Pick<Storage, "getItem" | "setItem" | "removeItem">

export const createMemoryStorage = (): StorageLike => {
  const values = new Map<string, string>()
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => values.set(key, value),
    removeItem: (key) => values.delete(key),
  }
}

export const loadRecentAgentRuntimes = (
  storage: StorageLike
): RecentAgentRuntime[] => {
  let stored: string | null
  try {
    stored = storage.getItem(RECENT_AGENT_RUNTIMES_KEY)
  } catch {
    return []
  }
  if (stored === null) return []

  try {
    const runtimes: unknown = JSON.parse(stored)
    if (Array.isArray(runtimes) && runtimes.every(isRecentAgentRuntime)) {
      const recent = runtimes
        .slice(0, MAX_RECENT_AGENT_RUNTIMES)
        .map(copyRecentAgentRuntime)
      saveRecentAgentRuntimes(storage, recent)
      return recent
    }
  } catch {
    // Remove corrupt navigation metadata below.
  }

  try {
    storage.removeItem(RECENT_AGENT_RUNTIMES_KEY)
  } catch {
    // Storage can be unavailable in private browsing or when disabled.
  }
  return []
}

export const rememberRecentAgentRuntime = (
  storage: StorageLike,
  runtime: RecentAgentRuntime
): void => {
  saveRecentAgentRuntimes(storage, [
    runtime,
    ...loadRecentAgentRuntimes(storage).filter(
      (recent) => recent.agentRuntimeId !== runtime.agentRuntimeId
    ),
  ])
}

export const removeRecentAgentRuntime = (
  storage: StorageLike,
  agentRuntimeId: string
): void => {
  const runtimes = loadRecentAgentRuntimes(storage)
  const remaining = runtimes.filter(
    (runtime) => runtime.agentRuntimeId !== agentRuntimeId
  )
  if (remaining.length !== runtimes.length)
    saveRecentAgentRuntimes(storage, remaining)
}

export const formatRelativeTime = (iso: string, locale: string): string => {
  try {
    const date = new Date(iso)
    const now = new Date()
    const seconds = Math.floor((now.getTime() - date.getTime()) / 1000)
    const formatter = new Intl.RelativeTimeFormat(locale, { numeric: "auto" })

    if (seconds < 60) return formatter.format(-seconds, "second")
    const minutes = Math.floor(seconds / 60)
    if (minutes < 60) return formatter.format(-minutes, "minute")
    const hours = Math.floor(minutes / 60)
    if (hours < 24) return formatter.format(-hours, "hour")
    const days = Math.floor(hours / 24)
    if (days < 30) return formatter.format(-days, "day")
    const months = Math.floor(days / 30)
    if (months < 12) return formatter.format(-months, "month")
    return formatter.format(-Math.floor(months / 12), "year")
  } catch {
    return iso
  }
}

function saveRecentAgentRuntimes(
  storage: StorageLike,
  runtimes: readonly RecentAgentRuntime[]
): void {
  try {
    storage.setItem(
      RECENT_AGENT_RUNTIMES_KEY,
      JSON.stringify(
        runtimes.slice(0, MAX_RECENT_AGENT_RUNTIMES).map(copyRecentAgentRuntime)
      )
    )
  } catch {
    // Storage can be unavailable in private browsing or when disabled.
  }
}

function isRecentAgentRuntime(value: unknown): value is RecentAgentRuntime {
  if (typeof value !== "object" || value === null) return false
  const runtime = value as Record<string, unknown>
  return (
    typeof runtime.agentRuntimeId === "string" &&
    typeof runtime.agentId === "string" &&
    typeof runtime.agentName === "string" &&
    typeof runtime.agentVersion === "string" &&
    typeof runtime.title === "string" &&
    typeof runtime.lastOpenedAt === "string"
  )
}

const copyRecentAgentRuntime = (
  runtime: RecentAgentRuntime
): RecentAgentRuntime => ({
  agentRuntimeId: runtime.agentRuntimeId,
  agentId: runtime.agentId,
  agentName: runtime.agentName,
  agentVersion: runtime.agentVersion,
  title: runtime.title,
  lastOpenedAt: runtime.lastOpenedAt,
})
