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
    telemetry_floor_event_seq: "0",
    pending_approvals: [],
    latest_usage: null,
    resume_required: false,
    ...overrides,
  }
}

function readyState(
  overrides: Partial<ConversationState> = {}
): ConversationState {
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
  event: TelemetryFrame["event"],
  durableBeforeEventSeq = "10"
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
    durable_before_event_seq: durableBeforeEventSeq,
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
      baseBarrier: "10",
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
      baseBarrier: "12",
      view: freshView,
      items: [
        historyItem("13", {
          type: "message_appended",
          data: { message: textMessage("hi", "user") },
        }),
        historyItem("14", {
          type: "loop_finished",
          data: {
            finish_reason: "stop",
            usage: { input_tokens: 1, output_tokens: 2, total_tokens: 3 },
          },
        }),
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
      baseBarrier: "12",
      view,
      items: [],
    })

    expect(next.barrier).toBe("12")
    expect(next.view).toBe(view)
  })

  it("discards a bundle when realtime advanced after reconcile started", () => {
    const state = readyState({
      barrier: "12",
      view: makeView({ status: "finished", snapshot_event_seq: "12" }),
    })
    const fetchedBeforeRealtime = makeView({
      status: "running",
      snapshot_event_seq: "12",
      resume_required: true,
    })

    const next = conversationReducer(state, {
      type: "view_reconciled",
      baseBarrier: "10",
      view: fetchedBeforeRealtime,
      items: [],
    })

    expect(next).toBe(state)
  })
})

describe("telemetry gap handling", () => {
  it("seeds the cold telemetry floor even when the final is outside the latest page", () => {
    const snapshot = conversationReducer(
      {
        ...initialConversationState,
        agentId: AGENT_ID,
        phase: "recovering",
      },
      {
        type: "snapshot_loaded",
        view: makeView({
          snapshot_event_seq: "100",
          telemetry_floor_event_seq: "40",
        }),
        items: [],
        historyBefore: "80",
        historyHasMore: true,
      }
    )

    expect(snapshot.telemetryFloorEventSeq).toBe("40")
    const lateOldCall = conversationReducer(snapshot, {
      type: "telemetry_frame",
      frame: telemetryFrame(
        "old-call",
        1,
        {
          type: "text_delta",
          data: { delta: "late" },
        },
        "39"
      ),
    })
    expect(lateOldCall).toBe(snapshot)

    const nextCall = conversationReducer(snapshot, {
      type: "telemetry_frame",
      frame: telemetryFrame("next-call", 0, { type: "llm_started" }, "40"),
    })
    expect(nextCall.activeLlmCallId).toBe("next-call")
  })

  it("rejects old-call telemetry when PG reconcile observes the final first", () => {
    const reconciled = conversationReducer(readyState(), {
      type: "view_reconciled",
      baseBarrier: "10",
      view: makeView({ snapshot_event_seq: "11" }),
      items: [
        historyItem("11", {
          type: "message_appended",
          data: { message: textMessage("complete", "assistant") },
        }),
      ],
    })

    expect(reconciled.telemetryFloorEventSeq).toBe("11")
    const lateOldCall = conversationReducer(reconciled, {
      type: "telemetry_frame",
      frame: telemetryFrame("old-call", 0, { type: "llm_started" }, "10"),
    })
    expect(lateOldCall).toBe(reconciled)

    // 下一 call 在 final receipt 后入队，watermark 不低于 final，可以正常流式。
    const nextCall = conversationReducer(reconciled, {
      type: "telemetry_frame",
      frame: telemetryFrame("next-call", 0, { type: "llm_started" }, "11"),
    })
    expect(nextCall.activeLlmCallId).toBe("next-call")
    expect(nextCall.drafts["next-call"]).toBeDefined()

    const duplicateKnownFinal = conversationReducer(nextCall, {
      type: "durable_frame",
      frame: durableFrame("11", {
        type: "message_appended",
        data: { message: textMessage("complete", "assistant") },
      }),
    })
    expect(duplicateKnownFinal).toBe(nextCall)
  })

  it("uses the reconciled floor when the corresponding final is outside the fetched interval", () => {
    const streaming = conversationReducer(readyState(), {
      type: "telemetry_frame",
      frame: telemetryFrame("old-call", 0, { type: "llm_started" }, "10"),
    })
    const reconciled = conversationReducer(streaming, {
      type: "view_reconciled",
      baseBarrier: "10",
      view: makeView({
        snapshot_event_seq: "10",
        telemetry_floor_event_seq: "9",
      }),
      items: [],
    })
    expect(reconciled.activeLlmCallId).toBe("old-call")

    const advanced = conversationReducer(reconciled, {
      type: "view_reconciled",
      baseBarrier: "10",
      view: makeView({
        snapshot_event_seq: "12",
        telemetry_floor_event_seq: "11",
      }),
      // A specialized/current page can omit the assistant item; the view
      // floor remains sufficient convergence evidence.
      items: [],
    })
    expect(advanced.activeLlmCallId).toBeNull()
    expect(advanced.telemetryFloorEventSeq).toBe("11")
  })

  it("uses an ordered duplicate final when the cold history page omitted it", () => {
    const snapshot = readyState({
      barrier: "100",
      view: makeView({ snapshot_event_seq: "100" }),
      telemetryFloorEventSeq: "0",
    })
    const ghost = conversationReducer(snapshot, {
      type: "telemetry_frame",
      frame: telemetryFrame(
        "old-call",
        1,
        { type: "text_delta", data: { delta: "late" } },
        "39"
      ),
    })
    expect(ghost.activeLlmCallId).toBe("old-call")

    // Dispatcher publishes this duplicate only after the queued old-call
    // telemetry. It is below the PG barrier, so it converges transient state
    // without appending a second timeline item.
    const converged = conversationReducer(ghost, {
      type: "durable_frame",
      frame: durableFrame("40", {
        type: "message_appended",
        data: { message: textMessage("complete", "assistant") },
      }),
    })
    expect(converged.activeLlmCallId).toBeNull()
    expect(converged.drafts).toEqual({})
    expect(converged.telemetryFloorEventSeq).toBe("40")
    expect(converged.timeline).toEqual(snapshot.timeline)

    const nextCall = conversationReducer(converged, {
      type: "telemetry_frame",
      frame: telemetryFrame("next-call", 0, { type: "llm_started" }, "40"),
    })
    expect(nextCall.activeLlmCallId).toBe("next-call")
  })

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

  it("rejects a previous Turn's queued telemetry after the next Turn is running", () => {
    const nextTurn = readyState({
      barrier: "14",
      view: makeView({
        status: "running",
        current_turn_id: "turn-2",
        snapshot_event_seq: "14",
      }),
    })
    const latePreviousTurn = conversationReducer(nextTurn, {
      type: "telemetry_frame",
      frame: {
        ...telemetryFrame(
          "old-call",
          1,
          { type: "text_delta", data: { delta: "late" } },
          "10"
        ),
        turn_id: "turn-1",
      },
    })

    expect(latePreviousTurn).toBe(nextTurn)
  })
})

describe("accepted Turn convergence", () => {
  it("keeps polling intent across cold recovery until AgentView confirms the Turn", () => {
    const oldView = makeView({
      status: "finished",
      current_turn_id: "turn-1",
      snapshot_event_seq: "10",
    })
    const accepted = conversationReducer(readyState({ view: oldView }), {
      type: "turn_accepted",
      agentId: AGENT_ID,
      turnId: "turn-2",
    })
    expect(accepted.acceptedTurnId).toBe("turn-2")

    const recovering = conversationReducer(accepted, {
      type: "recovery_started",
      agentId: AGENT_ID,
    })
    const staleSnapshot = conversationReducer(recovering, {
      type: "snapshot_loaded",
      view: oldView,
      items: [],
      historyBefore: null,
      historyHasMore: false,
    })
    expect(staleSnapshot.acceptedTurnId).toBe("turn-2")

    const confirmed = conversationReducer(staleSnapshot, {
      type: "view_reconciled",
      baseBarrier: "10",
      view: makeView({
        status: "running",
        current_turn_id: "turn-2",
        snapshot_event_seq: "12",
      }),
      items: [],
    })
    expect(confirmed.acceptedTurnId).toBeNull()
  })

  it("does not arm convergence when realtime already confirmed the Turn", () => {
    const state = readyState({
      view: makeView({ current_turn_id: "turn-2" }),
    })
    const next = conversationReducer(state, {
      type: "turn_accepted",
      agentId: AGENT_ID,
      turnId: "turn-2",
    })
    expect(next).toBe(state)
  })

  it("accepts telemetry for the exact accepted Turn while the old view is terminal", () => {
    const oldView = makeView({ status: "finished", current_turn_id: "turn-1" })
    const accepted = conversationReducer(readyState({ view: oldView }), {
      type: "turn_accepted",
      agentId: AGENT_ID,
      turnId: "turn-2",
    })
    const next = conversationReducer(accepted, {
      type: "telemetry_frame",
      frame: {
        ...telemetryFrame("call-2", 0, { type: "llm_started" }),
        turn_id: "turn-2",
      },
    })

    expect(next.activeLlmCallId).toBe("call-2")
    expect(next.acceptedTurnId).toBe("turn-2")
  })

  it("lets an accepted Turn supersede telemetry from a stale running view", () => {
    const accepted = conversationReducer(readyState(), {
      type: "turn_accepted",
      agentId: AGENT_ID,
      turnId: "turn-2",
    })
    const latePreviousTurn = conversationReducer(accepted, {
      type: "telemetry_frame",
      frame: telemetryFrame("old-call", 0, { type: "llm_started" }),
    })

    expect(latePreviousTurn).toBe(accepted)
  })

  it("uses an exact terminal frame to confirm and close the accepted Turn", () => {
    const accepted = conversationReducer(
      readyState({
        view: makeView({ status: "finished", current_turn_id: "turn-1" }),
      }),
      { type: "turn_accepted", agentId: AGENT_ID, turnId: "turn-2" }
    )
    const terminal = conversationReducer(accepted, {
      type: "durable_frame",
      frame: {
        ...durableFrame("11", {
          type: "loop_finished",
          data: {
            finish_reason: "stop",
            usage: { input_tokens: 1, output_tokens: 1, total_tokens: 2 },
          },
        }),
        turn_id: "turn-2",
      },
    })

    expect(terminal.acceptedTurnId).toBeNull()
    expect(terminal.view).toMatchObject({
      status: "finished",
      current_turn_id: "turn-2",
    })
    const late = conversationReducer(terminal, {
      type: "telemetry_frame",
      frame: {
        ...telemetryFrame("call-2", 2, {
          type: "text_delta",
          data: { delta: "late" },
        }),
        turn_id: "turn-2",
      },
    })
    expect(late).toBe(terminal)
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
