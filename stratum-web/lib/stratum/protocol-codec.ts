import type {
  AgentRuntimeCreated,
  AgentRuntimeDurableRecordV1,
  AgentRuntimeHistoryPage,
  AgentRuntimeProductEventV1,
  AgentRuntimeTurnAccepted,
  AgentRuntimeView,
  ChatMessage,
  LlmTelemetryEventV1,
  PendingApprovalView,
  TokenUsage,
  ToolCall,
} from "@/lib/stratum/api"
import type {
  AgentTemplateView,
  ModelConfig,
  ModelDescriptor,
} from "@/lib/stratum/model-config"

type JsonRecord = Record<string, unknown>

const EVENT_SEQ_PATTERN = /^(0|[1-9]\d*)$/

const isEventSeq = (value: unknown): value is string =>
  typeof value === "string" && EVENT_SEQ_PATTERN.test(value)

const compareEventSeq = (left: string, right: string): number => {
  const a = BigInt(left)
  const b = BigInt(right)
  return a < b ? -1 : a > b ? 1 : 0
}

const isRecord = (value: unknown): value is JsonRecord =>
  typeof value === "object" && value !== null && !Array.isArray(value)

const isString = (value: unknown): value is string => typeof value === "string"

const isNonEmptyString = (value: unknown): value is string =>
  typeof value === "string" && value.length > 0

const isNullableString = (value: unknown): value is string | null =>
  value === null || isNonEmptyString(value)

const isNonNegativeSafeInteger = (value: unknown): value is number =>
  typeof value === "number" && Number.isSafeInteger(value) && value >= 0

const isPositiveSafeInteger = (value: unknown): value is number =>
  isNonNegativeSafeInteger(value) && value > 0

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

function isTokenUsage(value: unknown): value is TokenUsage {
  return (
    isRecord(value) &&
    hasExactKeys(value, ["input_tokens", "output_tokens", "total_tokens"]) &&
    isNonNegativeSafeInteger(value.input_tokens) &&
    isNonNegativeSafeInteger(value.output_tokens) &&
    isNonNegativeSafeInteger(value.total_tokens)
  )
}

function isToolCall(value: unknown): value is ToolCall {
  return (
    isRecord(value) &&
    hasExactKeys(value, ["call_id", "name", "arguments"]) &&
    isNonEmptyString(value.call_id) &&
    isNonEmptyString(value.name)
  )
}

function isChatMessage(value: unknown): value is ChatMessage {
  if (
    !isRecord(value) ||
    !hasExactKeys(
      value,
      ["role", "content"],
      ["tool_calls", "reasoning_content", "tool_call_id"]
    ) ||
    !isRecord(value.content) ||
    !hasExactKeys(value.content, ["type", "data"])
  )
    return false

  const roleValid =
    value.role === "user" ||
    value.role === "assistant" ||
    value.role === "tool" ||
    value.role === "system"
  const contentValid =
    (value.content.type === "text" && isString(value.content.data)) ||
    value.content.type === "json"
  return (
    roleValid &&
    contentValid &&
    (value.tool_calls === undefined ||
      (Array.isArray(value.tool_calls) &&
        value.tool_calls.every(isToolCall))) &&
    (value.reasoning_content === undefined ||
      isString(value.reasoning_content)) &&
    (value.tool_call_id === undefined || isNonEmptyString(value.tool_call_id))
  )
}

function isApprovalDetails(
  value: unknown
): value is Omit<PendingApprovalView, "requested_event_seq"> {
  return (
    isRecord(value) &&
    hasExactKeys(value, [
      "approval_id",
      "call_id",
      "tool_name",
      "arguments",
      "tool_kind",
      "danger_level",
    ]) &&
    isNonEmptyString(value.approval_id) &&
    isNonEmptyString(value.call_id) &&
    isNonEmptyString(value.tool_name) &&
    (value.tool_kind === "read" || value.tool_kind === "write") &&
    (value.danger_level === "low" ||
      value.danger_level === "medium" ||
      value.danger_level === "high")
  )
}

function isPendingApproval(value: unknown): value is PendingApprovalView {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "requested_event_seq",
      "approval_id",
      "call_id",
      "tool_name",
      "arguments",
      "tool_kind",
      "danger_level",
    ]) ||
    !isEventSeq(value.requested_event_seq) ||
    value.requested_event_seq === "0"
  )
    return false
  return isApprovalDetails({
    approval_id: value.approval_id,
    call_id: value.call_id,
    tool_name: value.tool_name,
    arguments: value.arguments,
    tool_kind: value.tool_kind,
    danger_level: value.danger_level,
  })
}

export function parseAgentRuntimeProductEvent(
  value: unknown
): AgentRuntimeProductEventV1 | undefined {
  if (!isRecord(value) || !isNonEmptyString(value.type)) return undefined
  if (value.type === "loop_started")
    return hasExactKeys(value, ["type"])
      ? (value as AgentRuntimeProductEventV1)
      : undefined
  if (!hasExactKeys(value, ["type", "data"]) || !isRecord(value.data))
    return undefined

  const data = value.data
  let valid = false
  switch (value.type) {
    case "message_appended":
      valid = hasExactKeys(data, ["message"]) && isChatMessage(data.message)
      break
    case "tool_approval_requested":
      valid = isApprovalDetails(data)
      break
    case "tool_approval_resolved":
      valid =
        hasExactKeys(data, ["approval_id", "decision"]) &&
        isNonEmptyString(data.approval_id) &&
        (data.decision === "approve" || data.decision === "reject")
      break
    case "transcript_compacted":
      valid =
        hasExactKeys(data, ["summary", "compacted_iteration"]) &&
        isChatMessage(data.summary) &&
        data.summary.role === "system" &&
        isNonNegativeSafeInteger(data.compacted_iteration)
      break
    case "iteration_completed":
      valid =
        hasExactKeys(data, ["iteration", "usage"]) &&
        isNonNegativeSafeInteger(data.iteration) &&
        isTokenUsage(data.usage)
      break
    case "loop_finished":
      valid =
        hasExactKeys(data, ["finish_reason", "usage"]) &&
        isString(data.finish_reason) &&
        isTokenUsage(data.usage)
      break
    case "loop_failed":
      valid =
        hasExactKeys(data, ["error_text", "usage"]) &&
        isString(data.error_text) &&
        isTokenUsage(data.usage)
      break
    case "loop_cancelled":
      valid = hasExactKeys(data, ["usage"]) && isTokenUsage(data.usage)
      break
    default:
      return undefined
  }
  return valid ? (value as AgentRuntimeProductEventV1) : undefined
}

export function parseLlmTelemetryEvent(
  value: unknown
): LlmTelemetryEventV1 | undefined {
  if (!isRecord(value) || !isNonEmptyString(value.type)) return undefined
  if (value.type === "llm_started")
    return hasExactKeys(value, ["type"])
      ? (value as LlmTelemetryEventV1)
      : undefined
  if (!hasExactKeys(value, ["type", "data"]) || !isRecord(value.data))
    return undefined

  const data = value.data
  let valid = false
  switch (value.type) {
    case "text_delta":
    case "reasoning_delta":
      valid = hasExactKeys(data, ["delta"]) && isString(data.delta)
      break
    case "tool_call_delta":
      valid =
        hasExactKeys(data, ["call_id", "arguments_delta"], ["name"]) &&
        isNonEmptyString(data.call_id) &&
        isString(data.arguments_delta) &&
        (data.name === undefined || data.name === null || isString(data.name))
      break
    case "llm_finished":
      valid =
        hasExactKeys(data, ["finish_reason"], ["usage"]) &&
        isString(data.finish_reason) &&
        (data.usage === undefined ||
          data.usage === null ||
          isTokenUsage(data.usage))
      break
    default:
      return undefined
  }
  return valid ? (value as LlmTelemetryEventV1) : undefined
}

function isModelConfig(value: unknown): value is ModelConfig {
  return (
    isRecord(value) &&
    hasExactKeys(value, ["model", "parameters"]) &&
    isNonEmptyString(value.model) &&
    isRecord(value.parameters)
  )
}

function isAgentTemplateVersion(value: unknown): value is string {
  return (
    isNonEmptyString(value) &&
    new TextEncoder().encode(value).length <= 128 &&
    value.trim() === value &&
    !/\p{Cc}/u.test(value)
  )
}

function isAgentTemplate(value: unknown): value is AgentTemplateView {
  return (
    isRecord(value) &&
    hasExactKeys(value, ["agent_name", "version", "model_config"]) &&
    isNonEmptyString(value.agent_name) &&
    isAgentTemplateVersion(value.version) &&
    isModelConfig(value.model_config)
  )
}

function isModelDescriptor(value: unknown): value is ModelDescriptor {
  return (
    isRecord(value) &&
    hasExactKeys(value, ["model", "parameters_schema"]) &&
    isNonEmptyString(value.model)
  )
}

export function parseAgentTemplatesResponse(
  value: unknown
): { templates: readonly AgentTemplateView[] } | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, ["templates"]) ||
    !Array.isArray(value.templates) ||
    !value.templates.every(isAgentTemplate)
  )
    return undefined
  return { templates: value.templates }
}

export function parseModelsResponse(
  value: unknown
): { models: readonly ModelDescriptor[] } | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, ["models"]) ||
    !Array.isArray(value.models) ||
    !value.models.every(isModelDescriptor)
  )
    return undefined
  return { models: value.models }
}

export function parseAgentRuntimeCreated(
  value: unknown
): AgentRuntimeCreated | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "agent_runtime_id",
      "agent_id",
      "agent_name",
      "agent_version",
      "created_at",
    ]) ||
    !isNonEmptyString(value.agent_runtime_id) ||
    !isNonEmptyString(value.agent_id) ||
    !isNonEmptyString(value.agent_name) ||
    !isAgentTemplateVersion(value.agent_version) ||
    !isNonEmptyString(value.created_at)
  )
    return undefined
  return value as AgentRuntimeCreated
}

export function parseAgentRuntimeTurnAccepted(
  value: unknown
): AgentRuntimeTurnAccepted | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "agent_runtime_id",
      "agent_id",
      "session_id",
      "turn_id",
    ]) ||
    !isNonEmptyString(value.agent_runtime_id) ||
    !isNonEmptyString(value.agent_id) ||
    !isNonEmptyString(value.session_id) ||
    !isNonEmptyString(value.turn_id)
  )
    return undefined
  return value as AgentRuntimeTurnAccepted
}

export function parseAgentRuntimeView(
  value: unknown
): AgentRuntimeView | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "agent_runtime_id",
      "agent_id",
      "agent_name",
      "agent_version",
      "status",
      "model_config",
      "session_id",
      "current_turn_id",
      "snapshot_event_seq",
      "telemetry_floor_event_seq",
      "pending_approvals",
      "latest_usage",
      "resume_required",
    ]) ||
    !isNonEmptyString(value.agent_runtime_id) ||
    !isNonEmptyString(value.agent_id) ||
    !isNonEmptyString(value.agent_name) ||
    !isAgentTemplateVersion(value.agent_version) ||
    (value.status !== "idle" &&
      value.status !== "running" &&
      value.status !== "finished" &&
      value.status !== "failed" &&
      value.status !== "cancelled") ||
    !isModelConfig(value.model_config) ||
    !isNullableString(value.session_id) ||
    !isNullableString(value.current_turn_id) ||
    (value.session_id === null) !== (value.current_turn_id === null) ||
    (value.status === "idle" && value.session_id !== null) ||
    (value.status !== "idle" && value.session_id === null) ||
    !isEventSeq(value.snapshot_event_seq) ||
    !isEventSeq(value.telemetry_floor_event_seq) ||
    compareEventSeq(value.telemetry_floor_event_seq, value.snapshot_event_seq) >
      0 ||
    !Array.isArray(value.pending_approvals) ||
    !value.pending_approvals.every(isPendingApproval) ||
    (value.latest_usage !== null && !isTokenUsage(value.latest_usage)) ||
    typeof value.resume_required !== "boolean"
  )
    return undefined

  let previous = "0"
  for (const approval of value.pending_approvals) {
    if (
      compareEventSeq(approval.requested_event_seq, previous) <= 0 ||
      compareEventSeq(approval.requested_event_seq, value.snapshot_event_seq) >
        0
    )
      return undefined
    previous = approval.requested_event_seq
  }
  return value as AgentRuntimeView
}

function parseHistoryItem(
  value: unknown
): AgentRuntimeDurableRecordV1 | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "event_seq",
      "event_version",
      "session_id",
      "turn_id",
      "created_at",
      "event",
    ]) ||
    !isEventSeq(value.event_seq) ||
    value.event_seq === "0" ||
    !isPositiveSafeInteger(value.event_version) ||
    !isNonEmptyString(value.session_id) ||
    !isNonEmptyString(value.turn_id) ||
    !isNonEmptyString(value.created_at) ||
    parseAgentRuntimeProductEvent(value.event) === undefined
  )
    return undefined
  return value as AgentRuntimeDurableRecordV1
}

export function parseAgentRuntimeHistoryPage(
  value: unknown
): AgentRuntimeHistoryPage | undefined {
  if (
    !isRecord(value) ||
    !hasExactKeys(value, [
      "items",
      "through_event_seq",
      "next_before_event_seq",
      "has_more",
    ]) ||
    !Array.isArray(value.items) ||
    !value.items.every((item) => parseHistoryItem(item) !== undefined) ||
    !isEventSeq(value.through_event_seq) ||
    !isNullableString(value.next_before_event_seq) ||
    (value.next_before_event_seq !== null &&
      !isEventSeq(value.next_before_event_seq)) ||
    typeof value.has_more !== "boolean"
  )
    return undefined

  let previous = "0"
  for (const item of value.items) {
    if (
      compareEventSeq(item.event_seq, previous) <= 0 ||
      compareEventSeq(item.event_seq, value.through_event_seq) > 0
    )
      return undefined
    previous = item.event_seq
  }
  const oldest = value.items[0]?.event_seq ?? null
  if (
    (value.has_more && value.next_before_event_seq === null) ||
    (oldest === null && value.next_before_event_seq !== null) ||
    (value.next_before_event_seq !== null &&
      value.next_before_event_seq !== oldest)
  )
    return undefined
  return value as AgentRuntimeHistoryPage
}
