import {
  ApiError,
  apiErrorFromResponse,
  isEventSeq,
  type AgentRuntimeStreamFrameV1,
} from "@/lib/stratum/api"
import {
  parseAgentRuntimeProductEvent,
  parseLlmTelemetryEvent,
} from "@/lib/stratum/protocol-codec"

export type SseEvent = { id: string | null; event: string; data: string }

/** One broker frame is bounded; the browser parser has the same hard guard. */
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
    try {
      await reader.cancel()
    } catch {
      // The AbortSignal may already have cancelled the stream.
    }
    throw error
  } finally {
    reader.releaseLock()
  }
}

type JsonRecord = Record<string, unknown>

const isRecord = (value: unknown): value is JsonRecord =>
  typeof value === "object" && value !== null && !Array.isArray(value)

const hasOwn = (value: JsonRecord, key: string): boolean =>
  Object.prototype.hasOwnProperty.call(value, key)

function hasExactKeys(
  value: JsonRecord,
  required: readonly string[],
  optional: readonly string[] = []
): boolean {
  if (!required.every((key) => hasOwn(value, key))) return false
  const allowed = new Set([...required, ...optional])
  return Object.keys(value).every((key) => allowed.has(key))
}

const isNonEmptyString = (value: unknown): value is string =>
  typeof value === "string" && value.length > 0

const isPositiveSafeInteger = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value > 0

/**
 * Strictly decode one `AgentRuntimeStreamFrameV1`. Unknown versions, kinds,
 * variants, missing dual identity, non-decimal sequence fields, and extra
 * v1 envelope fields fail closed.
 */
export function parseAgentRuntimeStreamFrame(
  data: string
): AgentRuntimeStreamFrameV1 | undefined {
  let value: unknown
  try {
    value = JSON.parse(data)
  } catch {
    return undefined
  }
  if (
    !isRecord(value) ||
    value.protocol_version !== 1 ||
    !isNonEmptyString(value.agent_runtime_id) ||
    !isNonEmptyString(value.agent_id) ||
    !isNonEmptyString(value.created_at) ||
    !isRecord(value.event)
  )
    return undefined

  const base = {
    protocol_version: 1 as const,
    agent_runtime_id: value.agent_runtime_id,
    agent_id: value.agent_id,
    created_at: value.created_at,
  }

  if (value.kind === "control") {
    if (
      !hasExactKeys(
        value,
        [
          "protocol_version",
          "kind",
          "agent_runtime_id",
          "agent_id",
          "created_at",
          "event",
        ],
        ["session_id", "turn_id"]
      ) ||
      hasOwn(value, "session_id") !== hasOwn(value, "turn_id")
    )
      return undefined
    const sessionId = value.session_id ?? null
    const turnId = value.turn_id ?? null
    if (
      (sessionId !== null && !isNonEmptyString(sessionId)) ||
      (turnId !== null && !isNonEmptyString(turnId)) ||
      (sessionId === null) !== (turnId === null)
    )
      return undefined

    if (
      hasExactKeys(value.event, ["type"]) &&
      value.event.type === "stream_ready"
    )
      return {
        ...base,
        kind: "control",
        session_id: sessionId,
        turn_id: turnId,
        event: { type: "stream_ready" },
      }
    if (
      hasExactKeys(value.event, ["type", "reason"]) &&
      value.event.type === "stream_reset" &&
      value.event.reason === "buffer_overflow" &&
      sessionId === null &&
      turnId === null
    )
      return {
        ...base,
        kind: "control",
        session_id: null,
        turn_id: null,
        event: { type: "stream_reset", reason: "buffer_overflow" },
      }
    return undefined
  }

  if (value.kind === "durable") {
    const event = parseAgentRuntimeProductEvent(value.event)
    if (
      !hasExactKeys(value, [
        "protocol_version",
        "kind",
        "agent_runtime_id",
        "agent_id",
        "session_id",
        "turn_id",
        "created_at",
        "event_seq",
        "event_version",
        "event",
      ]) ||
      !isNonEmptyString(value.session_id) ||
      !isNonEmptyString(value.turn_id) ||
      !isEventSeq(value.event_seq) ||
      value.event_seq === "0" ||
      !isPositiveSafeInteger(value.event_version) ||
      event === undefined
    )
      return undefined
    return {
      ...base,
      kind: "durable",
      session_id: value.session_id,
      turn_id: value.turn_id,
      event_seq: value.event_seq,
      event_version: value.event_version,
      event,
    }
  }

  if (value.kind === "telemetry") {
    const event = parseLlmTelemetryEvent(value.event)
    if (
      !hasExactKeys(value, [
        "protocol_version",
        "kind",
        "agent_runtime_id",
        "agent_id",
        "session_id",
        "turn_id",
        "created_at",
        "durable_before_event_seq",
        "llm_call_id",
        "telemetry_seq",
        "event",
      ]) ||
      !isNonEmptyString(value.session_id) ||
      !isNonEmptyString(value.turn_id) ||
      !isEventSeq(value.durable_before_event_seq) ||
      !isNonEmptyString(value.llm_call_id) ||
      !isEventSeq(value.telemetry_seq) ||
      event === undefined
    )
      return undefined
    return {
      ...base,
      kind: "telemetry",
      session_id: value.session_id,
      turn_id: value.turn_id,
      durable_before_event_seq: value.durable_before_event_seq,
      llm_call_id: value.llm_call_id,
      telemetry_seq: value.telemetry_seq,
      event,
    }
  }

  return undefined
}

/** Subscribe to one exact AgentRuntime short tail. */
export function subscribeToAgentRuntimeEvents(options: {
  baseUrl: string
  agentRuntimeId: string
  afterCursor?: string
  signal?: AbortSignal
  fetcher?: typeof fetch
  onFrame(frame: AgentRuntimeStreamFrameV1, cursor: string | null): void
}): { done: Promise<void> } {
  const search = options.afterCursor
    ? new URLSearchParams({ after_cursor: options.afterCursor })
    : ""
  const base = `${options.baseUrl.replace(/\/$/, "")}/v1/agent-runtimes/${encodeURIComponent(options.agentRuntimeId)}/events`
  const url = search === "" ? base : `${base}?${search}`
  const fetcher = options.fetcher ?? fetch

  return {
    done: (async () => {
      const response = await fetcher(url, {
        headers: { Accept: "text/event-stream" },
        signal: options.signal,
      })
      if (response.status === 410)
        throw new ApiError("cursor_expired", 410, "event cursor expired")
      if (!response.ok) throw await apiErrorFromResponse(response)
      if (!response.body)
        throw new ApiError("invalid_stream", 500, "event stream has no body")

      await readSseStream(response.body, (sseEvent) => {
        const frame = parseAgentRuntimeStreamFrame(sseEvent.data)
        if (frame === undefined)
          throw new ApiError(
            "unsupported_frame",
            400,
            "received an unsupported stream frame"
          )
        if (sseEvent.id === "")
          throw new ApiError(
            "unsupported_frame",
            400,
            "stream cursors must not be empty"
          )
        if (
          frame.kind === "control" &&
          frame.event.type === "stream_reset" &&
          sseEvent.id !== null
        )
          throw new ApiError(
            "unsupported_frame",
            400,
            "stream reset must not carry a cursor"
          )
        options.onFrame(frame, sseEvent.id)
      })
    })(),
  }
}
