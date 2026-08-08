import { describe, expect, it } from "vitest"

import { parseAgentStreamFrame } from "@/lib/stratum/event-stream"

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
      parseAgentStreamFrame(
        durableFrame({ type: "loop_started", data: {} })
      )
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
      usage: { input_tokens: 1, output_tokens: 2, total_tokens: 3 },
    }
    const frame = parseAgentStreamFrame(
      durableFrame({ type: "loop_finished", data })
    )

    expect(frame).toMatchObject({
      kind: "durable",
      event: { type: "loop_finished", data },
    })
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
})

describe("parseAgentStreamFrame telemetry frames", () => {
  it("accepts the llm_started unit variant without any data key", () => {
    const frame = parseAgentStreamFrame(telemetryFrame({ type: "llm_started" }))

    expect(frame).toMatchObject({
      kind: "telemetry",
      llm_call_id: "call-1",
      telemetry_seq: 0,
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

  it("tolerates telemetry_seq as a decimal string", () => {
    const frame = parseAgentStreamFrame(
      telemetryFrame({ type: "llm_started" }, "7")
    )
    expect(frame).toMatchObject({ kind: "telemetry", telemetry_seq: 7 })
  })

  it("rejects unknown telemetry event variants", () => {
    expect(
      parseAgentStreamFrame(telemetryFrame({ type: "llm_exploded", data: {} }))
    ).toBeUndefined()
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
        JSON.stringify({ ...IDENTITY, kind: "control", event: { type: "stream_ready" } })
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
})
