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
  AgentRuntimeDurableRecordV1,
  AgentRuntimeProductEventV1,
  AgentRuntimeView,
  LlmTelemetryEventV1,
} from "@/lib/stratum/api"

const RUNTIME_ID = "runtime-1"
const AGENT_ID = "agent-definition-1"
const SESSION_ID = "session-1"
const TURN_ID = "turn-1"
const USAGE = { input_tokens: 1, output_tokens: 2, total_tokens: 3 }

function runtimeView(
  overrides: Partial<AgentRuntimeView> = {}
): AgentRuntimeView {
  return {
    agent_runtime_id: RUNTIME_ID,
    agent_id: AGENT_ID,
    agent_name: "researcher",
    agent_version: "v-author-tag",
    status: "running",
    model_config: { model: "openai:gpt-5", parameters: {} },
    session_id: SESSION_ID,
    current_turn_id: TURN_ID,
    snapshot_event_seq: "7",
    telemetry_floor_event_seq: "0",
    pending_approvals: [],
    latest_usage: null,
    resume_required: false,
    ...overrides,
  }
}

function record(
  eventSeq: string,
  event: AgentRuntimeProductEventV1,
  overrides: Partial<AgentRuntimeDurableRecordV1> = {}
): AgentRuntimeDurableRecordV1 {
  return {
    event_seq: eventSeq,
    event_version: 1,
    session_id: SESSION_ID,
    turn_id: TURN_ID,
    created_at: `2026-08-09T00:00:${eventSeq.padStart(2, "0")}Z`,
    event,
    ...overrides,
  }
}

function durableFrame(
  eventSeq: string,
  event: AgentRuntimeProductEventV1,
  overrides: Partial<DurableFrame> = {}
): DurableFrame {
  return {
    protocol_version: 1,
    kind: "durable",
    agent_runtime_id: RUNTIME_ID,
    agent_id: AGENT_ID,
    ...record(eventSeq, event),
    ...overrides,
  }
}

function telemetryFrame(
  telemetrySeq: string,
  event: LlmTelemetryEventV1,
  overrides: Partial<TelemetryFrame> = {}
): TelemetryFrame {
  return {
    protocol_version: 1,
    kind: "telemetry",
    agent_runtime_id: RUNTIME_ID,
    agent_id: AGENT_ID,
    session_id: SESSION_ID,
    turn_id: TURN_ID,
    created_at: "2026-08-09T00:01:00Z",
    durable_before_event_seq: "7",
    llm_call_id: "llm-call-1",
    telemetry_seq: telemetrySeq,
    event,
    ...overrides,
  }
}

function readyState(
  view = runtimeView(),
  items: readonly AgentRuntimeDurableRecordV1[] = []
): ConversationState {
  let state = conversationReducer(initialConversationState, {
    type: "runtime_selected",
    agentRuntimeId: RUNTIME_ID,
  })
  state = conversationReducer(state, {
    type: "snapshot_loaded",
    view,
    items,
    historyBefore: items[0]?.event_seq ?? null,
    historyHasMore: false,
  })
  return conversationReducer(state, { type: "recovery_ready" })
}

const userMessage = (text: string): AgentRuntimeProductEventV1 => ({
  type: "message_appended",
  data: {
    message: { role: "user", content: { type: "text", data: text } },
  },
})

const assistantMessage = (text: string): AgentRuntimeProductEventV1 => ({
  type: "message_appended",
  data: {
    message: { role: "assistant", content: { type: "text", data: text } },
  },
})

describe("conversationReducer PostgreSQL barrier", () => {
  it("initializes the PG-confirmed barrier and telemetry floor from one cold view", () => {
    const state = readyState(
      runtimeView({
        snapshot_event_seq: "20",
        telemetry_floor_event_seq: "18",
      }),
      [record("17", userMessage("persisted"))]
    )

    expect(state.agentRuntimeId).toBe(RUNTIME_ID)
    expect(state.agentId).toBe(AGENT_ID)
    expect(state.pgConfirmedEventSeq).toBe("20")
    expect(state.telemetryFloorEventSeq).toBe("18")
    expect(state.timeline).toHaveLength(1)
  })

  it("projects a NATS product immediately without advancing the PG barrier", () => {
    const state = readyState()
    const next = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("10", userMessage("from realtime")),
    })

    expect(next.pgConfirmedEventSeq).toBe("7")
    expect(Object.keys(next.unconfirmedDurableFrames)).toEqual(["10"])
    expect(
      next.timeline.map((entry) =>
        entry.kind === "message"
          ? entry.message.eventSeq
          : entry.marker.eventSeq
      )
    ).toEqual(["10"])
  })

  it("fills a missed lower product and de-duplicates a seen higher product in complete (B,T]", () => {
    let state = readyState()
    state = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("10", assistantMessage("already seen")),
    })

    const next = conversationReducer(state, {
      type: "view_reconciled",
      basePgConfirmedEventSeq: "7",
      view: runtimeView({
        snapshot_event_seq: "10",
        telemetry_floor_event_seq: "10",
      }),
      items: [
        record("8", userMessage("publish failed")),
        record("10", assistantMessage("already seen")),
      ],
    })

    expect(next.pgConfirmedEventSeq).toBe("10")
    expect(next.unconfirmedDurableFrames).toEqual({})
    expect(
      next.timeline.map((entry) =>
        entry.kind === "message"
          ? entry.message.eventSeq
          : entry.marker.eventSeq
      )
    ).toEqual(["8", "10"])
  })

  it("rebases using the current unconfirmed map and replays frames newer than view@T", () => {
    let state = readyState()
    state = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("10", {
        type: "loop_cancelled",
        data: { usage: USAGE },
      }),
    })
    expect(state.view?.status).toBe("cancelled")

    const next = conversationReducer(state, {
      type: "view_reconciled",
      basePgConfirmedEventSeq: "7",
      view: runtimeView({ snapshot_event_seq: "9", status: "running" }),
      items: [],
    })

    expect(next.pgConfirmedEventSeq).toBe("9")
    expect(next.view?.status).toBe("cancelled")
    expect(Object.keys(next.unconfirmedDurableFrames)).toEqual(["10"])
  })

  it("drops a stale reconcile generation without changing any state", () => {
    const state = readyState(runtimeView({ snapshot_event_seq: "12" }))
    const next = conversationReducer(state, {
      type: "view_reconciled",
      basePgConfirmedEventSeq: "7",
      view: runtimeView({ snapshot_event_seq: "14" }),
      items: [record("14", userMessage("stale"))],
    })

    expect(next).toBe(state)
  })

  it("ignores product frames with either identity mismatched", () => {
    const state = readyState()
    const wrongRuntime = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("8", userMessage("wrong"), {
        agent_runtime_id: "runtime-2",
      }),
    })
    const wrongDefinition = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("8", userMessage("wrong"), {
        agent_id: "agent-definition-2",
      }),
    })

    expect(wrongRuntime).toBe(state)
    expect(wrongDefinition).toBe(state)
  })
})

describe("conversationReducer volatile telemetry", () => {
  it("accepts only the exact accepted Turn before the cold view catches up", () => {
    let state = readyState(
      runtimeView({
        status: "idle",
        session_id: null,
        current_turn_id: null,
        snapshot_event_seq: "0",
      })
    )
    state = conversationReducer(state, {
      type: "turn_accepted",
      agentRuntimeId: RUNTIME_ID,
      agentId: AGENT_ID,
      turnId: "turn-2",
    })

    const oldTurn = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("0", { type: "llm_started" }),
    })
    const acceptedTurn = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame(
        "0",
        { type: "llm_started" },
        {
          turn_id: "turn-2",
          durable_before_event_seq: "0",
        }
      ),
    })

    expect(oldTurn.drafts).toEqual({})
    expect(acceptedTurn.drafts["llm-call-1"]?.turnId).toBe("turn-2")
  })

  it("marks a call incomplete on a sequence gap and ignores late duplicates", () => {
    let state = readyState()
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("0", { type: "llm_started" }),
    })
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("2", {
        type: "text_delta",
        data: { delta: "after-gap" },
      }),
    })
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("1", {
        type: "text_delta",
        data: { delta: "late" },
      }),
    })

    expect(state.drafts["llm-call-1"]).toMatchObject({
      text: "after-gap",
      nextTelemetrySeq: "3",
      incomplete: true,
    })
  })

  it("replaces an active draft with durable final and rejects its old tail", () => {
    let state = readyState()
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("0", { type: "llm_started" }),
    })
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("1", {
        type: "text_delta",
        data: { delta: "partial" },
      }),
    })
    state = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("8", assistantMessage("complete")),
    })

    expect(state.drafts).toEqual({})
    expect(state.closedLlmCallIds.has("llm-call-1")).toBe(true)
    expect(state.telemetryFloorEventSeq).toBe("8")

    const late = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("2", {
        type: "text_delta",
        data: { delta: "late" },
      }),
    })
    const nextCall = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame(
        "0",
        { type: "llm_started" },
        {
          llm_call_id: "llm-call-2",
          durable_before_event_seq: "8",
        }
      ),
    })

    expect(late).toBe(state)
    expect(nextCall.drafts["llm-call-2"]).toBeDefined()
  })

  it("cleans drafts and interrupts result-less tools at every terminal", () => {
    let state = readyState()
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("0", { type: "llm_started" }),
    })
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("1", {
        type: "tool_call_delta",
        data: {
          call_id: "tool-call-1",
          name: "write_file",
          arguments_delta: "{",
        },
      }),
    })
    state = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("8", {
        type: "loop_failed",
        data: { error_text: "safe failure", usage: USAGE },
      }),
    })

    expect(state.view?.status).toBe("failed")
    expect(state.drafts).toEqual({})
    expect(state.tools["tool-call-1"]?.status).toBe("interrupted")
    expect(state.cancelRequested).toBe(false)
  })

  it("drops an old-Turn draft and interrupts its tool when the next Turn starts", () => {
    let state = readyState()
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("0", { type: "llm_started" }),
    })
    state = conversationReducer(state, {
      type: "telemetry_frame",
      frame: telemetryFrame("1", {
        type: "tool_call_delta",
        data: {
          call_id: "tool-call-1",
          name: "lookup",
          arguments_delta: "{",
        },
      }),
    })
    state = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame(
        "8",
        { type: "loop_started" },
        {
          turn_id: "turn-2",
        }
      ),
    })

    expect(state.view).toMatchObject({
      status: "running",
      current_turn_id: "turn-2",
    })
    expect(state.drafts).toEqual({})
    expect(state.tools["tool-call-1"]?.status).toBe("interrupted")
  })
})

describe("conversationReducer history and approvals", () => {
  it("strict history consumption only renders message, compaction, and safe terminal markers", () => {
    const state = readyState(
      runtimeView({ snapshot_event_seq: "20", status: "running" }),
      [
        record("1", { type: "loop_started" }),
        record("2", userMessage("old message")),
        record("3", {
          type: "tool_approval_requested",
          data: {
            approval_id: "old-approval",
            call_id: "old-call",
            tool_name: "writer",
            arguments: {},
            tool_kind: "write",
            danger_level: "high",
          },
        }),
        record("4", {
          type: "iteration_completed",
          data: { iteration: 1, usage: USAGE },
        }),
        record("5", {
          type: "loop_finished",
          data: { finish_reason: "stop", usage: USAGE },
        }),
        record("6", {
          type: "transcript_compacted",
          data: {
            summary: {
              role: "system",
              content: {
                type: "text",
                data: "[stratum:transcript-compacted] summary",
              },
            },
            compacted_iteration: 1,
          },
        }),
        record("7", {
          type: "loop_cancelled",
          data: { usage: USAGE },
        }),
      ]
    )

    expect(state.view?.status).toBe("running")
    expect(state.approvals).toEqual({})
    expect(state.timeline.map((entry) => entry.kind)).toEqual([
      "message",
      "compaction",
      "terminal",
    ])
    expect(state.timeline[1]).toMatchObject({
      kind: "compaction",
      marker: { summary: "summary" },
    })
  })

  it("projects live approval request/resolve and keeps cancel pending advisory", () => {
    let state = readyState()
    state = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("8", {
        type: "tool_approval_requested",
        data: {
          approval_id: "approval-1",
          call_id: "call-1",
          tool_name: "writer",
          arguments: { path: "safe.txt" },
          tool_kind: "write",
          danger_level: "high",
        },
      }),
    })
    state = conversationReducer(state, { type: "cancel_requested" })
    expect(state.approvals["approval-1"]).toBeDefined()
    expect(state.cancelRequested).toBe(true)

    state = conversationReducer(state, {
      type: "durable_frame",
      frame: durableFrame("9", {
        type: "tool_approval_resolved",
        data: { approval_id: "approval-1", decision: "reject" },
      }),
    })
    expect(state.approvals).toEqual({})
    expect(state.cancelRequested).toBe(true)
  })

  it("does not let upward pagination move the PG barrier or current view", () => {
    const state = readyState(runtimeView({ snapshot_event_seq: "20" }))
    const next = conversationReducer(state, {
      type: "history_page_loaded",
      items: [record("2", userMessage("older"))],
      historyBefore: "2",
      historyHasMore: false,
    })

    expect(next.pgConfirmedEventSeq).toBe("20")
    expect(next.view).toBe(state.view)
    expect(next.timeline).toHaveLength(1)
  })

  it("retains a durable historical tool result across a newer running Turn", () => {
    const assistantWithTool: AgentRuntimeProductEventV1 = {
      type: "message_appended",
      data: {
        message: {
          role: "assistant",
          content: { type: "text", data: "checking" },
          tool_calls: [
            { call_id: "old-tool", name: "lookup", arguments: { q: "x" } },
          ],
        },
      },
    }
    const toolResult: AgentRuntimeProductEventV1 = {
      type: "message_appended",
      data: {
        message: {
          role: "tool",
          content: { type: "json", data: { answer: 42 } },
          tool_call_id: "old-tool",
        },
      },
    }
    const state = readyState(runtimeView({ snapshot_event_seq: "20" }), [
      record("2", assistantWithTool, { turn_id: "turn-0" }),
      record("3", toolResult, { turn_id: "turn-0" }),
    ])

    expect(state.tools["old-tool"]).toMatchObject({
      turnId: "turn-0",
      status: "finished",
      result: { answer: 42 },
    })
  })
})
