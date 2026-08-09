import { describe, expect, it, vi } from "vitest"

import { ApiError, compareEventSeq, createStratumApi } from "@/lib/stratum/api"

const RUNTIME_ID = "runtime-1"
const AGENT_ID = "agent-definition-1"
const MODEL_CONFIG = {
  model: "openai:gpt-5",
  parameters: { thinking: { type: "enabled", reasoning_effort: "high" } },
}

const CREATED = {
  agent_runtime_id: RUNTIME_ID,
  agent_id: AGENT_ID,
  agent_name: "researcher",
  agent_version: "release-candidate",
  created_at: "2026-08-09T00:00:00Z",
}

const VIEW = {
  agent_runtime_id: RUNTIME_ID,
  agent_id: AGENT_ID,
  agent_name: "researcher",
  agent_version: "release-candidate",
  status: "running",
  model_config: MODEL_CONFIG,
  session_id: "session-1",
  current_turn_id: "turn-1",
  snapshot_event_seq: "42",
  telemetry_floor_event_seq: "40",
  pending_approvals: [],
  latest_usage: null,
  resume_required: false,
}

const ACCEPTED = {
  agent_runtime_id: RUNTIME_ID,
  agent_id: AGENT_ID,
  session_id: "session-1",
  turn_id: "turn-1",
}

const USAGE = { input_tokens: 1, output_tokens: 2, total_tokens: 3 }
const PRODUCT_EVENTS = [
  { type: "loop_started" },
  {
    type: "message_appended",
    data: {
      message: { role: "user", content: { type: "text", data: "hello" } },
    },
  },
  {
    type: "tool_approval_requested",
    data: {
      approval_id: "approval-1",
      call_id: "call-1",
      tool_name: "writer",
      arguments: {},
      tool_kind: "write",
      danger_level: "high",
    },
  },
  {
    type: "tool_approval_resolved",
    data: { approval_id: "approval-1", decision: "approve" },
  },
  {
    type: "transcript_compacted",
    data: {
      summary: {
        role: "system",
        content: { type: "text", data: "summary" },
      },
      compacted_iteration: 1,
    },
  },
  { type: "iteration_completed", data: { iteration: 1, usage: USAGE } },
  { type: "loop_finished", data: { finish_reason: "stop", usage: USAGE } },
  { type: "loop_failed", data: { error_text: "safe", usage: USAGE } },
  { type: "loop_cancelled", data: { usage: USAGE } },
] as const

function jsonResponse(payload: unknown, status = 200): Response {
  return new Response(JSON.stringify(payload), {
    status,
    headers: { "content-type": "application/json" },
  })
}

describe("Stratum AgentRuntime API", () => {
  it("decodes catalog author tags and rejects a catalog without version", async () => {
    const valid = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() =>
        Promise.resolve(
          jsonResponse({
            templates: [
              {
                agent_name: "researcher",
                version: "release-candidate",
                model_config: MODEL_CONFIG,
              },
            ],
          })
        )) as typeof fetch,
    })

    await expect(valid.getAgentTemplates()).resolves.toMatchObject([
      { agent_name: "researcher", version: "release-candidate" },
    ])

    const invalid = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() =>
        Promise.resolve(
          jsonResponse({
            templates: [
              { agent_name: "researcher", model_config: MODEL_CONFIG },
            ],
          })
        )) as typeof fetch,
    })
    await expect(invalid.getAgentTemplates()).rejects.toMatchObject({
      code: "invalid_response",
    } satisfies Partial<ApiError>)
  })

  it("creates through the runtime route with a key-only replay body", async () => {
    const fetcher = vi.fn((input: RequestInfo | URL, init?: RequestInit) => {
      expect(String(input)).toBe("http://stratum.test/v1/agent-runtimes")
      expect(init?.method).toBe("POST")
      expect(new Headers(init?.headers).get("idempotency-key")).toBe(
        "4c4ee58d-e87d-4a63-b50d-ccf304269ca6"
      )
      expect(JSON.parse(String(init?.body))).toEqual({
        agent_name: "researcher",
        model_config: MODEL_CONFIG,
      })
      return Promise.resolve(jsonResponse(CREATED, 201))
    }) as typeof fetch
    const api = createStratumApi({ baseUrl: "http://stratum.test", fetcher })

    await expect(
      api.createAgentRuntime({
        agentName: "researcher",
        modelConfig: MODEL_CONFIG,
        idempotencyKey: "4c4ee58d-e87d-4a63-b50d-ccf304269ca6",
      })
    ).resolves.toEqual(CREATED)
    expect(fetcher).toHaveBeenCalledOnce()
  })

  it("uses AgentRuntime routes for view, history, message, resume, cancel, and approval", async () => {
    const requests: { url: string; init?: RequestInit }[] = []
    const fetcher = ((input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input)
      requests.push({ url, init })
      if (url.endsWith("/history?through_event_seq=42&limit=12"))
        return Promise.resolve(
          jsonResponse({
            items: [],
            through_event_seq: "42",
            next_before_event_seq: null,
            has_more: false,
          })
        )
      if (url.endsWith("/messages"))
        return Promise.resolve(jsonResponse(ACCEPTED, 202))
      if (url.endsWith("/resume"))
        return Promise.resolve(jsonResponse(ACCEPTED, 202))
      if (url.endsWith("/cancel") || url.includes("/approvals/"))
        return Promise.resolve(new Response(null, { status: 204 }))
      return Promise.resolve(jsonResponse(VIEW))
    }) as typeof fetch
    const api = createStratumApi({ baseUrl: "http://stratum.test/", fetcher })

    await api.getAgentRuntime(RUNTIME_ID)
    await api.getAgentRuntimeHistory(RUNTIME_ID, {
      throughSeq: "42",
      limit: 12,
    })
    await api.sendMessage(RUNTIME_ID, {
      text: "  keep whitespace  ",
      expectedCurrentTurnId: "turn-0",
      modelConfig: MODEL_CONFIG,
    })
    await api.resume(RUNTIME_ID, "turn-1")
    await api.cancel(RUNTIME_ID, "turn-1")
    await api.resolveApproval(RUNTIME_ID, "approval-1", {
      turnId: "turn-1",
      decision: "approve",
    })

    expect(requests.map(({ url }) => url)).toEqual([
      `http://stratum.test/v1/agent-runtimes/${RUNTIME_ID}`,
      `http://stratum.test/v1/agent-runtimes/${RUNTIME_ID}/history?through_event_seq=42&limit=12`,
      `http://stratum.test/v1/agent-runtimes/${RUNTIME_ID}/messages`,
      `http://stratum.test/v1/agent-runtimes/${RUNTIME_ID}/resume`,
      `http://stratum.test/v1/agent-runtimes/${RUNTIME_ID}/cancel`,
      `http://stratum.test/v1/agent-runtimes/${RUNTIME_ID}/approvals/approval-1`,
    ])
    expect(JSON.parse(String(requests[2]?.init?.body))).toEqual({
      text: "  keep whitespace  ",
      expected_current_turn_id: "turn-0",
      model_config: MODEL_CONFIG,
    })
  })

  it("accepts a 204 idempotent resume without inventing a Turn", async () => {
    const api = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() =>
        Promise.resolve(new Response(null, { status: 204 }))) as typeof fetch,
    })

    await expect(api.resume(RUNTIME_ID, "turn-1")).resolves.toBeNull()
  })

  it("accepts the oldest-item cursor on a non-empty final history page", async () => {
    const api = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() =>
        Promise.resolve(
          jsonResponse({
            items: [
              {
                event_seq: "1",
                event_version: 1,
                session_id: "session-1",
                turn_id: "turn-1",
                created_at: "2026-08-09T00:00:00Z",
                event: { type: "loop_started" },
              },
            ],
            through_event_seq: "42",
            next_before_event_seq: "1",
            has_more: false,
          })
        )) as typeof fetch,
    })

    await expect(
      api.getAgentRuntimeHistory(RUNTIME_ID, { throughSeq: "42" })
    ).resolves.toMatchObject({
      next_before_event_seq: "1",
      has_more: false,
    })
  })

  it("strictly decodes the complete public product union from history", async () => {
    const api = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() =>
        Promise.resolve(
          jsonResponse({
            items: PRODUCT_EVENTS.map((event, index) => ({
              event_seq: String(index + 1),
              event_version: 1,
              session_id: "session-1",
              turn_id: "turn-1",
              created_at: "2026-08-09T00:00:00Z",
              event,
            })),
            through_event_seq: "9",
            next_before_event_seq: "1",
            has_more: false,
          })
        )) as typeof fetch,
    })

    const history = await api.getAgentRuntimeHistory(RUNTIME_ID, {
      throughSeq: "9",
    })

    expect(history.items.map((item) => item.event.type)).toEqual(
      PRODUCT_EVENTS.map((event) => event.type)
    )
  })

  it("fails closed when a command response belongs to another runtime", async () => {
    const api = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() =>
        Promise.resolve(
          jsonResponse({ ...ACCEPTED, agent_runtime_id: "runtime-2" }, 202)
        )) as typeof fetch,
    })

    await expect(
      api.sendMessage(RUNTIME_ID, {
        text: "hello",
        expectedCurrentTurnId: null,
      })
    ).rejects.toMatchObject({ code: "protocol_identity_error" })
  })

  it("fails closed when resume acknowledges a different Turn", async () => {
    const api = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() =>
        Promise.resolve(
          jsonResponse({ ...ACCEPTED, turn_id: "turn-2" }, 202)
        )) as typeof fetch,
    })

    await expect(api.resume(RUNTIME_ID, "turn-1")).rejects.toMatchObject({
      code: "protocol_identity_error",
    })
  })

  it("rejects an unexpected successful HTTP status", async () => {
    const api = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() => Promise.resolve(jsonResponse(CREATED))) as typeof fetch,
    })

    await expect(
      api.createAgentRuntime({
        agentName: "researcher",
        idempotencyKey: "4c4ee58d-e87d-4a63-b50d-ccf304269ca6",
      })
    ).rejects.toMatchObject({ code: "invalid_response", status: 200 })
  })

  it("strictly rejects unknown success fields", async () => {
    const api = createStratumApi({
      baseUrl: "http://stratum.test",
      fetcher: (() =>
        Promise.resolve(
          jsonResponse({ ...VIEW, raw_prompt: "secret" })
        )) as typeof fetch,
    })

    await expect(api.getAgentRuntime(RUNTIME_ID)).rejects.toMatchObject({
      code: "invalid_response",
    })
  })
})

describe("event sequence helpers", () => {
  it("compares decimal strings without JavaScript number coercion", () => {
    expect(compareEventSeq("9007199254740993", "9007199254740992")).toBe(1)
    expect(
      compareEventSeq("18446744073709551615", "18446744073709551615")
    ).toBe(0)
  })
})
