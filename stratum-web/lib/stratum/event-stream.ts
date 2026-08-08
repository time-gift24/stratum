import {
  ApiError,
  apiErrorFromResponse,
  isEventSeq,
  type AgentStreamFrameV1,
} from "@/lib/stratum/api"

export type SseEvent = { id: string | null; event: string; data: string }

export async function readSseStream(
  stream: ReadableStream<Uint8Array>,
  onEvent: (event: SseEvent) => void
): Promise<void> {
  const decoder = new TextDecoder()
  const reader = stream.getReader()
  let buffer = ""
  let id: string | null = null
  let event = "message"
  let data: string[] = []

  const dispatch = () => {
    if (data.length > 0) onEvent({ id, event, data: data.join("\n") })
    id = null
    event = "message"
    data = []
  }

  const readLine = (line: string) => {
    if (line === "") {
      dispatch()
      return
    }
    if (line.startsWith(":")) return

    const separator = line.indexOf(":")
    const field = separator === -1 ? line : line.slice(0, separator)
    const value =
      separator === -1 ? "" : line.slice(separator + 1).replace(/^ /, "")

    if (field === "data") data.push(value)
    else if (field === "event") event = value || "message"
    else if (field === "id" && !value.includes("\0")) id = value
  }

  const consumeLines = (final = false) => {
    let start = 0
    for (let index = 0; index < buffer.length; index += 1) {
      if (buffer[index] === "\r") {
        if (index + 1 === buffer.length && !final) break
        readLine(buffer.slice(start, index))
        if (buffer[index + 1] === "\n") index += 1
        start = index + 1
      } else if (buffer[index] === "\n") {
        readLine(buffer.slice(start, index))
        start = index + 1
      }
    }
    buffer = buffer.slice(start)
  }

  try {
    while (true) {
      const { done, value } = await reader.read()
      if (done) break
      buffer += decoder.decode(value, { stream: true })
      consumeLines()
    }
    buffer += decoder.decode()
    consumeLines(true)
  } finally {
    reader.releaseLock()
  }
}

const PRODUCT_EVENT_TYPES = new Set([
  "loop_started",
  "message_appended",
  "tool_approval_requested",
  "tool_approval_resolved",
  "transcript_compacted",
  "iteration_completed",
  "loop_finished",
  "loop_failed",
  "loop_cancelled",
])

const TELEMETRY_EVENT_TYPES = new Set([
  "llm_started",
  "text_delta",
  "reasoning_delta",
  "tool_call_delta",
  "llm_finished",
])

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value)

const isNullableString = (value: unknown): value is string | null =>
  value === null || typeof value === "string"

/**
 * 解析并校验一条 `AgentStreamFrameV1`。未知 protocol_version、未知 kind 或
 * 未知 event variant 一律拒绝（返回 undefined），不按 v1 猜测。
 */
export function parseAgentStreamFrame(data: string): AgentStreamFrameV1 | undefined {
  let value: unknown
  try {
    value = JSON.parse(data)
  } catch {
    return undefined
  }
  if (!isRecord(value)) return undefined
  if (value.protocol_version !== 1) return undefined
  if (typeof value.agent_id !== "string") return undefined
  if (typeof value.created_at !== "string") return undefined
  if (!isNullableString(value.session_id ?? null)) return undefined
  if (!isNullableString(value.turn_id ?? null)) return undefined
  if (!isRecord(value.event) || typeof value.event.type !== "string")
    return undefined

  const identity = {
    protocol_version: 1 as const,
    agent_id: value.agent_id,
    session_id: (value.session_id ?? null) as string | null,
    turn_id: (value.turn_id ?? null) as string | null,
    created_at: value.created_at,
  }
  const event = value.event
  const eventType = event.type as string

  switch (value.kind) {
    case "control": {
      if (eventType === "stream_ready")
        return { ...identity, kind: "control", event: { type: "stream_ready" } }
      if (eventType === "stream_reset" && event.reason === "buffer_overflow")
        return {
          ...identity,
          kind: "control",
          event: { type: "stream_reset", reason: "buffer_overflow" },
        }
      return undefined
    }
    case "durable": {
      if (
        !isEventSeq(value.event_seq) ||
        typeof value.event_version !== "number" ||
        !PRODUCT_EVENT_TYPES.has(eventType) ||
        !isRecord(event.data)
      )
        return undefined
      return {
        ...identity,
        kind: "durable",
        event_seq: value.event_seq,
        event_version: value.event_version,
        event: event as unknown as Extract<
          AgentStreamFrameV1,
          { kind: "durable" }
        >["event"],
      }
    }
    case "telemetry": {
      if (
        typeof value.llm_call_id !== "string" ||
        !TELEMETRY_EVENT_TYPES.has(eventType) ||
        !isRecord(event.data)
      )
        return undefined
      // telemetry_seq 是 call-local 序号；容忍十进制字符串或 number
      const seq =
        typeof value.telemetry_seq === "number"
          ? value.telemetry_seq
          : isEventSeq(value.telemetry_seq)
            ? Number(value.telemetry_seq)
            : Number.NaN
      if (!Number.isSafeInteger(seq) || seq < 0) return undefined
      return {
        ...identity,
        kind: "telemetry",
        llm_call_id: value.llm_call_id,
        telemetry_seq: seq,
        event: event as unknown as Extract<
          AgentStreamFrameV1,
          { kind: "telemetry" }
        >["event"],
      }
    }
    default:
      return undefined
  }
}

/**
 * 订阅 Agent SSE tail。无 cursor 时从当前新 tail 开始；页面内恢复通过
 * `after_cursor` query param（单一 cursor 来源，避免 Last-Event-ID 二义性）。
 * cursor 是不透明 NATS position，只做 transport 寻址。
 */
export function subscribeToAgentEvents(options: {
  baseUrl: string
  agentId: string
  afterCursor?: string
  signal?: AbortSignal
  fetcher?: typeof fetch
  onFrame(frame: AgentStreamFrameV1, cursor: string | null): void
}): { done: Promise<void> } {
  const search = options.afterCursor
    ? new URLSearchParams({ after_cursor: options.afterCursor })
    : ""
  const base = `${options.baseUrl.replace(/\/$/, "")}/v1/agents/${options.agentId}/events`
  const url = search === "" ? base : `${base}?${search}`
  const fetcher = options.fetcher ?? fetch

  return {
    done: (async () => {
      const response = await fetcher(url, {
        headers: { Accept: "text/event-stream" },
        signal: options.signal,
      })
      if (response.status === 410) {
        throw new ApiError("cursor_expired", 410, "event cursor expired")
      }
      if (!response.ok) throw await apiErrorFromResponse(response)
      if (!response.body) {
        throw new ApiError("invalid_stream", 500, "event stream has no body")
      }
      await readSseStream(response.body, (event) => {
        const frame = parseAgentStreamFrame(event.data)
        if (frame === undefined) {
          throw new ApiError(
            "unsupported_frame",
            400,
            "received an unsupported stream frame"
          )
        }
        options.onFrame(frame, event.id)
      })
    })(),
  }
}
