import { describe, expect, it, vi } from "vitest"

import { ApiError } from "@/lib/stratum/api"
import {
  parseAgentStreamFrame,
  readSseStream,
} from "@/lib/stratum/event-stream"

/**
 * 协议形状 fixture 对齐 crates/stratum-api/src/frames.rs：
 * - frame 以 `kind` tag（control / durable / telemetry），内层 event 以
 *   `type` + 可选 `data` tag（serde adjacently tagged）。
 * - unit variants（loop_started / llm_started）序列化时不携带 `data` key。
 * - durable frame 的 event_seq 是十进制字符串，另携带 event_version。
 */

const IDENTITY = {
  protocol_version: 1,
  agent_id: "agent-1",
  session_id: "session-1",
  turn_id: "turn-1",
  created_at: "2026-01-01T00:00:00.000Z",
}

const USAGE = { input_tokens: 1, output_tokens: 2, total_tokens: 3 }
const SUMMARY = {
  role: "system",
  content: { type: "text", data: "summary" },
}

function durableFrame(event: Record<string, unknown>): string {
  return JSON.stringify({
    ...IDENTITY,
    kind: "durable",
    event_seq: "42",
    event_version: 1,
    event,
  })
}

function telemetryFrame(
  event: Record<string, unknown>,
  telemetrySeq: number | string = 0
): string {
  return JSON.stringify({
    ...IDENTITY,
    kind: "telemetry",
    llm_call_id: "call-1",
    telemetry_seq: telemetrySeq,
    durable_before_event_seq: "41",
    event,
  })
}

describe("parseAgentStreamFrame durable frames", () => {
  it("accepts the loop_started unit variant without any data key", () => {
    const frame = parseAgentStreamFrame(durableFrame({ type: "loop_started" }))

    expect(frame).toMatchObject({
      kind: "durable",
      event_seq: "42",
      event_version: 1,
      event: { type: "loop_started" },
    })
  })

  it("rejects a loop_started frame that wrongly carries data", () => {
    expect(
      parseAgentStreamFrame(durableFrame({ type: "loop_started", data: {} }))
    ).toBeUndefined()
  })

  it("accepts data variants with an object data payload", () => {
    const message = {
      role: "assistant",
      content: { type: "text", data: "hello" },
    }
    const frame = parseAgentStreamFrame(
      durableFrame({ type: "message_appended", data: { message } })
    )

    expect(frame).toMatchObject({
      kind: "durable",
      event: { type: "message_appended", data: { message } },
    })
  })

  it("accepts loop_finished with finish_reason and usage", () => {
    const data = {
      finish_reason: "stop",
      usage: USAGE,
    }
    const frame = parseAgentStreamFrame(
      durableFrame({ type: "loop_finished", data })
    )

    expect(frame).toMatchObject({
      kind: "durable",
      event: { type: "loop_finished", data },
    })
  })

  it("validates every typed durable data variant", () => {
    const events = [
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
    ]

    for (const event of events)
      expect(parseAgentStreamFrame(durableFrame(event))).toBeDefined()
  })

  it("rejects malformed fields in every durable data variant", () => {
    const malformedEvents = [
      {
        type: "message_appended",
        data: {
          message: {
            role: "assistant",
            content: { type: "text", data: 42 },
          },
        },
      },
      {
        type: "tool_approval_requested",
        data: {
          approval_id: "approval-1",
          tool_name: "write_file",
          arguments: {},
          tool_kind: "write",
          danger_level: "high",
        },
      },
      {
        type: "tool_approval_resolved",
        data: { approval_id: "approval-1", decision: "later" },
      },
      {
        type: "transcript_compacted",
        data: { summary: SUMMARY, compacted_iteration: -1 },
      },
      {
        type: "iteration_completed",
        data: {
          iteration: 4,
          usage: { input_tokens: 1, output_tokens: 2 },
        },
      },
      {
        type: "loop_finished",
        data: { finish_reason: 1, usage: USAGE },
      },
      { type: "loop_failed", data: { usage: USAGE } },
      {
        type: "loop_cancelled",
        data: {
          usage: { input_tokens: 1, output_tokens: -1, total_tokens: 0 },
        },
      },
    ]

    for (const event of malformedEvents)
      expect(parseAgentStreamFrame(durableFrame(event))).toBeUndefined()
  })

  it("rejects data variants without a data object", () => {
    expect(
      parseAgentStreamFrame(durableFrame({ type: "message_appended" }))
    ).toBeUndefined()
    expect(
      parseAgentStreamFrame(
        durableFrame({ type: "loop_finished", data: "stop" })
      )
    ).toBeUndefined()
  })

  it("rejects unknown durable event variants", () => {
    expect(
      parseAgentStreamFrame(
        durableFrame({ type: "tool_execution_started", data: {} })
      )
    ).toBeUndefined()
  })

  it("rejects non-decimal-string event_seq", () => {
    const raw = JSON.parse(durableFrame({ type: "loop_started" }))
    raw.event_seq = 42
    expect(parseAgentStreamFrame(JSON.stringify(raw))).toBeUndefined()
  })

  it("rejects a non-positive or non-integer event_version", () => {
    for (const eventVersion of [0, -1, 1.5]) {
      const raw = JSON.parse(durableFrame({ type: "loop_started" }))
      raw.event_version = eventVersion
      expect(parseAgentStreamFrame(JSON.stringify(raw))).toBeUndefined()
    }
  })

  it("rejects a durable frame without complete Turn identity", () => {
    const missingSession = JSON.parse(durableFrame({ type: "loop_started" }))
    delete missingSession.session_id
    const missingTurn = JSON.parse(durableFrame({ type: "loop_started" }))
    missingTurn.turn_id = null

    expect(
      parseAgentStreamFrame(JSON.stringify(missingSession))
    ).toBeUndefined()
    expect(parseAgentStreamFrame(JSON.stringify(missingTurn))).toBeUndefined()
  })
})

describe("parseAgentStreamFrame telemetry frames", () => {
  it("accepts the llm_started unit variant without any data key", () => {
    const frame = parseAgentStreamFrame(telemetryFrame({ type: "llm_started" }))

    expect(frame).toMatchObject({
      kind: "telemetry",
      llm_call_id: "call-1",
      telemetry_seq: 0,
      durable_before_event_seq: "41",
      event: { type: "llm_started" },
    })
  })

  it("rejects an llm_started frame that wrongly carries data", () => {
    expect(
      parseAgentStreamFrame(telemetryFrame({ type: "llm_started", data: {} }))
    ).toBeUndefined()
  })

  it("accepts text_delta with data and rejects it without data", () => {
    expect(
      parseAgentStreamFrame(
        telemetryFrame({ type: "text_delta", data: { delta: "he" } }, 1)
      )
    ).toMatchObject({
      kind: "telemetry",
      telemetry_seq: 1,
      event: { type: "text_delta", data: { delta: "he" } },
    })
    expect(
      parseAgentStreamFrame(telemetryFrame({ type: "text_delta" }, 1))
    ).toBeUndefined()
  })

  it("validates every typed telemetry data variant", () => {
    const events = [
      { type: "text_delta", data: { delta: "text" } },
      { type: "reasoning_delta", data: { delta: "reasoning" } },
      {
        type: "tool_call_delta",
        data: {
          call_id: "call-1",
          name: "lookup",
          arguments_delta: "{",
        },
      },
      {
        type: "llm_finished",
        data: { finish_reason: "stop", usage: USAGE },
      },
    ]

    for (const event of events)
      expect(parseAgentStreamFrame(telemetryFrame(event))).toBeDefined()
  })

  it("rejects malformed fields in every telemetry data variant", () => {
    const malformedEvents = [
      { type: "text_delta", data: { delta: 1 } },
      { type: "reasoning_delta", data: {} },
      {
        type: "tool_call_delta",
        data: { call_id: "call-1", name: 3, arguments_delta: "{" },
      },
      {
        type: "llm_finished",
        data: {
          finish_reason: "stop",
          usage: { input_tokens: 1, output_tokens: 2 },
        },
      },
    ]

    for (const event of malformedEvents)
      expect(parseAgentStreamFrame(telemetryFrame(event))).toBeUndefined()
  })

  it("tolerates telemetry_seq as a decimal string", () => {
    const frame = parseAgentStreamFrame(
      telemetryFrame({ type: "llm_started" }, "7")
    )
    expect(frame).toMatchObject({ kind: "telemetry", telemetry_seq: 7 })
  })

  it("requires a decimal durable-before watermark", () => {
    const missing = JSON.parse(telemetryFrame({ type: "llm_started" }))
    delete missing.durable_before_event_seq
    const malformed = JSON.parse(telemetryFrame({ type: "llm_started" }))
    malformed.durable_before_event_seq = -1

    expect(parseAgentStreamFrame(JSON.stringify(missing))).toBeUndefined()
    expect(parseAgentStreamFrame(JSON.stringify(malformed))).toBeUndefined()
  })

  it("rejects unknown telemetry event variants", () => {
    expect(
      parseAgentStreamFrame(telemetryFrame({ type: "llm_exploded", data: {} }))
    ).toBeUndefined()
  })

  it("rejects telemetry without complete Turn identity", () => {
    const missingTurn = JSON.parse(telemetryFrame({ type: "llm_started" }))
    delete missingTurn.turn_id

    expect(parseAgentStreamFrame(JSON.stringify(missingTurn))).toBeUndefined()
  })
})

describe("parseAgentStreamFrame envelope validation", () => {
  it("rejects unknown protocol_version", () => {
    const raw = JSON.parse(durableFrame({ type: "loop_started" }))
    raw.protocol_version = 2
    expect(parseAgentStreamFrame(JSON.stringify(raw))).toBeUndefined()
  })

  it("rejects unknown frame kinds", () => {
    const raw = JSON.parse(durableFrame({ type: "loop_started" }))
    raw.kind = "snapshot"
    expect(parseAgentStreamFrame(JSON.stringify(raw))).toBeUndefined()
  })

  it("rejects non-JSON payloads", () => {
    expect(parseAgentStreamFrame("not json")).toBeUndefined()
  })

  it("accepts stream_ready and stream_reset control frames", () => {
    expect(
      parseAgentStreamFrame(
        JSON.stringify({
          ...IDENTITY,
          kind: "control",
          event: { type: "stream_ready" },
        })
      )
    ).toMatchObject({ kind: "control", event: { type: "stream_ready" } })
    expect(
      parseAgentStreamFrame(
        JSON.stringify({
          ...IDENTITY,
          kind: "control",
          event: { type: "stream_reset", reason: "buffer_overflow" },
        })
      )
    ).toMatchObject({
      kind: "control",
      event: { type: "stream_reset", reason: "buffer_overflow" },
    })
  })

  it("accepts both-or-neither control identity and rejects invalid identity", () => {
    const idleReady = {
      protocol_version: 1,
      agent_id: "agent-1",
      created_at: IDENTITY.created_at,
      kind: "control",
      event: { type: "stream_ready" },
    }
    expect(parseAgentStreamFrame(JSON.stringify(idleReady))).toMatchObject({
      kind: "control",
      session_id: null,
      turn_id: null,
    })

    expect(
      parseAgentStreamFrame(
        JSON.stringify({ ...idleReady, session_id: "session-1" })
      )
    ).toBeUndefined()
    expect(
      parseAgentStreamFrame(
        JSON.stringify({ ...idleReady, session_id: 7, turn_id: {} })
      )
    ).toBeUndefined()
  })
})

describe("readSseStream resource bounds", () => {
  it("rejects an unterminated frame instead of buffering it without bound", async () => {
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
