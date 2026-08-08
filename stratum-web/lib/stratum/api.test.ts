import { describe, expect, it } from "vitest"

import { createStratumApi } from "@/lib/stratum/api"

/**
 * 协议形状 fixture 对齐 crates/stratum-api/src/dto.rs：
 * GET /v1/agent-templates 响应 {"templates": [...]}（AgentTemplatesResponse）。
 */

const TEMPLATES_PAYLOAD = {
  templates: [
    {
      agent_name: "default",
      model_config: { model: "anthropic:claude-sonnet", parameters: {} },
    },
    {
      agent_name: "researcher",
      model_config: { model: "openai:gpt-5", parameters: { thinking: "low" } },
    },
  ],
}

function jsonFetcher(payload: unknown, status = 200): typeof fetch {
  return (() =>
    Promise.resolve(
      new Response(JSON.stringify(payload), {
        status,
        headers: { "content-type": "application/json" },
      })
    )) as typeof fetch
}

describe("getAgentTemplates", () => {
  it("parses the wire { templates: [...] } response shape", async () => {
    const api = createStratumApi({
      baseUrl: "http://127.0.0.1:18080",
      fetcher: jsonFetcher(TEMPLATES_PAYLOAD),
    })

    const templates = await api.getAgentTemplates()

    expect(templates).toHaveLength(2)
    expect(templates[0]?.agent_name).toBe("default")
    expect(templates[0]?.model_config.model).toBe("anthropic:claude-sonnet")
    // 调用方对首个元素做默认值派生（agentTemplates[0]）：必须不是 undefined
    expect(templates[0]).toBeDefined()
  })

  it("returns an empty array when the catalog is empty", async () => {
    const api = createStratumApi({
      baseUrl: "http://127.0.0.1:18080",
      fetcher: jsonFetcher({ templates: [] }),
    })

    expect(await api.getAgentTemplates()).toEqual([])
  })
})

describe("sendMessage", () => {
  it("sends the raw text verbatim (trim is only an emptiness check upstream)", async () => {
    const bodies: string[] = []
    const fetcher = ((_input: unknown, init?: RequestInit) => {
      bodies.push(String(init?.body))
      return Promise.resolve(
        new Response(
          JSON.stringify({ agent_id: "a", session_id: "s", turn_id: "t" }),
          { status: 200, headers: { "content-type": "application/json" } }
        )
      )
    }) as typeof fetch
    const api = createStratumApi({
      baseUrl: "http://127.0.0.1:18080",
      fetcher,
    })

    await api.sendMessage("agent-1", {
      text: "  padded input  ",
      expectedCurrentTurnId: null,
    })

    expect(JSON.parse(bodies[0] ?? "{}")).toMatchObject({
      text: "  padded input  ",
      expected_current_turn_id: null,
    })
  })
})
