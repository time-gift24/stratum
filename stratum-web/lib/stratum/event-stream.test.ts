import { describe, expect, it, vi } from "vitest"

import { ApiError } from "@/lib/stratum/api"
import {
  parseAgentRuntimeStreamFrame,
  readSseStream,
  subscribeToAgentRuntimeEvents,
} from "@/lib/stratum/event-stream"

const TURN_IDENTITY = {
  protocol_version: 1,
  agent_runtime_id: "runtime-1",
  agent_id: "agent-definition-1",
  session_id: "session-1",
  turn_id: "turn-1",
  created_at: "2026-08-09T00:00:00Z",
}

const CONNECTION_IDENTITY = {
  protocol_version: 1,
  agent_runtime_id: "runtime-1",
  agent_id: "agent-definition-1",
  created_at: "2026-08-09T00:00:00Z",
}

const USAGE = { input_tokens: 1, output_tokens: 2, total_tokens: 3 }
const SUMMARY = {
  role: "system",
  content: { type: "text", data: "summary" },
}

const PRODUCT_EVENTS = [
  { type: "loop_started" },
  {
    type: "message_appended",
    data: {
      message: {
        role: "assistant",
        content: { type: "text", data: "hello" },
        tool_calls: [
          { call_id: "call-1", name: "lookup", arguments: { q: "x" } },
        ],
        reasoning_content: "reasoning",
      },
    },
  },
  {
    type: "tool_approval_requested",
    data: {
      approval_id: "approval-1",
      call_id: "call-1",
      tool_name: "write_file",
      arguments: { path: "safe.txt" },
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
    data: { summary: SUMMARY, compacted_iteration: 3 },
  },
  { type: "iteration_completed", data: { iteration: 4, usage: USAGE } },
  {
    type: "loop_finished",
    data: { finish_reason: "stop", usage: USAGE },
  },
  {
    type: "loop_failed",
    data: { error_text: "safe failure", usage: USAGE },
  },
  { type: "loop_cancelled", data: { usage: USAGE } },
] as const

function durableFrame(event: unknown): string {
  return JSON.stringify({
    ...TURN_IDENTITY,
    kind: "durable",
    event_seq: "42",
    event_version: 1,
    event,
  })
}

function telemetryFrame(event: unknown, telemetrySeq = "0"): string {
  return JSON.stringify({
    ...TURN_IDENTITY,
    kind: "telemetry",
    llm_call_id: "llm-call-1",
    telemetry_seq: telemetrySeq,
    durable_before_event_seq: "41",
    event,
  })
}

function sseStream(lines: string): ReadableStream<Uint8Array> {
  return new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(lines))
      controller.close()
    },
  })
}

describe("parseAgentRuntimeStreamFrame", () => {
  it("accepts every complete public durable product variant", () => {
    for (const event of PRODUCT_EVENTS) {
      expect(parseAgentRuntimeStreamFrame(durableFrame(event))).toMatchObject({
        protocol_version: 1,
        kind: "durable",
        agent_runtime_id: "runtime-1",
        agent_id: "agent-definition-1",
        event_seq: "42",
        event_version: 1,
        event,
      })
    }
  })

  it("rejects unknown, extra, and malformed product variants", () => {
    const invalid = [
      { type: "tool_execution_started", data: {} },
      { type: "loop_started", data: {} },
      {
        type: "message_appended",
        data: {
          message: {
            role: "assistant",
            content: { type: "text", data: 7 },
          },
        },
      },
      {
        type: "tool_approval_requested",
        data: {
          approval_id: "approval-1",
          call_id: "call-1",
          tool_name: "writer",
          arguments: {},
          tool_kind: "mutating",
          danger_level: "high",
        },
      },
      {
        type: "transcript_compacted",
        data: { summary: SUMMARY, compacted_iteration: -1 },
      },
      {
        type: "transcript_compacted",
        data: {
          summary: { ...SUMMARY, role: "user" },
          compacted_iteration: 1,
        },
      },
      {
        type: "loop_finished",
        data: { finish_reason: "stop", usage: USAGE, raw: "provider" },
      },
    ]

    for (const event of invalid)
      expect(parseAgentRuntimeStreamFrame(durableFrame(event))).toBeUndefined()
  })

  it("accepts every telemetry variant with decimal-string call sequence", () => {
    const events = [
      { type: "llm_started" },
      { type: "text_delta", data: { delta: "text" } },
      { type: "reasoning_delta", data: { delta: "reasoning" } },
      {
        type: "tool_call_delta",
        data: {
          call_id: "tool-call-1",
          name: "lookup",
          arguments_delta: "{",
        },
      },
      {
        type: "llm_finished",
        data: { finish_reason: "stop", usage: USAGE },
      },
    ]

    events.forEach((event, index) => {
      const sequence = String(index)
      expect(
        parseAgentRuntimeStreamFrame(telemetryFrame(event, sequence))
      ).toMatchObject({
        kind: "telemetry",
        telemetry_seq: sequence,
        durable_before_event_seq: "41",
        event,
      })
    })
  })

  it("rejects numeric/malformed sequences and unsafe versions", () => {
    const numericEventSeq = JSON.parse(durableFrame({ type: "loop_started" }))
    numericEventSeq.event_seq = 42
    const leadingZero = JSON.parse(durableFrame({ type: "loop_started" }))
    leadingZero.event_seq = "042"
    const unsafeVersion = JSON.parse(durableFrame({ type: "loop_started" }))
    unsafeVersion.event_version = Number.MAX_SAFE_INTEGER + 1
    const numericTelemetry = JSON.parse(telemetryFrame({ type: "llm_started" }))
    numericTelemetry.telemetry_seq = 0

    for (const value of [
      numericEventSeq,
      leadingZero,
      unsafeVersion,
      numericTelemetry,
    ])
      expect(
        parseAgentRuntimeStreamFrame(JSON.stringify(value))
      ).toBeUndefined()
  })

  it("requires both runtime and pinned definition identities", () => {
    const missingRuntime = JSON.parse(durableFrame({ type: "loop_started" }))
    delete missingRuntime.agent_runtime_id
    const missingDefinition = JSON.parse(durableFrame({ type: "loop_started" }))
    delete missingDefinition.agent_id
    const missingTurn = JSON.parse(durableFrame({ type: "loop_started" }))
    delete missingTurn.turn_id

    for (const value of [missingRuntime, missingDefinition, missingTurn])
      expect(
        parseAgentRuntimeStreamFrame(JSON.stringify(value))
      ).toBeUndefined()
  })

  it("rejects unknown protocol versions, frame kinds, and envelope fields", () => {
    const version = JSON.parse(durableFrame({ type: "loop_started" }))
    version.protocol_version = 2
    const kind = JSON.parse(durableFrame({ type: "loop_started" }))
    kind.kind = "snapshot"
    const extra = JSON.parse(durableFrame({ type: "loop_started" }))
    extra.metadata = { secret: true }

    for (const value of [version, kind, extra])
      expect(
        parseAgentRuntimeStreamFrame(JSON.stringify(value))
      ).toBeUndefined()
  })

  it("accepts ready with both-or-neither Turn identity and reset with neither", () => {
    expect(
      parseAgentRuntimeStreamFrame(
        JSON.stringify({
          ...CONNECTION_IDENTITY,
          kind: "control",
          event: { type: "stream_ready" },
        })
      )
    ).toMatchObject({ session_id: null, turn_id: null })
    expect(
      parseAgentRuntimeStreamFrame(
        JSON.stringify({
          ...TURN_IDENTITY,
          kind: "control",
          event: { type: "stream_ready" },
        })
      )
    ).toBeDefined()
    expect(
      parseAgentRuntimeStreamFrame(
        JSON.stringify({
          ...CONNECTION_IDENTITY,
          kind: "control",
          event: { type: "stream_reset", reason: "buffer_overflow" },
        })
      )
    ).toMatchObject({ session_id: null, turn_id: null })
    expect(
      parseAgentRuntimeStreamFrame(
        JSON.stringify({
          ...CONNECTION_IDENTITY,
          session_id: "session-1",
          kind: "control",
          event: { type: "stream_ready" },
        })
      )
    ).toBeUndefined()
  })
})

describe("subscribeToAgentRuntimeEvents", () => {
  it("uses the runtime route and opaque after_cursor", async () => {
    const ready = JSON.stringify({
      ...CONNECTION_IDENTITY,
      kind: "control",
      event: { type: "stream_ready" },
    })
    const fetcher = vi.fn((input: RequestInfo | URL) => {
      expect(String(input)).toBe(
        "http://stratum.test/v1/agent-runtimes/runtime-1/events?after_cursor=opaque%2Fcursor"
      )
      return Promise.resolve(
        new Response(sseStream(`data: ${ready}\n\n`), { status: 200 })
      )
    }) as typeof fetch
    const onFrame = vi.fn()

    await subscribeToAgentRuntimeEvents({
      baseUrl: "http://stratum.test",
      agentRuntimeId: "runtime-1",
      afterCursor: "opaque/cursor",
      fetcher,
      onFrame,
    }).done

    expect(onFrame).toHaveBeenCalledOnce()
  })

  it("rejects a stream_reset carrying an SSE id", async () => {
    const reset = JSON.stringify({
      ...CONNECTION_IDENTITY,
      kind: "control",
      event: { type: "stream_reset", reason: "buffer_overflow" },
    })
    const fetcher = (() =>
      Promise.resolve(
        new Response(sseStream(`id: forbidden\ndata: ${reset}\n\n`), {
          status: 200,
        })
      )) as typeof fetch

    await expect(
      subscribeToAgentRuntimeEvents({
        baseUrl: "http://stratum.test",
        agentRuntimeId: "runtime-1",
        fetcher,
        onFrame: () => {},
      }).done
    ).rejects.toMatchObject({ code: "unsupported_frame" })
  })

  it("rejects an empty SSE cursor instead of confusing it with cold mode", async () => {
    const ready = JSON.stringify({
      ...CONNECTION_IDENTITY,
      kind: "control",
      event: { type: "stream_ready" },
    })
    const fetcher = (() =>
      Promise.resolve(
        new Response(sseStream(`id:\ndata: ${ready}\n\n`), { status: 200 })
      )) as typeof fetch

    await expect(
      subscribeToAgentRuntimeEvents({
        baseUrl: "http://stratum.test",
        agentRuntimeId: "runtime-1",
        fetcher,
        onFrame: () => {},
      }).done
    ).rejects.toMatchObject({ code: "unsupported_frame" })
  })
})

describe("readSseStream resource bounds", () => {
  it("rejects an unterminated frame instead of buffering without bound", async () => {
    const oversized = new TextEncoder().encode("x".repeat(2 * 1024 * 1024 + 1))
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(oversized)
        controller.close()
      },
    })

    await expect(readSseStream(stream, () => {})).rejects.toMatchObject({
      name: "ApiError",
      code: "stream_frame_too_large",
    } satisfies Partial<ApiError>)
  })

  it("cancels the response body when frame handling fails", async () => {
    const cancel = vi.fn()
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode("data: {}\n\n"))
      },
      cancel,
    })

    await expect(
      readSseStream(stream, () => {
        throw new Error("frame rejected")
      })
    ).rejects.toThrow("frame rejected")
    expect(cancel).toHaveBeenCalledOnce()
  })
})
