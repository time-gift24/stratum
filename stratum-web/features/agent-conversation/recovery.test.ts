import { describe, expect, it, vi } from "vitest"

import {
  loadOlderHistoryPage,
  reconcileConversation,
  runConversationSession,
} from "@/features/agent-conversation/recovery"
import {
  conversationReducer,
  initialConversationState,
} from "@/features/agent-conversation/reducer"
import type {
  ConversationAction,
  ConversationState,
  DurableFrame,
  TelemetryFrame,
} from "@/features/agent-conversation/types"
import {
  ApiError,
  type AgentRuntimeDurableRecordV1,
  type AgentRuntimeHistoryPage,
  type AgentRuntimeProductEventV1,
  type AgentRuntimeStreamFrameV1,
  type AgentRuntimeView,
} from "@/lib/stratum/api"
import type { subscribeToAgentRuntimeEvents } from "@/lib/stratum/event-stream"

const RUNTIME_ID = "runtime-1"
const AGENT_ID = "agent-definition-1"
const SESSION_ID = "session-1"
const TURN_ID = "turn-1"

function runtimeView(
  overrides: Partial<AgentRuntimeView> = {}
): AgentRuntimeView {
  return {
    agent_runtime_id: RUNTIME_ID,
    agent_id: AGENT_ID,
    agent_name: "researcher",
    agent_version: "author-tag",
    status: "running",
    model_config: { model: "openai:gpt-5", parameters: {} },
    session_id: SESSION_ID,
    current_turn_id: TURN_ID,
    snapshot_event_seq: "7",
    telemetry_floor_event_seq: "6",
    pending_approvals: [],
    latest_usage: null,
    resume_required: false,
    ...overrides,
  }
}

function record(
  eventSeq: string,
  event: AgentRuntimeProductEventV1
): AgentRuntimeDurableRecordV1 {
  return {
    event_seq: eventSeq,
    event_version: 1,
    session_id: SESSION_ID,
    turn_id: TURN_ID,
    created_at: "2026-08-09T00:00:00Z",
    event,
  }
}

const userMessage = (text: string): AgentRuntimeProductEventV1 => ({
  type: "message_appended",
  data: {
    message: { role: "user", content: { type: "text", data: text } },
  },
})

function readyFrame(agentId = AGENT_ID): AgentRuntimeStreamFrameV1 {
  return {
    protocol_version: 1,
    kind: "control",
    agent_runtime_id: RUNTIME_ID,
    agent_id: agentId,
    session_id: SESSION_ID,
    turn_id: TURN_ID,
    created_at: "2026-08-09T00:00:00Z",
    event: { type: "stream_ready" },
  }
}

function resetFrame(): AgentRuntimeStreamFrameV1 {
  return {
    protocol_version: 1,
    kind: "control",
    agent_runtime_id: RUNTIME_ID,
    agent_id: AGENT_ID,
    session_id: null,
    turn_id: null,
    created_at: "2026-08-09T00:00:00Z",
    event: { type: "stream_reset", reason: "buffer_overflow" },
  }
}

function durableFrame(eventSeq: string): DurableFrame {
  return {
    protocol_version: 1,
    kind: "durable",
    agent_runtime_id: RUNTIME_ID,
    agent_id: AGENT_ID,
    ...record(eventSeq, userMessage(`message-${eventSeq}`)),
  }
}

function telemetryFrame(): TelemetryFrame {
  return {
    protocol_version: 1,
    kind: "telemetry",
    agent_runtime_id: RUNTIME_ID,
    agent_id: AGENT_ID,
    session_id: SESSION_ID,
    turn_id: TURN_ID,
    created_at: "2026-08-09T00:00:00Z",
    durable_before_event_seq: "7",
    llm_call_id: "llm-call-1",
    telemetry_seq: "0",
    event: { type: "llm_started" },
  }
}

type Deferred<T> = {
  promise: Promise<T>
  resolve(value: T): void
  reject(error: unknown): void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

type SubscribeOptions = Parameters<typeof subscribeToAgentRuntimeEvents>[0]

function controlledSubscriptions() {
  const entries: { options: SubscribeOptions; done: Deferred<void> }[] = []
  const subscribe: typeof subscribeToAgentRuntimeEvents = (options) => {
    const done = deferred<void>()
    options.signal?.addEventListener("abort", () => done.resolve(undefined), {
      once: true,
    })
    entries.push({ options, done })
    return { done: done.promise }
  }
  return { entries, subscribe }
}

async function waitUntil(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (predicate()) return
    await new Promise((resolve) => setTimeout(resolve, 0))
  }
  throw new Error("condition was not reached")
}

describe("runConversationSession cold bootstrap", () => {
  it("subscribes before PG, discards cold telemetry, and commits cursor only after snapshot", async () => {
    const viewResult = deferred<AgentRuntimeView>()
    const history = {
      items: [record("6", userMessage("persisted"))],
      through_event_seq: "7",
      next_before_event_seq: "6",
      has_more: false,
    }
    const getAgentRuntime = vi.fn(() => viewResult.promise)
    const getAgentRuntimeHistory = vi.fn(() => Promise.resolve(history))
    const streams = controlledSubscriptions()
    const saved: string[] = []
    const actions: ConversationAction[] = []
    let state: ConversationState = initialConversationState
    const controller = new AbortController()

    const running = runConversationSession(
      {
        api: { getAgentRuntime, getAgentRuntimeHistory },
        subscribe: streams.subscribe,
        loadCursor: () => undefined,
        saveCursor: (_runtimeId, cursor) => saved.push(cursor),
        clearCursor: () => {},
        dispatch: (action) => {
          actions.push(action)
          state = conversationReducer(state, action)
        },
      },
      { agentRuntimeId: RUNTIME_ID, signal: controller.signal }
    )

    await waitUntil(() => streams.entries.length === 1)
    expect(getAgentRuntime).not.toHaveBeenCalled()
    streams.entries[0]?.options.onFrame(readyFrame(), null)
    await waitUntil(() => getAgentRuntime.mock.calls.length === 1)
    streams.entries[0]?.options.onFrame(durableFrame("8"), "durable-cursor")
    streams.entries[0]?.options.onFrame(telemetryFrame(), "telemetry-cursor")
    expect(saved).toEqual([])

    viewResult.resolve(runtimeView())
    await waitUntil(() => state.phase === "ready")

    expect(getAgentRuntimeHistory).toHaveBeenCalledWith(
      RUNTIME_ID,
      { throughSeq: "7", limit: 50 },
      expect.objectContaining({ signal: expect.any(AbortSignal) })
    )
    expect(saved).toEqual(["telemetry-cursor"])
    expect(state.pgConfirmedEventSeq).toBe("7")
    expect(Object.keys(state.unconfirmedDurableFrames)).toEqual(["8"])
    expect(
      actions.filter((action) => action.type === "telemetry_frame")
    ).toEqual([])

    controller.abort()
    await running
  })

  it.each(["EOF", "stream error"] as const)(
    "keeps PG recovery ready after an established live stream ends with %s",
    async (ending) => {
      const streams = controlledSubscriptions()
      const actions: ConversationAction[] = []
      const controller = new AbortController()
      let settled = false
      const running = runConversationSession(
        {
          api: {
            getAgentRuntime: vi.fn(() => Promise.resolve(runtimeView())),
            getAgentRuntimeHistory: vi.fn(() =>
              Promise.resolve({
                items: [],
                through_event_seq: "7",
                next_before_event_seq: null,
                has_more: false,
              })
            ),
          },
          subscribe: streams.subscribe,
          loadCursor: () => undefined,
          saveCursor: () => {},
          clearCursor: () => {},
          dispatch: (action) => actions.push(action),
        },
        { agentRuntimeId: RUNTIME_ID, signal: controller.signal }
      )
      void running.then(() => {
        settled = true
      })

      await waitUntil(() => streams.entries.length === 1)
      streams.entries[0]?.options.onFrame(readyFrame(), null)
      await waitUntil(
        () =>
          actions.filter((action) => action.type === "recovery_ready")
            .length === 1
      )

      if (ending === "EOF") streams.entries[0]?.done.resolve(undefined)
      else streams.entries[0]?.done.reject(new Error("stream disconnected"))

      await waitUntil(() =>
        actions.some(
          (action) => action.type === "realtime_degraded" && action.degraded
        )
      )
      expect(actions.slice(-2).map((action) => action.type)).toEqual([
        "realtime_degraded",
        "recovery_ready",
      ])
      expect(actions.some((action) => action.type === "connection_error")).toBe(
        false
      )
      expect(streams.entries).toHaveLength(1)
      expect(settled).toBe(false)

      controller.abort()
      await running
    }
  )

  it("does not save a buffered cursor when the PG snapshot fails", async () => {
    const streams = controlledSubscriptions()
    const saveCursor = vi.fn()
    const dispatch = vi.fn()
    const running = runConversationSession(
      {
        api: {
          getAgentRuntime: vi.fn(() => Promise.resolve(runtimeView())),
          getAgentRuntimeHistory: vi.fn(() =>
            Promise.reject(new ApiError("store_unavailable", 503, "down"))
          ),
        },
        subscribe: streams.subscribe,
        loadCursor: () => undefined,
        saveCursor,
        clearCursor: () => {},
        dispatch,
      },
      { agentRuntimeId: RUNTIME_ID, signal: new AbortController().signal }
    )

    await waitUntil(() => streams.entries.length === 1)
    streams.entries[0]?.options.onFrame(readyFrame(), null)
    streams.entries[0]?.options.onFrame(durableFrame("8"), "not-committed")
    await running

    expect(saveCursor).not.toHaveBeenCalled()
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ type: "connection_error" })
    )
  })

  it("treats EOF before stream_ready as a failed bootstrap without committing cursor", async () => {
    const streams = controlledSubscriptions()
    const saveCursor = vi.fn()
    const dispatch = vi.fn()
    const running = runConversationSession(
      {
        api: {
          getAgentRuntime: vi.fn(),
          getAgentRuntimeHistory: vi.fn(),
        },
        subscribe: streams.subscribe,
        loadCursor: () => undefined,
        saveCursor,
        clearCursor: () => {},
        dispatch,
      },
      { agentRuntimeId: RUNTIME_ID, signal: new AbortController().signal }
    )

    await waitUntil(() => streams.entries.length === 1)
    streams.entries[0]?.done.resolve(undefined)
    await running

    expect(saveCursor).not.toHaveBeenCalled()
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ type: "connection_error" })
    )
  })

  it("cold-retries once on dual-identity mismatch then fails closed", async () => {
    const streams = controlledSubscriptions()
    const dispatch = vi.fn()
    const clearCursor = vi.fn()
    const running = runConversationSession(
      {
        api: {
          getAgentRuntime: vi.fn(() => Promise.resolve(runtimeView())),
          getAgentRuntimeHistory: vi.fn(() =>
            Promise.resolve({
              items: [],
              through_event_seq: "7",
              next_before_event_seq: null,
              has_more: false,
            })
          ),
        },
        subscribe: streams.subscribe,
        loadCursor: () => "old-cursor",
        saveCursor: () => {},
        clearCursor,
        dispatch,
      },
      { agentRuntimeId: RUNTIME_ID, signal: new AbortController().signal }
    )

    // A resumed stream cannot be checked against a view yet in this process;
    // its pinned identity mismatch forces the no-cursor cold path.
    await waitUntil(() => streams.entries.length === 1)
    streams.entries[0]?.done.reject(
      new ApiError("protocol_identity_error", 0, "mismatch")
    )
    await waitUntil(() => streams.entries.length === 2)
    expect(streams.entries[1]?.options.afterCursor).toBeUndefined()
    streams.entries[1]?.options.onFrame(
      readyFrame("agent-definition-wrong"),
      null
    )
    await running

    expect(streams.entries).toHaveLength(2)
    expect(clearCursor).toHaveBeenCalledTimes(2)
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "connection_error",
        error: expect.objectContaining({ code: "protocol_identity_error" }),
      })
    )
  })

  it("drops the cursor and cold-bootstraps after a buffer reset", async () => {
    const streams = controlledSubscriptions()
    const clearCursor = vi.fn()
    const controller = new AbortController()
    const actions: ConversationAction[] = []
    const running = runConversationSession(
      {
        api: {
          getAgentRuntime: vi.fn(() => Promise.resolve(runtimeView())),
          getAgentRuntimeHistory: vi.fn(() =>
            Promise.resolve({
              items: [],
              through_event_seq: "7",
              next_before_event_seq: null,
              has_more: false,
            })
          ),
        },
        subscribe: streams.subscribe,
        loadCursor: () => undefined,
        saveCursor: () => {},
        clearCursor,
        dispatch: (action) => actions.push(action),
      },
      { agentRuntimeId: RUNTIME_ID, signal: controller.signal }
    )

    await waitUntil(() => streams.entries.length === 1)
    streams.entries[0]?.options.onFrame(resetFrame(), null)
    await waitUntil(() => streams.entries.length === 2)
    expect(streams.entries[1]?.options.afterCursor).toBeUndefined()
    streams.entries[1]?.options.onFrame(readyFrame(), null)
    await waitUntil(() =>
      actions.some((action) => action.type === "recovery_ready")
    )
    controller.abort()
    await running

    expect(clearCursor).toHaveBeenCalledWith(RUNTIME_ID)
  })

  it("falls back to a PG-only ready view when realtime is unavailable", async () => {
    const actions: ConversationAction[] = []
    const controller = new AbortController()
    const subscribe: typeof subscribeToAgentRuntimeEvents = () => ({
      done: Promise.reject(
        new ApiError("realtime_unavailable", 503, "nats unavailable")
      ),
    })
    const running = runConversationSession(
      {
        api: {
          getAgentRuntime: vi.fn(() => Promise.resolve(runtimeView())),
          getAgentRuntimeHistory: vi.fn(() =>
            Promise.resolve({
              items: [],
              through_event_seq: "7",
              next_before_event_seq: null,
              has_more: false,
            })
          ),
        },
        subscribe,
        loadCursor: () => undefined,
        saveCursor: () => {},
        clearCursor: () => {},
        dispatch: (action) => actions.push(action),
      },
      { agentRuntimeId: RUNTIME_ID, signal: controller.signal }
    )

    await waitUntil(() =>
      actions.some(
        (action) => action.type === "realtime_degraded" && action.degraded
      )
    )
    expect(actions.some((action) => action.type === "snapshot_loaded")).toBe(
      true
    )
    expect(actions.some((action) => action.type === "recovery_ready")).toBe(
      true
    )
    controller.abort()
    await running
  })

  it("treats a stalled subscription (no stream_ready) as realtime_unavailable after the ready deadline", async () => {
    const actions: ConversationAction[] = []
    const controller = new AbortController()
    const streams = controlledSubscriptions()
    const running = runConversationSession(
      {
        api: {
          getAgentRuntime: vi.fn(() => Promise.resolve(runtimeView())),
          getAgentRuntimeHistory: vi.fn(() =>
            Promise.resolve({
              items: [],
              through_event_seq: "7",
              next_before_event_seq: null,
              has_more: false,
            })
          ),
        },
        subscribe: streams.subscribe,
        loadCursor: () => undefined,
        saveCursor: () => {},
        clearCursor: () => {},
        dispatch: (action) => actions.push(action),
        streamReadyTimeoutMs: 20,
      },
      { agentRuntimeId: RUNTIME_ID, signal: controller.signal }
    )

    // 订阅永不定夺（NATS 挂起：连接保有但 stream_ready 不到达）；
    // deadline 后必须走 PG 快照降级，历史不被事件流阻塞
    await waitUntil(() =>
      actions.some((action) => action.type === "recovery_ready")
    )
    expect(actions.some((action) => action.type === "snapshot_loaded")).toBe(
      true
    )
    expect(
      actions.some(
        (action) => action.type === "realtime_degraded" && action.degraded
      )
    ).toBe(true)
    controller.abort()
    await running
  })
})

describe("reconcileConversation", () => {
  it("gives every fetch its own deadline and returns when a client ignores abort", async () => {
    vi.useFakeTimers()
    try {
      const viewSignals: AbortSignal[] = []
      const historySignals: AbortSignal[] = []
      const stalledHistory = deferred<AgentRuntimeHistoryPage>()
      const dispatch = vi.fn()
      let historyCall = 0

      const reconciling = reconcileConversation(
        {
          api: {
            getAgentRuntime: vi.fn((_agentRuntimeId, options) => {
              if (options?.signal !== undefined)
                viewSignals.push(options.signal)
              return new Promise<AgentRuntimeView>((resolve) => {
                setTimeout(
                  () => resolve(runtimeView({ snapshot_event_seq: "600" })),
                  29_000
                )
              })
            }),
            getAgentRuntimeHistory: vi.fn(
              (_agentRuntimeId, _query, options) => {
                if (options?.signal !== undefined)
                  historySignals.push(options.signal)
                historyCall += 1
                if (historyCall > 1) return stalledHistory.promise
                return new Promise<AgentRuntimeHistoryPage>((resolve) => {
                  setTimeout(
                    () =>
                      resolve({
                        items: [record("300", userMessage("300"))],
                        through_event_seq: "600",
                        next_before_event_seq: "300",
                        has_more: true,
                      }),
                    29_000
                  )
                })
              }
            ),
          },
          getPgConfirmedEventSeq: () => "100",
          getPinnedAgentId: () => AGENT_ID,
          isCurrent: () => true,
          dispatch,
        },
        { agentRuntimeId: RUNTIME_ID }
      )

      await vi.advanceTimersByTimeAsync(29_000)
      expect(viewSignals).toHaveLength(1)
      expect(viewSignals[0]?.aborted).toBe(false)
      expect(historySignals).toHaveLength(1)

      await vi.advanceTimersByTimeAsync(29_000)
      expect(historySignals).toHaveLength(2)
      expect(historySignals[0]?.aborted).toBe(false)
      expect(historySignals[1]?.aborted).toBe(false)

      await vi.advanceTimersByTimeAsync(29_999)
      expect(historySignals[1]?.aborted).toBe(false)
      expect(dispatch).not.toHaveBeenCalled()

      await vi.advanceTimersByTimeAsync(1)
      await reconciling
      expect(historySignals[1]?.aborted).toBe(true)
      expect(dispatch).not.toHaveBeenCalled()
      expect(vi.getTimerCount()).toBe(0)

      stalledHistory.reject(new Error("late history failure"))
      await Promise.resolve()
    } finally {
      vi.useRealTimers()
    }
  })

  it("propagates an outer abort through a stalled fetch and clears its deadline", async () => {
    vi.useFakeTimers()
    try {
      const controller = new AbortController()
      const stalledView = deferred<AgentRuntimeView>()
      const dispatch = vi.fn()
      let fetchSignal: AbortSignal | undefined

      const reconciling = reconcileConversation(
        {
          api: {
            getAgentRuntime: vi.fn((_agentRuntimeId, options) => {
              fetchSignal = options?.signal
              return stalledView.promise
            }),
            getAgentRuntimeHistory: vi.fn(() =>
              Promise.resolve({
                items: [],
                through_event_seq: "7",
                next_before_event_seq: null,
                has_more: false,
              })
            ),
          },
          getPgConfirmedEventSeq: () => "7",
          getPinnedAgentId: () => AGENT_ID,
          isCurrent: () => true,
          dispatch,
        },
        { agentRuntimeId: RUNTIME_ID, signal: controller.signal }
      )
      await Promise.resolve()
      expect(fetchSignal?.aborted).toBe(false)

      controller.abort()
      await reconciling

      expect(fetchSignal?.aborted).toBe(true)
      expect(dispatch).not.toHaveBeenCalled()
      expect(vi.getTimerCount()).toBe(0)

      stalledView.reject(new Error("late view failure"))
      await Promise.resolve()
    } finally {
      vi.useRealTimers()
    }
  })

  it("walks every older page needed for the complete public (B,T] window", async () => {
    const getAgentRuntimeHistory = vi
      .fn()
      .mockResolvedValueOnce({
        items: [
          record("300", userMessage("300")),
          record("500", userMessage("500")),
        ],
        through_event_seq: "600",
        next_before_event_seq: "300",
        has_more: true,
      })
      .mockResolvedValueOnce({
        items: [
          record("50", userMessage("50")),
          record("150", userMessage("150")),
        ],
        through_event_seq: "600",
        next_before_event_seq: "50",
        has_more: false,
      })
    const dispatch = vi.fn()

    await reconcileConversation(
      {
        api: {
          getAgentRuntime: vi.fn(() =>
            Promise.resolve(runtimeView({ snapshot_event_seq: "600" }))
          ),
          getAgentRuntimeHistory,
        },
        getPgConfirmedEventSeq: () => "100",
        getPinnedAgentId: () => AGENT_ID,
        isCurrent: () => true,
        dispatch,
      },
      { agentRuntimeId: RUNTIME_ID }
    )

    expect(getAgentRuntimeHistory).toHaveBeenNthCalledWith(
      1,
      RUNTIME_ID,
      { throughSeq: "600", beforeSeq: undefined, limit: 256 },
      { signal: expect.any(AbortSignal) }
    )
    expect(getAgentRuntimeHistory).toHaveBeenNthCalledWith(
      2,
      RUNTIME_ID,
      { throughSeq: "600", beforeSeq: "300", limit: 256 },
      { signal: expect.any(AbortSignal) }
    )
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "view_reconciled",
        basePgConfirmedEventSeq: "100",
        items: expect.arrayContaining([
          expect.objectContaining({ event_seq: "150" }),
          expect.objectContaining({ event_seq: "300" }),
          expect.objectContaining({ event_seq: "500" }),
        ]),
      })
    )
    const action = dispatch.mock.calls[0]?.[0] as Extract<
      ConversationAction,
      { type: "view_reconciled" }
    >
    expect(action.items.map((item) => item.event_seq)).toEqual([
      "150",
      "300",
      "500",
    ])
  })

  it("drops a result when another PG generation advances the base", async () => {
    const page = deferred<{
      items: AgentRuntimeDurableRecordV1[]
      through_event_seq: string
      next_before_event_seq: string | null
      has_more: boolean
    }>()
    let base = "7"
    const dispatch = vi.fn()
    const running = reconcileConversation(
      {
        api: {
          getAgentRuntime: vi.fn(() =>
            Promise.resolve(runtimeView({ snapshot_event_seq: "8" }))
          ),
          getAgentRuntimeHistory: vi.fn(() => page.promise),
        },
        getPgConfirmedEventSeq: () => base,
        getPinnedAgentId: () => AGENT_ID,
        isCurrent: () => true,
        dispatch,
      },
      { agentRuntimeId: RUNTIME_ID }
    )

    await Promise.resolve()
    base = "8"
    page.resolve({
      items: [record("8", userMessage("8"))],
      through_event_seq: "8",
      next_before_event_seq: "8",
      has_more: false,
    })
    await running

    expect(dispatch).not.toHaveBeenCalled()
  })

  it("fails closed when the PG view changes either identity", async () => {
    const dispatch = vi.fn()
    await reconcileConversation(
      {
        api: {
          getAgentRuntime: vi.fn(() =>
            Promise.resolve(runtimeView({ agent_id: "wrong-definition" }))
          ),
          getAgentRuntimeHistory: vi.fn(),
        },
        getPgConfirmedEventSeq: () => "7",
        getPinnedAgentId: () => AGENT_ID,
        isCurrent: () => true,
        dispatch,
      },
      { agentRuntimeId: RUNTIME_ID }
    )

    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "connection_error",
        error: expect.objectContaining({ code: "protocol_identity_error" }),
      })
    )
  })

  it("stops a malformed pagination loop that does not move backwards", async () => {
    const getAgentRuntimeHistory = vi
      .fn()
      .mockResolvedValueOnce({
        items: [record("300", userMessage("300"))],
        through_event_seq: "600",
        next_before_event_seq: "300",
        has_more: true,
      })
      .mockResolvedValueOnce({
        items: [record("300", userMessage("repeated"))],
        through_event_seq: "600",
        next_before_event_seq: "300",
        has_more: true,
      })
    const dispatch = vi.fn()

    await reconcileConversation(
      {
        api: {
          getAgentRuntime: vi.fn(() =>
            Promise.resolve(runtimeView({ snapshot_event_seq: "600" }))
          ),
          getAgentRuntimeHistory,
        },
        getPgConfirmedEventSeq: () => "100",
        getPinnedAgentId: () => AGENT_ID,
        isCurrent: () => true,
        dispatch,
      },
      { agentRuntimeId: RUNTIME_ID }
    )

    expect(getAgentRuntimeHistory).toHaveBeenCalledTimes(2)
    expect(dispatch).not.toHaveBeenCalled()
  })
})

describe("loadOlderHistoryPage", () => {
  it("keeps the original cold through barrier while moving only the older cursor", async () => {
    const dispatch = vi.fn()
    const getAgentRuntimeHistory = vi.fn(() =>
      Promise.resolve({
        items: [record("2", userMessage("older"))],
        through_event_seq: "7",
        next_before_event_seq: "2",
        has_more: false,
      })
    )
    await loadOlderHistoryPage(
      {
        api: { getAgentRuntimeHistory },
        getWindow: () => ({
          through: "7",
          before: "4",
          hasMore: true,
          loading: false,
        }),
        dispatch,
      },
      { agentRuntimeId: RUNTIME_ID }
    )

    expect(getAgentRuntimeHistory).toHaveBeenCalledWith(
      RUNTIME_ID,
      { throughSeq: "7", beforeSeq: "4", limit: 50 },
      { signal: undefined }
    )
    expect(dispatch.mock.calls.map(([action]) => action.type)).toEqual([
      "history_page_started",
      "history_page_loaded",
    ])
  })
})
