import {
  compareEventSeq,
  type AgentDurableRecordV1,
  type AgentView,
  type ChatMessage,
} from "@/lib/stratum/api"
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
 * 事件流 → 会话状态的 reducer。
 *
 * 契约（runtime-event-protocol spec）：
 * - durable identity = (agentId, eventSeq)：frame `<= barrier` 跳过，
 *   appliedEventSeqs 去重；可见序号间隔（内部 Hook/Tool 事件）不算丢帧。
 * - telemetry identity = (llmCallId, telemetrySeq)：低于期待值 = 重复忽略；
 *   高于期待值 = draft 标 incomplete 并等待 durable final 收敛。
 * - final assistant MessageAppended 整体替换 draft 并关闭该 call；
 *   closed call 的迟到 telemetry 忽略。
 * - 任一 terminal（frame 或 reconcile 发现）清空未闭合 draft，并把无 result
 *   的实时 Tool UI 标为 interrupted，不伪造 result。
 */

export const initialConversationState: ConversationState = {
  agentId: null,
  view: null,
  barrier: "0",
  timeline: [],
  appliedEventSeqs: new Set<string>(),
  drafts: {},
  activeLlmCallId: null,
  tools: {},
  approvals: {},
  historyThrough: null,
  historyBefore: null,
  historyHasMore: false,
  historyLoading: false,
  realtimeDegraded: false,
  cancelRequested: false,
  phase: "empty",
  error: null,
}

const TERMINAL_STATUSES = new Set(["finished", "failed", "cancelled"])

export function conversationReducer(
  state: ConversationState,
  action: ConversationAction
): ConversationState {
  switch (action.type) {
    case "agent_selected":
      return action.agentId === null
        ? initialConversationState
        : {
            ...initialConversationState,
            agentId: action.agentId,
            phase: "recovering",
          }
    case "recovery_started":
      // cold rebuild：丢弃该连接的全部 transient 状态（draft/cursor 由外层丢弃）
      return {
        ...initialConversationState,
        agentId: action.agentId,
        realtimeDegraded: state.agentId === action.agentId ? state.realtimeDegraded : false,
        phase: "recovering",
      }
    case "snapshot_loaded": {
      if (state.agentId !== null && state.agentId !== action.view.agent_id)
        return state
      let next: ConversationState = {
        ...initialConversationState,
        agentId: action.view.agent_id,
        view: action.view,
        barrier: action.view.snapshot_event_seq,
        approvals: approvalsFromView(action.view),
        historyThrough: action.view.snapshot_event_seq,
        historyBefore: action.historyBefore,
        historyHasMore: action.historyHasMore,
        realtimeDegraded: state.realtimeDegraded,
        phase: "recovering",
      }
      for (const item of action.items) next = projectHistoryItem(next, item, true)
      return next
    }
    case "history_page_started":
      return { ...state, historyLoading: true }
    case "history_page_failed":
      return { ...state, historyLoading: false }
    case "history_page_loaded": {
      let next = state
      // 向上分页的旧页：不收敛 active draft
      for (const item of action.items)
        next = projectHistoryItem(next, item, false)
      return {
        ...next,
        historyBefore: action.historyBefore,
        historyHasMore: action.historyHasMore,
        historyLoading: false,
      }
    }
    case "durable_frame":
      return projectDurableFrame(state, action.frame)
    case "telemetry_frame":
      return projectTelemetryFrame(state, action.frame)
    case "view_reconciled": {
      if (state.agentId !== action.view.agent_id) return state
      let next = state
      for (const item of action.items) next = projectHistoryItem(next, item, true)
      next = {
        ...next,
        view: action.view,
        barrier:
          compareEventSeq(action.view.snapshot_event_seq, next.barrier) > 0
            ? action.view.snapshot_event_seq
            : next.barrier,
        approvals: approvalsFromView(action.view),
      }
      // reconcile 发现 terminal：与 terminal frame 相同的 draft/Tool 清理
      if (TERMINAL_STATUSES.has(action.view.status))
        next = { ...terminalCleanup(next), view: action.view }
      return next
    }
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

function approvalsFromView(
  view: AgentView
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
  return entry.kind === "message" ? entry.message.eventSeq : entry.marker.eventSeq
}

/** 按 eventSeq 升序插入（常见情形是末位 append，向上分页才走排序） */
function insertTimelineEntry(
  timeline: readonly TimelineEntry[],
  entry: TimelineEntry
): TimelineEntry[] {
  const last = timeline.at(-1)
  if (last === undefined || compareEventSeq(entrySeq(last), entrySeq(entry)) <= 0)
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

/**
 * 应用 history/reconcile 的可见 product item：只写 timeline/tools/draft 收敛，
 * 不改 view（历史 marker 可能属于旧 Turn）。按 eventSeq 去重。
 * `convergeDraft`：reconcile 合并的 current-Turn final message 要关闭 active
 * draft；向上分页的旧页（convergeDraft=false）绝不动 draft。
 */
function projectHistoryItem(
  state: ConversationState,
  item: AgentDurableRecordV1,
  convergeDraft: boolean
): ConversationState {
  if (state.agentId === null || state.appliedEventSeqs.has(item.event_seq))
    return state
  return projectVisibleProductEvent(state, item, "", convergeDraft)
}

/** 应用实时 durable frame：`<= barrier` 跳过，`> barrier` 按 eventSeq 应用 */
function projectDurableFrame(
  state: ConversationState,
  frame: DurableFrame
): ConversationState {
  if (state.agentId === null || frame.agent_id !== state.agentId) return state
  if (compareEventSeq(frame.event_seq, state.barrier) <= 0) return state
  if (state.appliedEventSeqs.has(frame.event_seq)) return state

  const record: AgentDurableRecordV1 = {
    event_seq: frame.event_seq,
    event_version: frame.event_version,
    event: frame.event,
  }
  let next = projectVisibleProductEvent(state, record, frame.created_at, true)
  next = {
    ...next,
    barrier:
      compareEventSeq(frame.event_seq, next.barrier) > 0
        ? frame.event_seq
        : next.barrier,
  }

  const event = frame.event
  switch (event.type) {
    case "loop_started":
      return {
        ...next,
        error: null,
        cancelRequested: false,
        view:
          next.view === null
            ? next.view
            : {
                ...next.view,
                status: "running",
                session_id: frame.session_id,
                current_turn_id: frame.turn_id,
              },
      }
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
      return projectTerminalFrame(next, "finished", event.data.usage)
    case "loop_failed":
      return projectTerminalFrame(next, "failed", event.data.usage)
    case "loop_cancelled":
      return projectTerminalFrame(next, "cancelled", event.data.usage)
    default:
      return next
  }
}

function projectTerminalFrame(
  state: ConversationState,
  status: "finished" | "failed" | "cancelled",
  usage: AgentView["latest_usage"]
): ConversationState {
  const cleaned = terminalCleanup(state)
  return {
    ...cleaned,
    error: null,
    view:
      cleaned.view === null
        ? cleaned.view
        : {
            ...cleaned.view,
            status,
            latest_usage: usage ?? cleaned.view.latest_usage,
            resume_required: false,
          },
  }
}

/** terminal 清理：清空未闭合 draft，无 result 的实时 Tool 标 interrupted */
function terminalCleanup(state: ConversationState): ConversationState {
  let tools: Record<string, ToolProgress> | null = null
  for (const [callId, tool] of Object.entries(state.tools)) {
    if (tool.status !== "streaming" || tool.result !== null) continue
    if (tools === null) tools = { ...state.tools }
    tools[callId] = { ...tool, status: "interrupted" }
  }
  return {
    ...state,
    drafts: {},
    activeLlmCallId: null,
    tools: tools ?? state.tools,
    approvals: {},
    cancelRequested: false,
  }
}

/**
 * 可见 product event 的共享投影（timeline + tools + draft 收敛 + 去重登记），
 * realtime frame 与 history/reconcile item 复用。`convergeDraft` = false 用于
 * 向上分页的旧页：旧 assistant 消息不得关闭当前 Turn 的 active draft。
 */
function projectVisibleProductEvent(
  state: ConversationState,
  record: AgentDurableRecordV1,
  timestamp: string,
  convergeDraft: boolean
): ConversationState {
  const event = record.event
  switch (event.type) {
    case "message_appended":
      return projectMessageAppended(
        withApplied(state, record.event_seq),
        record.event_seq,
        timestamp,
        event.data.message,
        convergeDraft
      )
    case "transcript_compacted": {
      const next = withApplied(state, record.event_seq)
      if (next.agentId === null) return next
      return {
        ...next,
        timeline: insertTimelineEntry(next.timeline, {
          kind: "compaction",
          marker: {
            agentId: next.agentId,
            eventSeq: record.event_seq,
            summary: compactionSummaryText(event.data.summary),
            compactedIteration: event.data.compacted_iteration,
            timestamp,
          },
        }),
      }
    }
    case "loop_failed":
    case "loop_cancelled": {
      const next = withApplied(state, record.event_seq)
      if (next.agentId === null) return next
      return {
        ...next,
        timeline: insertTimelineEntry(next.timeline, {
          kind: "terminal",
          marker: {
            agentId: next.agentId,
            eventSeq: record.event_seq,
            terminal: event.type === "loop_failed" ? "failed" : "cancelled",
            errorText:
              event.type === "loop_failed" ? event.data.error_text : null,
            timestamp,
          },
        }),
      }
    }
    default:
      // loop_started / approval / iteration_completed / loop_finished：
      // 非 timeline 可见项，只登记去重
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
  eventSeq: string,
  timestamp: string,
  message: ChatMessage,
  convergeDraft: boolean
): ConversationState {
  if (message.role === "tool")
    return projectPersistedToolResult(state, message)

  if (state.agentId === null) return state

  const stableMessage: StableMessage = {
    agentId: state.agentId,
    eventSeq,
    role: message.role,
    text: message.content.type === "text" ? message.content.data : null,
    json: message.content.type === "json" ? message.content.data : null,
    reasoning: message.reasoning_content ?? null,
    toolCalls: (message.tool_calls ?? []).map((toolCall) => ({
      callId: toolCall.call_id,
      name: toolCall.name,
      arguments: toolCall.arguments,
    })),
    timestamp,
  }

  let next = state
  // final assistant message 是 draft truth：替换 draft 并关闭该 call
  if (
    convergeDraft &&
    message.role === "assistant" &&
    next.activeLlmCallId !== null
  ) {
    const drafts = { ...next.drafts }
    delete drafts[next.activeLlmCallId]
    next = { ...next, drafts, activeLlmCallId: null }
  }

  next = stableMessage.toolCalls.reduce(
    (current, toolCall) => projectPersistedToolCall(current, toolCall),
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

function projectPersistedToolCall(
  state: ConversationState,
  toolCall: StableMessage["toolCalls"][number]
): ConversationState {
  const existing = state.tools[toolCall.callId]
  const tool: ToolProgress = {
    callId: toolCall.callId,
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

/** Tool completion 的唯一 truth：MessageAppended(role=tool) 的最终结果 */
function projectPersistedToolResult(
  state: ConversationState,
  message: ChatMessage
): ConversationState {
  if (!message.tool_call_id) return state

  const existing = state.tools[message.tool_call_id]
  const tool: ToolProgress = {
    callId: message.tool_call_id,
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
  if (state.agentId === null || frame.agent_id !== state.agentId) return state

  const callId = frame.llm_call_id
  const existing = state.drafts[callId]
  // closed call 的迟到 telemetry：忽略（不重新创建 draft）
  if (existing === undefined && frame.event.type !== "llm_started") {
    if (state.activeLlmCallId !== callId) return state
  }

  const seq = frame.telemetry_seq
  const next_expected = existing?.nextTelemetrySeq ?? 0
  // 低于下一期待值 = 重复，忽略
  if (existing !== undefined && seq < next_expected) return state

  const draft: DraftState = existing ?? {
    llmCallId: callId,
    text: "",
    reasoning: "",
    nextTelemetrySeq: 0,
    // 首次见到的 frame 不是该 call 的起点：prefix 不完整
    incomplete: seq !== 0,
  }
  // 高于下一期待值 = 出现间隔：标 incomplete，等待 durable final 收敛
  const incomplete = draft.incomplete || seq > draft.nextTelemetrySeq

  const next: ConversationState = {
    ...state,
    drafts: {
      ...state.drafts,
      [callId]: { ...draft, incomplete, nextTelemetrySeq: seq + 1 },
    },
  }

  const event = frame.event
  switch (event.type) {
    case "llm_started":
      return { ...next, activeLlmCallId: callId, error: null }
    case "text_delta":
      return updateDraft(next, callId, { text: event.data.delta })
    case "reasoning_delta":
      return updateDraft(next, callId, { reasoning: event.data.delta })
    case "tool_call_delta":
      return updateStreamingTool(next, callId, event.data)
    case "llm_finished":
      // 不关闭 draft：等待 final durable assistant message 收敛
      return next
    default:
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
  data: { call_id: string; name?: string | null; arguments_delta: string }
): ConversationState {
  const existing = state.tools[data.call_id]
  // 已落成（durable result 已到）或已中断的调用不再被迟到 delta 改写
  if (existing !== undefined && existing.status !== "streaming") return state
  const tool: ToolProgress = {
    callId: data.call_id,
    llmCallId,
    name: data.name ?? existing?.name ?? null,
    argumentsText: (existing?.argumentsText ?? "") + data.arguments_delta,
    result: existing?.result ?? null,
    errorText: existing?.errorText ?? null,
    status: "streaming",
  }
  return { ...state, tools: { ...state.tools, [tool.callId]: tool } }
}
