import {
  ApiError,
  apiErrorFromResponse,
  isEventSeq,
  type AgentProductEventV1,
  type AgentStreamFrameV1,
  type ChatMessage,
  type LlmTelemetryEventV1,
  type TokenUsage,
  type ToolCall,
} from "@/lib/stratum/api"

export type SseEvent = { id: string | null; event: string; data: string }

/** One NATS frame is bounded by the broker; keep parser memory bounded too. */
const MAX_SSE_EVENT_CHARS = 2 * 1024 * 1024

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
  let dataChars = 0

  const dispatch = () => {
    if (data.length > 0) onEvent({ id, event, data: data.join("\n") })
    id = null
    event = "message"
    data = []
    dataChars = 0
  }

  const readLine = (line: string) => {
    if (line.length > MAX_SSE_EVENT_CHARS)
      throw new ApiError(
        "stream_frame_too_large",
        0,
        "event stream frame is too large"
      )
    if (line === "") {
      dispatch()
      return
    }
    if (line.startsWith(":")) return

    const separator = line.indexOf(":")
    const field = separator === -1 ? line : line.slice(0, separator)
    const value =
      separator === -1 ? "" : line.slice(separator + 1).replace(/^ /, "")

    if (field === "data") {
      dataChars += value.length + (data.length === 0 ? 0 : 1)
      if (dataChars > MAX_SSE_EVENT_CHARS)
        throw new ApiError(
          "stream_frame_too_large",
          0,
          "event stream frame is too large"
        )
      data.push(value)
    } else if (field === "event") event = value || "message"
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
      if (buffer.length > MAX_SSE_EVENT_CHARS)
        throw new ApiError(
          "stream_frame_too_large",
          0,
          "event stream frame is too large"
        )
    }
    buffer += decoder.decode()
    consumeLines(true)
  } catch (error) {
    // A parser or callback failure owns this response body: cancel it now so
    // the fetch connection cannot continue buffering after the caller exits.
    try {
      await reader.cancel()
    } catch {
      // The stream may already have been aborted by its AbortSignal.
    }
    throw error
  } finally {
    reader.releaseLock()
  }
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value)

const isNullableString = (value: unknown): value is string | null =>
  value === null || typeof value === "string"

const isNonNegativeInteger = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0

const hasOwn = (value: Record<string, unknown>, key: string): boolean =>
  Object.prototype.hasOwnProperty.call(value, key)

function isTokenUsage(value: unknown): value is TokenUsage {
  return (
    isRecord(value) &&
    isNonNegativeInteger(value.input_tokens) &&
    isNonNegativeInteger(value.output_tokens) &&
    isNonNegativeInteger(value.total_tokens)
  )
}

function isToolCall(value: unknown): value is ToolCall {
  return (
    isRecord(value) &&
    typeof value.call_id === "string" &&
    typeof value.name === "string" &&
    hasOwn(value, "arguments")
  )
}

function isChatMessage(value: unknown): value is ChatMessage {
  if (!isRecord(value) || !isRecord(value.content)) return false
  const validRole =
    value.role === "user" ||
    value.role === "assistant" ||
    value.role === "tool" ||
    value.role === "system"
  const validContent =
    (value.content.type === "text" && typeof value.content.data === "string") ||
    (value.content.type === "json" && hasOwn(value.content, "data"))
  return (
    validRole &&
    validContent &&
    (value.tool_calls === undefined ||
      (Array.isArray(value.tool_calls) &&
        value.tool_calls.every(isToolCall))) &&
    (value.reasoning_content === undefined ||
      typeof value.reasoning_content === "string") &&
    (value.tool_call_id === undefined || typeof value.tool_call_id === "string")
  )
}

function isProductEvent(event: unknown): event is AgentProductEventV1 {
  if (!isRecord(event) || typeof event.type !== "string") return false
  if (event.type === "loop_started") return event.data === undefined
  if (!isRecord(event.data)) return false
  const data = event.data

  switch (event.type) {
    case "message_appended":
      return isChatMessage(data.message)
    case "tool_approval_requested":
      return (
        typeof data.approval_id === "string" &&
        typeof data.call_id === "string" &&
        typeof data.tool_name === "string" &&
        hasOwn(data, "arguments") &&
        (data.tool_kind === "read" || data.tool_kind === "write") &&
        (data.danger_level === "low" ||
          data.danger_level === "medium" ||
          data.danger_level === "high")
      )
    case "tool_approval_resolved":
      return (
        typeof data.approval_id === "string" &&
        (data.decision === "approve" || data.decision === "reject")
      )
    case "transcript_compacted":
      return (
        isChatMessage(data.summary) &&
        isNonNegativeInteger(data.compacted_iteration)
      )
    case "iteration_completed":
      return isNonNegativeInteger(data.iteration) && isTokenUsage(data.usage)
    case "loop_finished":
      return typeof data.finish_reason === "string" && isTokenUsage(data.usage)
    case "loop_failed":
      return typeof data.error_text === "string" && isTokenUsage(data.usage)
    case "loop_cancelled":
      return isTokenUsage(data.usage)
    default:
      return false
  }
}

function isTelemetryEvent(event: unknown): event is LlmTelemetryEventV1 {
  if (!isRecord(event) || typeof event.type !== "string") return false
  if (event.type === "llm_started") return event.data === undefined
  if (!isRecord(event.data)) return false
  const data = event.data

  switch (event.type) {
    case "text_delta":
    case "reasoning_delta":
      return typeof data.delta === "string"
    case "tool_call_delta":
      return (
        typeof data.call_id === "string" &&
        typeof data.arguments_delta === "string" &&
        (data.name === undefined ||
          data.name === null ||
          typeof data.name === "string")
      )
    case "llm_finished":
      return (
        typeof data.finish_reason === "string" &&
        (data.usage === undefined ||
          data.usage === null ||
          isTokenUsage(data.usage))
      )
    default:
      return false
  }
}

/**
 * 解析并校验一条 `AgentStreamFrameV1`。未知 protocol_version、未知 kind 或
 * 未知 event variant 一律拒绝（返回 undefined），不按 v1 猜测。
 */
export function parseAgentStreamFrame(
  data: string
): AgentStreamFrameV1 | undefined {
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
  const sessionId = value.session_id === undefined ? null : value.session_id
  const turnId = value.turn_id === undefined ? null : value.turn_id
  if (!isNullableString(sessionId) || !isNullableString(turnId))
    return undefined
  if ((sessionId === null) !== (turnId === null)) return undefined
  if (!isRecord(value.event) || typeof value.event.type !== "string")
    return undefined

  const baseIdentity = {
    protocol_version: 1 as const,
    agent_id: value.agent_id,
    created_at: value.created_at,
  }
  const event = value.event

  switch (value.kind) {
    case "control": {
      if (event.type === "stream_ready" && event.data === undefined)
        return {
          ...baseIdentity,
          kind: "control",
          session_id: sessionId,
          turn_id: turnId,
          event: { type: "stream_ready" },
        }
      if (
        event.type === "stream_reset" &&
        event.data === undefined &&
        event.reason === "buffer_overflow"
      )
        return {
          ...baseIdentity,
          kind: "control",
          session_id: sessionId,
          turn_id: turnId,
          event: { type: "stream_reset", reason: "buffer_overflow" },
        }
      return undefined
    }
    case "durable": {
      if (
        typeof sessionId !== "string" ||
        typeof turnId !== "string" ||
        !isEventSeq(value.event_seq) ||
        typeof value.event_version !== "number" ||
        !Number.isSafeInteger(value.event_version) ||
        value.event_version <= 0 ||
        !isProductEvent(event)
      )
        return undefined
      return {
        ...baseIdentity,
        kind: "durable",
        session_id: sessionId,
        turn_id: turnId,
        event_seq: value.event_seq,
        event_version: value.event_version,
        event,
      }
    }
    case "telemetry": {
      if (
        typeof sessionId !== "string" ||
        typeof turnId !== "string" ||
        typeof value.llm_call_id !== "string" ||
        !isEventSeq(value.durable_before_event_seq) ||
        !isTelemetryEvent(event)
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
        ...baseIdentity,
        kind: "telemetry",
        session_id: sessionId,
        turn_id: turnId,
        llm_call_id: value.llm_call_id,
        telemetry_seq: seq,
        durable_before_event_seq: value.durable_before_event_seq,
        event,
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
