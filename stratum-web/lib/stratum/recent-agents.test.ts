import { describe, expect, it } from "vitest"

import {
  createMemoryStorage,
  loadRecentAgentRuntimes,
  rememberRecentAgentRuntime,
  removeRecentAgentRuntime,
} from "@/lib/stratum/recent-agents"

const recent = (agentRuntimeId: string, title: string) => ({
  agentRuntimeId,
  agentId: "shared-agent-definition",
  agentName: "researcher",
  agentVersion: "author-tag",
  title,
  lastOpenedAt: "2026-08-09T00:00:00Z",
})

describe("recent AgentRuntime navigation", () => {
  it("keeps separate conversations that pin the same template version", () => {
    const storage = createMemoryStorage()
    rememberRecentAgentRuntime(storage, recent("runtime-1", "First"))
    rememberRecentAgentRuntime(storage, recent("runtime-2", "Second"))

    expect(
      loadRecentAgentRuntimes(storage).map((runtime) => runtime.agentRuntimeId)
    ).toEqual(["runtime-2", "runtime-1"])
  })

  it("replaces and removes entries only by AgentRuntimeId", () => {
    const storage = createMemoryStorage()
    rememberRecentAgentRuntime(storage, recent("runtime-1", "Old title"))
    rememberRecentAgentRuntime(storage, recent("runtime-1", "New title"))
    rememberRecentAgentRuntime(storage, recent("runtime-2", "Other runtime"))
    removeRecentAgentRuntime(storage, "runtime-1")

    expect(loadRecentAgentRuntimes(storage)).toEqual([
      recent("runtime-2", "Other runtime"),
    ])
  })
})
