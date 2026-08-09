import type {
  AgentRuntimeDurableRecordV1,
  AgentRuntimeView,
  ChatMessage,
} from "@/lib/stratum/api"
import { compareEventSeq, incrementEventSeq } from "@/lib/stratum/api"
import type {
  ApprovalRequest,
  ConversationAction,
  ConversationState,
  DraftState,
  DurableFrame,
  StableMessage,
  TelemetryFrame,
  TimelineEntry,
  ToolProgress,
} from "@/features/agent-conversation/types"

/**
 * AgentRuntime event projection.
 *
 * Realtime product frames are immediately visible but remain in
 * `unconfirmedDurableFrames`; only a successful PG snapshot/reconcile moves
 * `pgConfirmedEventSeq`. Telemetry is fenced by both runtime identities, exact
 * Turn, call-local sequence, and the durable watermark.
 */

export const initialConversationState: ConversationState = {
  agentRuntimeId: null,
  agentId: null,
  view: null,
  pgConfirmedEventSeq: "0",
  unconfirmedDurableFrames: {},
  timeline: [],
  appliedEventSeqs: new Set<string>(),
  drafts: {},
  activeLlmCallId: null,
  closedLlmCallIds: new Set<string>(),
  telemetryFloorEventSeq: "0",
  tools: {},
  approvals: {},
  historyThrough: null,
  historyBefore: null,
  historyHasMore: false,
  historyLoading: false,
  realtimeDegraded: false,
  cancelRequested: false,
  acceptedTurnId: null,
  phase: "empty",
  error: null,
}

const TERMINAL_STATUSES = new Set(["finished", "failed", "cancelled"])

export function conversationReducer(
  state: ConversationState,
  action: ConversationAction
): ConversationState {
  switch (action.type) {
    case "runtime_selected":
      return action.agentRuntimeId === null
        ? initialConversationState
        : {
            ...initialConversationState,
            agentRuntimeId: action.agentRuntimeId,
            phase: "recovering",
          }
    case "recovery_started":
      return {
        ...initialConversationState,
        agentRuntimeId: action.agentRuntimeId,
        realtimeDegraded:
          state.agentRuntimeId === action.agentRuntimeId
            ? state.realtimeDegraded
            : false,
        acceptedTurnId:
          state.agentRuntimeId === action.agentRuntimeId
            ? state.acceptedTurnId
            : null,
        phase: "recovering",
      }
    case "snapshot_loaded":
      return projectSnapshot(state, action)
    case "history_page_started":
      return { ...state, historyLoading: true }
    case "history_page_failed":
      return { ...state, historyLoading: false }
    case "history_page_loaded": {
      let next = state
      for (const item of action.items)
        next = projectHistoryProduct(next, item, false)
      return reconcileTransientState({
        ...next,
        historyBefore: action.historyBefore,
        historyHasMore: action.historyHasMore,
        historyLoading: false,
      })
    }
    case "durable_frame":
      return projectDurableFrame(state, action.frame, false)
    case "telemetry_frame":
      return projectTelemetryFrame(state, action.frame)
    case "turn_accepted":
      if (
        state.agentRuntimeId !== action.agentRuntimeId ||
        (state.agentId !== null && state.agentId !== action.agentId)
      )
        return state
      return state.view?.current_turn_id === action.turnId
        ? { ...state, agentId: action.agentId }
        : {
            ...state,
            agentId: action.agentId,
            acceptedTurnId: action.turnId,
          }
    case "view_reconciled":
      return rebaseOnPgView(state, action)
    case "approval_resolved": {
      if (!(action.approvalId in state.approvals)) return state
      const approvals = { ...state.approvals }
      delete approvals[action.approvalId]
      return { ...state, approvals }
    }
    case "cancel_requested":
      return { ...state, cancelRequested: true }
    case "realtime_degraded":
      return { ...state, realtimeDegraded: action.degraded }
    case "recovery_ready":
      return { ...state, phase: "ready" }
    case "connection_error":
      return { ...state, phase: "connection_error", error: action.error }
    case "missing":
      return { ...state, phase: "missing", error: action.error }
  }
}

function projectSnapshot(
  state: ConversationState,
  action: Extract<ConversationAction, { type: "snapshot_loaded" }>
): ConversationState {
  if (
    state.agentRuntimeId !== null &&
    state.agentRuntimeId !== action.view.agent_runtime_id
  )
    return state
  if (state.agentId !== null && state.agentId !== action.view.agent_id)
    return state

  let acceptedTurnId = state.acceptedTurnId
  if (
    acceptedTurnId === action.view.current_turn_id ||
    action.items.some(
      (item) =>
        item.turn_id === acceptedTurnId &&
        (item.event.type === "loop_started" ||
          item.event.type === "loop_finished" ||
          item.event.type === "loop_failed" ||
          item.event.type === "loop_cancelled")
    )
  )
    acceptedTurnId = null

  let next: ConversationState = {
    ...initialConversationState,
    agentRuntimeId: action.view.agent_runtime_id,
    agentId: action.view.agent_id,
    view: action.view,
    pgConfirmedEventSeq: action.view.snapshot_event_seq,
    telemetryFloorEventSeq: action.view.telemetry_floor_event_seq,
    approvals: approvalsFromView(action.view),
    historyThrough: action.view.snapshot_event_seq,
    historyBefore: action.historyBefore,
    historyHasMore: action.historyHasMore,
    realtimeDegraded: state.realtimeDegraded,
    acceptedTurnId,
    phase: "recovering",
  }
  for (const item of action.items)
    next = projectHistoryProduct(next, item, true)
  return reconcileTransientState(next)
}

/**
 * Atomic PG rebase. The unconfirmed map is intentionally read from reducer
 * state at commit time, not captured when the request started.
 */
function rebaseOnPgView(
  state: ConversationState,
  action: Extract<ConversationAction, { type: "view_reconciled" }>
): ConversationState {
  if (
    state.agentRuntimeId !== action.view.agent_runtime_id ||
    state.agentId !== action.view.agent_id ||
    compareEventSeq(
      state.pgConfirmedEventSeq,
      action.basePgConfirmedEventSeq
    ) !== 0 ||
    compareEventSeq(action.view.snapshot_event_seq, state.pgConfirmedEventSeq) <
      0
  )
    return state

  let next = state
  for (const item of action.items)
    next = projectHistoryProduct(next, item, true)

  const through = action.view.snapshot_event_seq
  const futureFrames = Object.values(state.unconfirmedDurableFrames)
    .filter((frame) => compareEventSeq(frame.event_seq, through) > 0)
    .sort((left, right) => compareEventSeq(left.event_seq, right.event_seq))
  const unconfirmedDurableFrames = Object.fromEntries(
    futureFrames.map((frame) => [frame.event_seq, frame])
  )

  const acceptedTurnId =
    next.acceptedTurnId === action.view.current_turn_id
      ? null
      : next.acceptedTurnId

  next = {
    ...next,
    agentId: action.view.agent_id,
    view: action.view,
    pgConfirmedEventSeq: through,
    unconfirmedDurableFrames,
    telemetryFloorEventSeq: action.view.telemetry_floor_event_seq,
    approvals: approvalsFromView(action.view),
    acceptedTurnId,
    cancelRequested:
      TERMINAL_STATUSES.has(action.view.status) && acceptedTurnId === null
        ? false
        : next.cancelRequested,
  }

  // Force the `>T` control effects to replay over view@T. Timeline insertion
  // is event-sequence idempotent, so already-visible messages/markers do not
  // duplicate while status/approval/final-floor are deterministically rebased.
  if (futureFrames.length > 0) {
    const applied = new Set(next.appliedEventSeqs)
    for (const frame of futureFrames) applied.delete(frame.event_seq)
    next = { ...next, appliedEventSeqs: applied }
    for (const frame of futureFrames)
      next = projectDurableFrame(next, frame, true)
  }

  return reconcileTransientState(next)
}

function approvalsFromView(
  view: AgentRuntimeView
): Readonly<Record<string, ApprovalRequest>> {
  const approvals: Record<string, ApprovalRequest> = {}
  for (const pending of view.pending_approvals) {
    approvals[pending.approval_id] = {
      approvalId: pending.approval_id,
      callId: pending.call_id,
      toolName: pending.tool_name,
      arguments: pending.arguments,
      toolKind: pending.tool_kind,
      dangerLevel: pending.danger_level,
    }
  }
  return approvals
}

function entrySeq(entry: TimelineEntry): string {
  return entry.kind === "message"
    ? entry.message.eventSeq
    : entry.marker.eventSeq
}

function insertTimelineEntry(
  timeline: readonly TimelineEntry[],
  entry: TimelineEntry
): TimelineEntry[] {
  if (timeline.some((candidate) => entrySeq(candidate) === entrySeq(entry)))
    return [...timeline]
  const last = timeline.at(-1)
  if (
    last === undefined ||
    compareEventSeq(entrySeq(last), entrySeq(entry)) <= 0
  )
    return [...timeline, entry]
  return [...timeline, entry].sort((left, right) =>
    compareEventSeq(entrySeq(left), entrySeq(right))
  )
}

function withApplied(
  state: ConversationState,
  eventSeq: string
): ConversationState {
  const appliedEventSeqs = new Set(state.appliedEventSeqs)
  appliedEventSeqs.add(eventSeq)
  return { ...state, appliedEventSeqs }
}

/** Cold/pagination history decodes all products but only projects safe timeline. */
function projectHistoryProduct(
  state: ConversationState,
  item: AgentRuntimeDurableRecordV1,
  convergeDraft: boolean
): ConversationState {
  if (
    state.agentRuntimeId === null ||
    state.appliedEventSeqs.has(item.event_seq)
  )
    return state
  return projectVisibleProductEvent(state, item, convergeDraft)
}

function projectDurableFrame(
  state: ConversationState,
  frame: DurableFrame,
  forceReplay: boolean
): ConversationState {
  if (!matchesFrameIdentity(state, frame)) return state
  if (compareEventSeq(frame.event_seq, state.pgConfirmedEventSeq) <= 0)
    return state
  if (!forceReplay && frame.event_seq in state.unconfirmedDurableFrames)
    return state

  let next: ConversationState = {
    ...state,
    unconfirmedDurableFrames: {
      ...state.unconfirmedDurableFrames,
      [frame.event_seq]: frame,
    },
  }
  if (next.appliedEventSeqs.has(frame.event_seq)) return next

  next = projectVisibleProductEvent(
    next,
    {
      event_seq: frame.event_seq,
      event_version: frame.event_version,
      session_id: frame.session_id,
      turn_id: frame.turn_id,
      created_at: frame.created_at,
      event: frame.event,
    },
    true
  )

  const event = frame.event
  switch (event.type) {
    case "loop_started":
      return reconcileTransientState({
        ...next,
        error: null,
        cancelRequested: false,
        acceptedTurnId:
          next.acceptedTurnId === frame.turn_id ? null : next.acceptedTurnId,
        view:
          next.view === null
            ? null
            : {
                ...next.view,
                status: "running",
                session_id: frame.session_id,
                current_turn_id: frame.turn_id,
                resume_required: false,
              },
      })
    case "tool_approval_requested": {
      const approval: ApprovalRequest = {
        approvalId: event.data.approval_id,
        callId: event.data.call_id,
        toolName: event.data.tool_name,
        arguments: event.data.arguments,
        toolKind: event.data.tool_kind,
        dangerLevel: event.data.danger_level,
      }
      return {
        ...next,
        approvals: { ...next.approvals, [approval.approvalId]: approval },
      }
    }
    case "tool_approval_resolved": {
      if (!(event.data.approval_id in next.approvals)) return next
      const approvals = { ...next.approvals }
      delete approvals[event.data.approval_id]
      return { ...next, approvals }
    }
    case "iteration_completed":
      return next.view === null
        ? next
        : {
            ...next,
            view: { ...next.view, latest_usage: event.data.usage },
          }
    case "loop_finished":
      return projectTerminalFrame(
        confirmDurableTurn(next, frame),
        "finished",
        event.data.usage
      )
    case "loop_failed":
      return projectTerminalFrame(
        confirmDurableTurn(next, frame),
        "failed",
        event.data.usage
      )
    case "loop_cancelled":
      return projectTerminalFrame(
        confirmDurableTurn(next, frame),
        "cancelled",
        event.data.usage
      )
    default:
      return next
  }
}

function matchesFrameIdentity(
  state: ConversationState,
  frame: DurableFrame | TelemetryFrame
): boolean {
  return (
    state.agentRuntimeId !== null &&
    state.agentId !== null &&
    frame.agent_runtime_id === state.agentRuntimeId &&
    frame.agent_id === state.agentId
  )
}

function confirmDurableTurn(
  state: ConversationState,
  frame: DurableFrame
): ConversationState {
  return {
    ...state,
    acceptedTurnId:
      state.acceptedTurnId === frame.turn_id ? null : state.acceptedTurnId,
    view:
      state.view === null
        ? null
        : {
            ...state.view,
            session_id: frame.session_id,
            current_turn_id: frame.turn_id,
          },
  }
}

function projectTerminalFrame(
  state: ConversationState,
  status: "finished" | "failed" | "cancelled",
  usage: AgentRuntimeView["latest_usage"]
): ConversationState {
  const cleaned = terminalCleanup(state)
  return {
    ...cleaned,
    error: null,
    view:
      cleaned.view === null
        ? null
        : {
            ...cleaned.view,
            status,
            latest_usage: usage ?? cleaned.view.latest_usage,
            resume_required: false,
          },
  }
}

function terminalCleanup(state: ConversationState): ConversationState {
  let tools: Record<string, ToolProgress> | null = null
  for (const [callId, tool] of Object.entries(state.tools)) {
    if (tool.status !== "streaming" || tool.result !== null) continue
    if (tools === null) tools = { ...state.tools }
    tools[callId] = { ...tool, status: "interrupted" }
  }
  const closedLlmCallIds = new Set(state.closedLlmCallIds)
  for (const callId of Object.keys(state.drafts)) closedLlmCallIds.add(callId)
  if (state.activeLlmCallId !== null)
    closedLlmCallIds.add(state.activeLlmCallId)
  return {
    ...state,
    drafts: {},
    activeLlmCallId: null,
    closedLlmCallIds,
    tools: tools ?? state.tools,
    approvals: {},
    cancelRequested: false,
  }
}

function projectVisibleProductEvent(
  state: ConversationState,
  record: AgentRuntimeDurableRecordV1,
  convergeDraft: boolean
): ConversationState {
  const event = record.event
  switch (event.type) {
    case "message_appended":
      return projectMessageAppended(
        withApplied(state, record.event_seq),
        record,
        event.data.message,
        convergeDraft
      )
    case "transcript_compacted": {
      const next = withApplied(state, record.event_seq)
      if (next.agentRuntimeId === null) return next
      return {
        ...next,
        timeline: insertTimelineEntry(next.timeline, {
          kind: "compaction",
          marker: {
            agentRuntimeId: next.agentRuntimeId,
            eventSeq: record.event_seq,
            summary: compactionSummaryText(event.data.summary),
            compactedIteration: event.data.compacted_iteration,
            timestamp: record.created_at,
          },
        }),
      }
    }
    case "loop_failed":
    case "loop_cancelled": {
      const next = withApplied(state, record.event_seq)
      if (next.agentRuntimeId === null) return next
      return {
        ...next,
        timeline: insertTimelineEntry(next.timeline, {
          kind: "terminal",
          marker: {
            agentRuntimeId: next.agentRuntimeId,
            eventSeq: record.event_seq,
            terminal: event.type === "loop_failed" ? "failed" : "cancelled",
            errorText:
              event.type === "loop_failed" ? event.data.error_text : null,
            timestamp: record.created_at,
          },
        }),
      }
    }
    default:
      return withApplied(state, record.event_seq)
  }
}

const COMPACTION_SUMMARY_PREFIX = "[stratum:transcript-compacted]"

function compactionSummaryText(summary: ChatMessage): string {
  const text =
    summary.content.type === "text"
      ? summary.content.data
      : JSON.stringify(summary.content.data)
  return text.startsWith(COMPACTION_SUMMARY_PREFIX)
    ? text.slice(COMPACTION_SUMMARY_PREFIX.length).replace(/^\s+/, "")
    : text
}

function projectMessageAppended(
  state: ConversationState,
  record: AgentRuntimeDurableRecordV1,
  message: ChatMessage,
  convergeDraft: boolean
): ConversationState {
  if (message.role === "tool")
    return projectPersistedToolResult(state, message, record.turn_id)
  if (state.agentRuntimeId === null) return state

  const stableMessage: StableMessage = {
    agentRuntimeId: state.agentRuntimeId,
    eventSeq: record.event_seq,
    role: message.role,
    text: message.content.type === "text" ? message.content.data : null,
    json: message.content.type === "json" ? message.content.data : null,
    reasoning: message.reasoning_content ?? null,
    toolCalls: (message.tool_calls ?? []).map((toolCall) => ({
      callId: toolCall.call_id,
      name: toolCall.name,
      arguments: toolCall.arguments,
    })),
    timestamp: record.created_at,
  }

  let next = state
  if (convergeDraft && message.role === "assistant")
    next = convergeAssistantFinal(next, record.event_seq)
  next = stableMessage.toolCalls.reduce(
    (current, toolCall) =>
      projectPersistedToolCall(current, toolCall, record.turn_id),
    next
  )
  return {
    ...next,
    timeline: insertTimelineEntry(next.timeline, {
      kind: "message",
      message: stableMessage,
    }),
  }
}

function convergeAssistantFinal(
  state: ConversationState,
  eventSeq: string
): ConversationState {
  if (compareEventSeq(eventSeq, state.telemetryFloorEventSeq) <= 0) return state
  const advanced = { ...state, telemetryFloorEventSeq: eventSeq }
  if (state.activeLlmCallId === null) return advanced

  const activeDraft = state.drafts[state.activeLlmCallId]
  if (
    activeDraft !== undefined &&
    compareEventSeq(activeDraft.durableBeforeEventSeq, eventSeq) >= 0
  )
    return advanced

  const closedCallId = state.activeLlmCallId
  const drafts = { ...state.drafts }
  delete drafts[closedCallId]
  const closedLlmCallIds = new Set(state.closedLlmCallIds)
  closedLlmCallIds.add(closedCallId)
  return {
    ...advanced,
    drafts,
    activeLlmCallId: null,
    closedLlmCallIds,
  }
}

function projectPersistedToolCall(
  state: ConversationState,
  toolCall: StableMessage["toolCalls"][number],
  turnId: string
): ConversationState {
  const existing = state.tools[toolCall.callId]
  const tool: ToolProgress = {
    callId: toolCall.callId,
    turnId,
    llmCallId: existing?.llmCallId ?? null,
    name: existing?.name ?? toolCall.name,
    argumentsText:
      existing?.argumentsText || JSON.stringify(toolCall.arguments),
    result: existing?.result ?? null,
    errorText: existing?.errorText ?? null,
    status: existing?.status ?? "streaming",
  }
  return { ...state, tools: { ...state.tools, [tool.callId]: tool } }
}

function projectPersistedToolResult(
  state: ConversationState,
  message: ChatMessage,
  turnId: string
): ConversationState {
  if (!message.tool_call_id) return state
  const existing = state.tools[message.tool_call_id]
  const tool: ToolProgress = {
    callId: message.tool_call_id,
    turnId,
    llmCallId: existing?.llmCallId ?? null,
    name: existing?.name ?? null,
    argumentsText: existing?.argumentsText ?? "",
    result: message.content.data,
    errorText: null,
    status: "finished",
  }
  return { ...state, tools: { ...state.tools, [tool.callId]: tool } }
}

function projectTelemetryFrame(
  state: ConversationState,
  frame: TelemetryFrame
): ConversationState {
  if (!matchesFrameIdentity(state, frame)) return state
  const expectedTurnId =
    state.acceptedTurnId ??
    (state.view?.status === "running" ? state.view.current_turn_id : null)
  if (expectedTurnId !== frame.turn_id) return state
  if (
    compareEventSeq(
      frame.durable_before_event_seq,
      state.telemetryFloorEventSeq
    ) < 0
  )
    return state

  const callId = frame.llm_call_id
  const existing = state.drafts[callId]
  if (existing === undefined && state.closedLlmCallIds.has(callId)) return state

  const nextExpected = existing?.nextTelemetrySeq ?? "0"
  if (compareEventSeq(frame.telemetry_seq, nextExpected) < 0) return state

  const draft: DraftState = existing ?? {
    llmCallId: callId,
    turnId: frame.turn_id,
    durableBeforeEventSeq: frame.durable_before_event_seq,
    text: "",
    reasoning: "",
    nextTelemetrySeq: "0",
    incomplete:
      frame.event.type !== "llm_started" || frame.telemetry_seq !== "0",
  }
  if (draft.turnId !== frame.turn_id) return state

  const incomplete =
    draft.incomplete ||
    compareEventSeq(frame.telemetry_seq, draft.nextTelemetrySeq) > 0
  const next: ConversationState = {
    ...state,
    drafts: {
      ...state.drafts,
      [callId]: {
        ...draft,
        incomplete,
        nextTelemetrySeq: incrementEventSeq(frame.telemetry_seq),
      },
    },
    activeLlmCallId: existing === undefined ? callId : state.activeLlmCallId,
  }

  switch (frame.event.type) {
    case "llm_started":
      return { ...next, activeLlmCallId: callId, error: null }
    case "text_delta":
      return updateDraft(next, callId, { text: frame.event.data.delta })
    case "reasoning_delta":
      return updateDraft(next, callId, { reasoning: frame.event.data.delta })
    case "tool_call_delta":
      return updateStreamingTool(next, callId, frame.turn_id, frame.event.data)
    case "llm_finished":
      return next
  }
}

function updateDraft(
  state: ConversationState,
  llmCallId: string,
  delta: Partial<{ text: string; reasoning: string }>
): ConversationState {
  const draft = state.drafts[llmCallId]
  if (draft === undefined) return state
  return {
    ...state,
    drafts: {
      ...state.drafts,
      [llmCallId]: {
        ...draft,
        text: draft.text + (delta.text ?? ""),
        reasoning: draft.reasoning + (delta.reasoning ?? ""),
      },
    },
  }
}

function updateStreamingTool(
  state: ConversationState,
  llmCallId: string,
  turnId: string,
  data: { call_id: string; name?: string | null; arguments_delta: string }
): ConversationState {
  const existing = state.tools[data.call_id]
  if (existing !== undefined && existing.status !== "streaming") return state
  const tool: ToolProgress = {
    callId: data.call_id,
    turnId,
    llmCallId,
    name: data.name ?? existing?.name ?? null,
    argumentsText: (existing?.argumentsText ?? "") + data.arguments_delta,
    result: existing?.result ?? null,
    errorText: existing?.errorText ?? null,
    status: "streaming",
  }
  return { ...state, tools: { ...state.tools, [tool.callId]: tool } }
}

/** Keep transient telemetry only for the exact accepted/running Turn. */
function reconcileTransientState(state: ConversationState): ConversationState {
  const expectedTurnId =
    state.acceptedTurnId ??
    (state.view?.status === "running" ? state.view.current_turn_id : null)
  if (expectedTurnId === null) return terminalCleanup(state)

  const drafts = Object.fromEntries(
    Object.entries(state.drafts).filter(
      ([, draft]) => draft.turnId === expectedTurnId
    )
  )
  const tools = Object.fromEntries(
    Object.entries(state.tools).map(([callId, tool]) => [
      callId,
      tool.turnId !== expectedTurnId &&
      tool.status === "streaming" &&
      tool.result === null
        ? { ...tool, status: "interrupted" as const }
        : tool,
    ])
  )
  const activeLlmCallId =
    state.activeLlmCallId !== null && state.activeLlmCallId in drafts
      ? state.activeLlmCallId
      : null
  return { ...state, drafts, tools, activeLlmCallId }
}
