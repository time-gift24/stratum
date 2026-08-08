import { describe, expect, it } from "vitest"

import {
  conversationReducer,
  initialConversationState,
} from "@/features/agent-conversation/reducer"
import type {
  ConversationState,
  DurableFrame,
  TelemetryFrame,
} from "@/features/agent-conversation/types"
import type {
  AgentDurableRecordV1,
  AgentView,
  ChatMessage,
} from "@/lib/stratum/api"
import { ApiError } from "@/lib/stratum/api"

const AGENT_ID = "agent-1"

function makeView(overrides: Partial<AgentView> = {}): AgentView {
  return {
    agent_id: AGENT_ID,
    agent_name: "default",
    status: "running",
    model_config: { model: "anthropic:claude-sonnet", parameters: {} },
    session_id: "session-1",
    current_turn_id: "turn-1",
    snapshot_event_seq: "10",
    pending_approvals: [],
    latest_usage: null,
    resume_required: false,
    ...overrides,
  }
}

function readyState(overrides: Partial<ConversationState> = {}): ConversationState {
  return {
    ...initialConversationState,
    agentId: AGENT_ID,
    view: makeView(),
    barrier: "10",
    phase: "ready",
    ...overrides,
  }
}

function textMessage(text: string, role: "user" | "assistant"): ChatMessage {
  return { role, content: { type: "text", data: text } }
}

function historyItem(
  eventSeq: string,
  event: AgentDurableRecordV1["event"]
): AgentDurableRecordV1 {
  return { event_seq: eventSeq, event_version: 1, event }
}

function durableFrame(
  eventSeq: string,
  event: DurableFrame["event"]
): DurableFrame {
  return {
    protocol_version: 1,
    kind: "durable",
    agent_id: AGENT_ID,
    session_id: "session-1",
    turn_id: "turn-1",
    created_at: "2026-01-01T00:00:00.000Z",
    event_seq: eventSeq,
    event_version: 1,
    event,
  }
}

function telemetryFrame(
  llmCallId: string,
  telemetrySeq: number,
  event: TelemetryFrame["event"]
): TelemetryFrame {
  return {
    protocol_version: 1,
    kind: "telemetry",
    agent_id: AGENT_ID,
    session_id: "session-1",
    turn_id: "turn-1",
    created_at: "2026-01-01T00:00:00.000Z",
    llm_call_id: llmCallId,
    telemetry_seq: telemetrySeq,
    event,
  }
}

describe("view_reconciled bundle application", () => {
  it("discards a stale bundle whose barrier is below the applied barrier", () => {
    const state = readyState({
      barrier: "12",
      view: makeView({ status: "finished", snapshot_event_seq: "12" }),
      approvals: {},
    })
    // 慢 reconcile：fetch 于 barrier=10，view 仍是 running 且挂着已决审批
    const staleView = makeView({
      status: "running",
      snapshot_event_seq: "10",
      pending_approvals: [
        {
          approval_id: "approval-1",
          call_id: "call-1",
          tool_name: "echo",
          arguments: {},
          tool_kind: "read",
          danger_level: "low",
        },
      ],
    })

    const next = conversationReducer(state, {
      type: "view_reconciled",
      view: staleView,
      items: [historyItem("9", { type: "loop_started" })],
    })

    // 整体丢弃：view/approvals/barrier/timeline 全部保持，不复活已决审批，
    // 不回退 terminal 状态
    expect(next).toBe(state)
  })

  it("applies a fresh bundle atomically: items, view, approvals, barrier", () => {
    const state = readyState({ barrier: "12" })
    const freshView = makeView({
      status: "finished",
      snapshot_event_seq: "14",
      pending_approvals: [],
    })

    const next = conversationReducer(state, {
      type: "view_reconciled",
      view: freshView,
      items: [
        historyItem("13", {
          type: "message_appended",
          data: { message: textMessage("hi", "user") },
        }),
        historyItem("14", { type: "loop_finished", data: {
          finish_reason: "stop",
          usage: { input_tokens: 1, output_tokens: 2, total_tokens: 3 },
        } }),
      ],
    })

    expect(next.barrier).toBe("14")
    expect(next.view).toBe(freshView)
    expect(next.approvals).toEqual({})
    expect(next.timeline).toHaveLength(1)
    expect(next.timeline[0]).toMatchObject({
      kind: "message",
      message: { eventSeq: "13", text: "hi" },
    })
  })

  it("applies a bundle at the current barrier (idempotent)", () => {
    const view = makeView({ snapshot_event_seq: "12" })
    const state = readyState({ barrier: "12", view })

    const next = conversationReducer(state, {
      type: "view_reconciled",
      view,
      items: [],
    })

    expect(next.barrier).toBe("12")
    expect(next.view).toBe(view)
  })
})

describe("telemetry gap handling", () => {
  it("creates an INCOMPLETE draft when llm_started was lost (first delta has a gap)", () => {
    const state = readyState()

    const next = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 3, {
        type: "text_delta",
        data: { delta: "hel" },
      }),
    })

    const draft = next.drafts["call-1"]
    expect(draft).toMatchObject({
      text: "hel",
      incomplete: true,
      nextTelemetrySeq: 4,
    })
    // 新 call 成为 active：durable final 按 activeLlmCallId 收敛 draft
    expect(next.activeLlmCallId).toBe("call-1")
  })

  it("ignores duplicate telemetry below the next expected seq", () => {
    const started = conversationReducer(readyState(), {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 0, { type: "llm_started" }),
    })
    const withDelta = conversationReducer(started, {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 1, {
        type: "text_delta",
        data: { delta: "a" },
      }),
    })

    const dup = conversationReducer(withDelta, {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 1, {
        type: "text_delta",
        data: { delta: "a" },
      }),
    })

    expect(dup).toBe(withDelta)
    expect(dup.drafts["call-1"]?.text).toBe("a")
  })

  it("marks an existing draft incomplete when a later seq is skipped", () => {
    const started = conversationReducer(readyState(), {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 0, { type: "llm_started" }),
    })

    const gapped = conversationReducer(started, {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 2, {
        type: "text_delta",
        data: { delta: "x" },
      }),
    })

    expect(gapped.drafts["call-1"]).toMatchObject({
      incomplete: true,
      nextTelemetrySeq: 3,
    })
  })

  it("closes the call on the durable final message and ignores late telemetry", () => {
    let state = conversationReducer(readyState(), {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 0, { type: "llm_started" }),
    })
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 1, {
        type: "text_delta",
        data: { delta: "hi" },
      }),
    })
    // durable final assistant message：替换 draft 并关闭该 call
    const closed = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("11", {
        type: "message_appended",
        data: { message: textMessage("hi", "assistant") },
      }),
    })

    expect(closed.drafts["call-1"]).toBeUndefined()
    expect(closed.activeLlmCallId).toBeNull()
    expect(closed.closedLlmCallIds.has("call-1")).toBe(true)

    // closed call 的迟到 telemetry：忽略，不重建 draft
    const late = conversationReducer(closed, {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 2, {
        type: "text_delta",
        data: { delta: "!" },
      }),
    })
    expect(late).toBe(closed)
  })

  it("ignores telemetry for calls closed by terminal cleanup", () => {
    let state = conversationReducer(readyState(), {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 0, { type: "llm_started" }),
    })
    state = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("11", {
        type: "loop_cancelled",
        data: { usage: { input_tokens: 0, output_tokens: 0, total_tokens: 0 } },
      }),
    })
    expect(state.drafts).toEqual({})

    const late = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("call-1", 1, {
        type: "text_delta",
        data: { delta: "x" },
      }),
    })
    expect(late).toBe(state)
  })
})

describe("first-message failure surfacing (reducer level)", () => {
  it("surfaces a command failure after agent selection as connection_error", () => {
    const selected = conversationReducer(initialConversationState, {
      type: "agent_selected",
      agentId: AGENT_ID,
    })
    expect(selected.phase).toBe("recovering")

    // createConversation 的首条 sendMessage 失败：reportError → connection_error
    const error = new ApiError("invalid_model", 400, "model not configured")
    const failed = conversationReducer(selected, {
      type: "connection_error",
      error,
    })

    expect(failed.phase).toBe("connection_error")
    expect(failed.error).toMatchObject({ code: "invalid_model" })
  })
})
