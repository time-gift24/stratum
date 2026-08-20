import { describe, expect, it, vi } from "vitest"

import {
  createStratumApi,
  ApiError,
  type AgentDefinitionInput,
  type AgentDefinitionView,
} from "@/lib/stratum/api"
import { safeStudioErrorMessage } from "@/features/studio-management/client"

const INPUT: AgentDefinitionInput = {
  agent_name: "researcher",
  agent_version: "release-8",
  model: "openai:gpt-5",
  model_parameters: { reasoning_effort: "high" },
  tools: ["web_search"],
  prompt: "Research carefully.",
}

const VIEW: AgentDefinitionView = {
  ...INPUT,
  agent_name: "researcher",
  updated_at: "2026-08-19T00:00:00Z",
}

const resourceResponse = (): Response =>
  new Response(JSON.stringify(VIEW), {
    headers: { "content-type": "application/json", etag: '"revision-8"' },
  })

describe("Studio management client", () => {
  it("only exposes public API errors in list feedback", () => {
    expect(
      safeStudioErrorMessage(
        new ApiError("studio_unavailable", 503, "Studio 暂时不可用"),
        "无法加载 Agent"
      )
    ).toBe("Studio 暂时不可用")
    expect(
      safeStudioErrorMessage(
        new Error("internal protocol detail"),
        "无法加载 Agent"
      )
    ).toBe("无法加载 Agent")
  })

  it("sends agent_version in create and replacement bodies", async () => {
    const requests: { url: string; init?: RequestInit }[] = []
    const fetcher = vi.fn(
      (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
        requests.push({ url: String(input), init })
        return Promise.resolve(resourceResponse())
      }
    ) as typeof fetch
    const api = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher,
    })

    await api.createAgentDefinition(INPUT)
    await api.updateAgentDefinition("researcher", INPUT, '"revision-7"')

    expect(requests[0]?.url).toBe("http://stratum.test/v1/agent-definitions")
    expect(requests[0]?.init?.method).toBe("POST")
    expect(JSON.parse(String(requests[0]?.init?.body))).toEqual(INPUT)

    expect(requests[1]?.url).toBe(
      "http://stratum.test/v1/agent-definitions/researcher"
    )
    expect(requests[1]?.init?.method).toBe("PUT")
    expect(new Headers(requests[1]?.init?.headers).get("if-match")).toBe(
      '"revision-7"'
    )
    expect(JSON.parse(String(requests[1]?.init?.body))).toEqual({
      agent_version: "release-8",
      model: "openai:gpt-5",
      model_parameters: { reasoning_effort: "high" },
      tools: ["web_search"],
      prompt: "Research carefully.",
    })
  })
})
