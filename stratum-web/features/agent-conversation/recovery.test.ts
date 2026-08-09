import { describe, expect, it, vi } from "vitest"

import {
  reconcileConversation,
  runConversationSession,
  type RecoveryDependencies,
} from "@/features/agent-conversation/recovery"
import type {
  ConversationAction,
  DurableFrame,
  TelemetryFrame,
} from "@/features/agent-conversation/types"
import { ApiError, type AgentView, type HistoryPage } from "@/lib/stratum/api"

const AGENT_ID = "agent-1"

type Deferred<T> = {
  promise: Promise<T>
  resolve(value: T): void
  reject(reason: unknown): void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise
    reject = rejectPromise
  })
  return { promise, resolve, reject }
}

async function eventually(predicate: () => boolean): Promise<void> {
  for (let attempt = 0; attempt < 100; attempt += 1) {
    if (predicate()) return
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
  }
  throw new Error("condition was not reached")
}

function view(snapshotEventSeq = "2"): AgentView {
  return {
    agent_id: AGENT_ID,
    agent_name: "default",
    status: "running",
    model_config: { model: "anthropic:claude-sonnet", parameters: {} },
    session_id: "session-1",
    current_turn_id: "turn-1",
    snapshot_event_seq: snapshotEventSeq,
    telemetry_floor_event_seq: "0",
    pending_approvals: [],
    latest_usage: null,
    resume_required: false,
  }
}

function page(throughEventSeq = "2"): HistoryPage {
  return {
    items: [
      {
        event_seq: throughEventSeq,
        event_version: 1,
        event: {
          type: "message_appended",
          data: {
            message: {
              role: "user",
              content: { type: "text", data: "committed" },
            },
          },
        },
      },
    ],
    through_event_seq: throughEventSeq,
    next_before_event_seq: null,
    has_more: false,
  }
}

function durableFrame(eventSeq: string): DurableFrame {
  return {
    protocol_version: 1,
    kind: "durable",
    agent_id: AGENT_ID,
    session_id: "session-1",
    turn_id: "turn-1",
    created_at: "2026-01-01T00:00:00.000Z",
    event_seq: eventSeq,
    event_version: 1,
    event: { type: "loop_started" },
  }
}

function telemetryFrame(): TelemetryFrame {
  return {
    protocol_version: 1,
    kind: "telemetry",
    agent_id: AGENT_ID,
    session_id: "session-1",
    turn_id: "turn-1",
    created_at: "2026-01-01T00:00:00.000Z",
    llm_call_id: "llm-call-1",
    telemetry_seq: 4,
    durable_before_event_seq: "10",
    event: { type: "text_delta", data: { delta: "partial" } },
  }
}

type SubscribeOptions = Parameters<RecoveryDependencies["subscribe"]>[0]

function rejectOnAbort(
  options: SubscribeOptions,
  streamDone: Deferred<void>
): void {
  const reject = () =>
    streamDone.reject(new DOMException("stream aborted", "AbortError"))
  if (options.signal?.aborted) reject()
  else options.signal?.addEventListener("abort", reject, { once: true })
}

describe("cold bootstrap", () => {
  it("falls back to a PG snapshot when realtime is unavailable before ready", async () => {
    const controller = new AbortController()
    const actions: ConversationAction[] = []

    const dependencies: RecoveryDependencies = {
      api: {
        getAgent: vi.fn(async () => view()),
        getHistory: vi.fn(async () => page()),
      },
      subscribe: () => ({
        done: Promise.reject(
          new ApiError("realtime_unavailable", 503, "realtime unavailable")
        ),
      }),
      loadCursor: () => undefined,
      saveCursor: vi.fn(),
      clearCursor: vi.fn(),
      dispatch: (action) => actions.push(action),
    }

    const running = runConversationSession(dependencies, {
      agentId: AGENT_ID,
      signal: controller.signal,
    })
    await eventually(() =>
      actions.some((action) => action.type === "recovery_ready")
    )

    expect(actions.map((action) => action.type)).toEqual([
      "recovery_started",
      "snapshot_loaded",
      "realtime_degraded",
      "recovery_ready",
    ])
    expect(dependencies.saveCursor).not.toHaveBeenCalled()

    controller.abort()
    await running
  })

  it("buffers after stream_ready, drops bootstrap telemetry, then commits cursor", async () => {
    const controller = new AbortController()
    const history = deferred<HistoryPage>()
    const streamDone = deferred<void>()
    const actions: ConversationAction[] = []
    const trace: string[] = []
    let subscriptionOptions: SubscribeOptions | undefined

    const dependencies: RecoveryDependencies = {
      api: {
        getAgent: vi.fn(async () => view()),
        getHistory: vi.fn(() => history.promise),
      },
      subscribe: (options) => {
        subscriptionOptions = options
        rejectOnAbort(options, streamDone)
        return { done: streamDone.promise }
      },
      loadCursor: () => undefined,
      saveCursor: (_agentId, cursor) => trace.push(`cursor:${cursor}`),
      clearCursor: vi.fn(),
      dispatch: (action) => {
        actions.push(action)
        trace.push(`dispatch:${action.type}`)
      },
    }

    const running = runConversationSession(dependencies, {
      agentId: AGENT_ID,
      signal: controller.signal,
    })
    await eventually(() => subscriptionOptions !== undefined)
    subscriptionOptions?.onFrame(
      {
        protocol_version: 1,
        kind: "control",
        agent_id: AGENT_ID,
        session_id: "session-1",
        turn_id: "turn-1",
        created_at: "2026-01-01T00:00:00.000Z",
        event: { type: "stream_ready" },
      },
      null
    )
    await eventually(
      () => vi.mocked(dependencies.api.getHistory).mock.calls.length === 1
    )

    subscriptionOptions?.onFrame(durableFrame("3"), "cursor-3")
    subscriptionOptions?.onFrame(telemetryFrame(), "cursor-4")
    history.resolve(page())
    await eventually(() =>
      actions.some((action) => action.type === "recovery_ready")
    )

    expect(actions.map((action) => action.type)).toEqual([
      "recovery_started",
      "snapshot_loaded",
      "durable_frame",
      "realtime_degraded",
      "recovery_ready",
    ])
    expect(trace.indexOf("dispatch:snapshot_loaded")).toBeLessThan(
      trace.indexOf("dispatch:durable_frame")
    )
    expect(trace.indexOf("dispatch:durable_frame")).toBeLessThan(
      trace.indexOf("cursor:cursor-4")
    )
    expect(actions.some((action) => action.type === "telemetry_frame")).toBe(
      false
    )

    controller.abort()
    await running
  })

  it("does not commit snapshot, buffer, or cursor after stream_reset", async () => {
    const controller = new AbortController()
    const history = deferred<HistoryPage>()
    const actions: ConversationAction[] = []
    const savedCursors: string[] = []
    const streamOptions: SubscribeOptions[] = []
    const streamPromises: Deferred<void>[] = []

    const dependencies: RecoveryDependencies = {
      api: {
        getAgent: vi.fn(async () => view()),
        getHistory: vi.fn(() => history.promise),
      },
      subscribe: (options) => {
        streamOptions.push(options)
        const done = deferred<void>()
        streamPromises.push(done)
        rejectOnAbort(options, done)
        return { done: done.promise }
      },
      loadCursor: () => undefined,
      saveCursor: (_agentId, cursor) => savedCursors.push(cursor),
      clearCursor: vi.fn(),
      dispatch: (action) => actions.push(action),
    }

    const running = runConversationSession(dependencies, {
      agentId: AGENT_ID,
      signal: controller.signal,
    })
    await eventually(() => streamOptions.length === 1)
    streamOptions[0]?.onFrame(
      {
        protocol_version: 1,
        kind: "control",
        agent_id: AGENT_ID,
        session_id: "session-1",
        turn_id: "turn-1",
        created_at: "2026-01-01T00:00:00.000Z",
        event: { type: "stream_ready" },
      },
      null
    )
    await eventually(
      () => vi.mocked(dependencies.api.getHistory).mock.calls.length === 1
    )
    streamOptions[0]?.onFrame(durableFrame("3"), "cursor-3")
    streamOptions[0]?.onFrame(
      {
        protocol_version: 1,
        kind: "control",
        agent_id: AGENT_ID,
        session_id: null,
        turn_id: null,
        created_at: "2026-01-01T00:00:00.000Z",
        event: { type: "stream_reset", reason: "buffer_overflow" },
      },
      null
    )
    history.resolve(page())

    // reset 必须启动一个全新的无 cursor cold bootstrap。
    await eventually(() => streamOptions.length === 2)
    expect(savedCursors).toEqual([])
    expect(actions.some((action) => action.type === "snapshot_loaded")).toBe(
      false
    )
    expect(actions.some((action) => action.type === "durable_frame")).toBe(
      false
    )
    expect(vi.mocked(dependencies.clearCursor)).toHaveBeenCalledWith(AGENT_ID)

    controller.abort()
    await running
    // 两条 subscription 的 rejection 都被恢复循环消费，测试结束时无悬挂流。
    expect(streamPromises).toHaveLength(2)
  })

  it("aborts an in-flight PG snapshot so stream_reset can cold-bootstrap immediately", async () => {
    const controller = new AbortController()
    const streamOptions: SubscribeOptions[] = []
    const streamPromises: Deferred<void>[] = []
    let snapshotSignal: AbortSignal | undefined

    const dependencies: RecoveryDependencies = {
      api: {
        getAgent: vi.fn(async () => view()),
        getHistory: vi.fn((_agentId, _query, options) => {
          snapshotSignal = options?.signal
          return new Promise<HistoryPage>((_resolve, reject) => {
            const abort = () =>
              reject(new DOMException("snapshot aborted", "AbortError"))
            if (snapshotSignal?.aborted) abort()
            else
              snapshotSignal?.addEventListener("abort", abort, { once: true })
          })
        }),
      },
      subscribe: (options) => {
        streamOptions.push(options)
        const done = deferred<void>()
        streamPromises.push(done)
        rejectOnAbort(options, done)
        return { done: done.promise }
      },
      loadCursor: () => undefined,
      saveCursor: vi.fn(),
      clearCursor: vi.fn(),
      dispatch: vi.fn(),
    }

    const running = runConversationSession(dependencies, {
      agentId: AGENT_ID,
      signal: controller.signal,
    })
    await eventually(() => streamOptions.length === 1)
    streamOptions[0]?.onFrame(
      {
        protocol_version: 1,
        kind: "control",
        agent_id: AGENT_ID,
        session_id: "session-1",
        turn_id: "turn-1",
        created_at: "2026-01-01T00:00:00.000Z",
        event: { type: "stream_ready" },
      },
      null
    )
    await eventually(() => snapshotSignal !== undefined)

    streamOptions[0]?.onFrame(
      {
        protocol_version: 1,
        kind: "control",
        agent_id: AGENT_ID,
        session_id: null,
        turn_id: null,
        created_at: "2026-01-01T00:00:00.000Z",
        event: { type: "stream_reset", reason: "buffer_overflow" },
      },
      null
    )

    await eventually(() => streamOptions.length === 2)
    expect(snapshotSignal?.aborted).toBe(true)
    expect(dependencies.clearCursor).toHaveBeenCalledWith(AGENT_ID)

    controller.abort()
    await running
  })

  it("does not commit a snapshot after the ready stream ends", async () => {
    const controller = new AbortController()
    const streamDone = deferred<void>()
    const actions: ConversationAction[] = []
    let subscriptionOptions: SubscribeOptions | undefined
    let snapshotSignal: AbortSignal | undefined

    const dependencies: RecoveryDependencies = {
      api: {
        getAgent: vi.fn(async () => view()),
        getHistory: vi.fn((_agentId, _query, options) => {
          snapshotSignal = options?.signal
          return new Promise<HistoryPage>((_resolve, reject) => {
            const abort = () =>
              reject(new DOMException("snapshot aborted", "AbortError"))
            if (snapshotSignal?.aborted) abort()
            else
              snapshotSignal?.addEventListener("abort", abort, { once: true })
          })
        }),
      },
      subscribe: (options) => {
        subscriptionOptions = options
        return { done: streamDone.promise }
      },
      loadCursor: () => undefined,
      saveCursor: vi.fn(),
      clearCursor: vi.fn(),
      dispatch: (action) => actions.push(action),
    }

    const running = runConversationSession(dependencies, {
      agentId: AGENT_ID,
      signal: controller.signal,
    })
    await eventually(() => subscriptionOptions !== undefined)
    subscriptionOptions?.onFrame(
      {
        protocol_version: 1,
        kind: "control",
        agent_id: AGENT_ID,
        session_id: "session-1",
        turn_id: "turn-1",
        created_at: "2026-01-01T00:00:00.000Z",
        event: { type: "stream_ready" },
      },
      null
    )
    await eventually(() => snapshotSignal !== undefined)

    streamDone.resolve()
    await running

    expect(snapshotSignal?.aborted).toBe(true)
    expect(actions.some((action) => action.type === "snapshot_loaded")).toBe(
      false
    )
    expect(actions.at(-1)?.type).toBe("connection_error")
    expect(dependencies.saveCursor).not.toHaveBeenCalled()
  })

  it("bounds the browser cold buffer while a PG snapshot is slow", async () => {
    const controller = new AbortController()
    const streamOptions: SubscribeOptions[] = []
    let snapshotSignal: AbortSignal | undefined

    const dependencies: RecoveryDependencies = {
      api: {
        getAgent: vi.fn(async () => view()),
        getHistory: vi.fn((_agentId, _query, options) => {
          snapshotSignal = options?.signal
          return new Promise<HistoryPage>((_resolve, reject) => {
            const abort = () =>
              reject(new DOMException("snapshot aborted", "AbortError"))
            if (snapshotSignal?.aborted) abort()
            else
              snapshotSignal?.addEventListener("abort", abort, { once: true })
          })
        }),
      },
      subscribe: (options) => {
        streamOptions.push(options)
        const done = deferred<void>()
        rejectOnAbort(options, done)
        return { done: done.promise }
      },
      loadCursor: () => undefined,
      saveCursor: vi.fn(),
      clearCursor: vi.fn(),
      dispatch: vi.fn(),
    }

    const running = runConversationSession(dependencies, {
      agentId: AGENT_ID,
      signal: controller.signal,
    })
    await eventually(() => streamOptions.length === 1)
    streamOptions[0]?.onFrame(
      {
        protocol_version: 1,
        kind: "control",
        agent_id: AGENT_ID,
        session_id: "session-1",
        turn_id: "turn-1",
        created_at: "2026-01-01T00:00:00.000Z",
        event: { type: "stream_ready" },
      },
      null
    )
    await eventually(() => snapshotSignal !== undefined)

    for (let index = 0; index <= 256; index += 1)
      streamOptions[0]?.onFrame(
        durableFrame(String(index + 3)),
        `cursor-${index}`
      )

    await eventually(() => streamOptions.length === 2)
    expect(snapshotSignal?.aborted).toBe(true)
    expect(dependencies.saveCursor).not.toHaveBeenCalled()

    controller.abort()
    await running
  })

  it("degrades to PG reconcile when an established live tail returns 503", async () => {
    const controller = new AbortController()
    const streamDone = deferred<void>()
    const actions: ConversationAction[] = []
    let subscriptionOptions: SubscribeOptions | undefined

    const dependencies: RecoveryDependencies = {
      api: {
        getAgent: vi.fn(async () => view()),
        getHistory: vi.fn(async () => page()),
      },
      subscribe: (options) => {
        subscriptionOptions = options
        return { done: streamDone.promise }
      },
      loadCursor: () => undefined,
      saveCursor: vi.fn(),
      clearCursor: vi.fn(),
      dispatch: (action) => actions.push(action),
    }

    const running = runConversationSession(dependencies, {
      agentId: AGENT_ID,
      signal: controller.signal,
    })
    await eventually(() => subscriptionOptions !== undefined)
    subscriptionOptions?.onFrame(
      {
        protocol_version: 1,
        kind: "control",
        agent_id: AGENT_ID,
        session_id: "session-1",
        turn_id: "turn-1",
        created_at: "2026-01-01T00:00:00.000Z",
        event: { type: "stream_ready" },
      },
      null
    )
    await eventually(() =>
      actions.some((action) => action.type === "recovery_ready")
    )

    streamDone.reject(
      new ApiError("realtime_unavailable", 503, "realtime unavailable")
    )
    await eventually(() => {
      const lastDegraded = actions
        .filter((action) => action.type === "realtime_degraded")
        .at(-1)
      return lastDegraded?.type === "realtime_degraded" && lastDegraded.degraded
    })

    expect(actions.some((action) => action.type === "connection_error")).toBe(
      false
    )
    controller.abort()
    await running
  })
})

describe("incremental reconcile", () => {
  it("gives every fetch its own deadline and returns when a client ignores abort", async () => {
    vi.useFakeTimers()
    try {
      const agentSignals: AbortSignal[] = []
      const historySignals: AbortSignal[] = []
      const stalledHistory = deferred<HistoryPage>()
      const dispatch = vi.fn<(action: ConversationAction) => void>()
      let historyCall = 0

      const reconciling = reconcileConversation(
        {
          api: {
            getAgent: vi.fn((_agentId, options) => {
              if (options?.signal !== undefined)
                agentSignals.push(options.signal)
              return new Promise<AgentView>((resolve) => {
                setTimeout(() => resolve(view("12")), 29_000)
              })
            }),
            getHistory: vi.fn((_agentId, _query, options) => {
              if (options?.signal !== undefined)
                historySignals.push(options.signal)
              historyCall += 1
              if (historyCall > 1) return stalledHistory.promise
              return new Promise<HistoryPage>((resolve) => {
                setTimeout(
                  () =>
                    resolve({
                      ...page("12"),
                      next_before_event_seq: "11",
                      has_more: true,
                    }),
                  29_000
                )
              })
            }),
          },
          getBarrier: () => "10",
          isCurrent: () => true,
          dispatch,
        },
        { agentId: AGENT_ID }
      )

      await vi.advanceTimersByTimeAsync(29_000)
      expect(agentSignals).toHaveLength(1)
      expect(agentSignals[0]?.aborted).toBe(false)
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

      // Promise.race must keep observing the abandoned request after the
      // deadline; a late client failure must not surface as an unhandled one.
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
      const stalledView = deferred<AgentView>()
      const dispatch = vi.fn<(action: ConversationAction) => void>()
      let fetchSignal: AbortSignal | undefined

      const reconciling = reconcileConversation(
        {
          api: {
            getAgent: vi.fn((_agentId, options) => {
              fetchSignal = options?.signal
              return stalledView.promise
            }),
            getHistory: vi.fn(async () => page()),
          },
          getBarrier: () => "10",
          isCurrent: () => true,
          dispatch,
        },
        { agentId: AGENT_ID, signal: controller.signal }
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

  it("drops a result when realtime advances the captured base barrier", async () => {
    let barrier = "10"
    const history = deferred<HistoryPage>()
    const dispatch = vi.fn<(action: ConversationAction) => void>()
    const getHistory = vi.fn(() => history.promise)

    const reconciling = reconcileConversation(
      {
        api: {
          getAgent: vi.fn(async () => view("12")),
          getHistory,
        },
        getBarrier: () => barrier,
        isCurrent: () => true,
        dispatch,
      },
      { agentId: AGENT_ID }
    )
    await eventually(() => getHistory.mock.calls.length === 1)

    barrier = "11"
    history.resolve(page("12"))
    await reconciling

    expect(dispatch).not.toHaveBeenCalled()
  })

  it("labels a fresh bundle with the exact base barrier", async () => {
    const dispatch = vi.fn<(action: ConversationAction) => void>()

    await reconcileConversation(
      {
        api: {
          getAgent: vi.fn(async () => view("12")),
          getHistory: vi.fn(async () => page("12")),
        },
        getBarrier: () => "10",
        isCurrent: () => true,
        dispatch,
      },
      { agentId: AGENT_ID }
    )

    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "view_reconciled",
        baseBarrier: "10",
        view: expect.objectContaining({ snapshot_event_seq: "12" }),
      })
    )
  })

  it("lets only the latest request update equal-barrier advisory fields", async () => {
    let latestRequest = 1
    const firstView = deferred<AgentView>()
    const dispatch = vi.fn<(action: ConversationAction) => void>()

    const first = reconcileConversation(
      {
        api: {
          getAgent: vi.fn(() => firstView.promise),
          getHistory: vi.fn(async () => page("10")),
        },
        getBarrier: () => "10",
        isCurrent: () => latestRequest === 1,
        dispatch,
      },
      { agentId: AGENT_ID }
    )
    latestRequest = 2
    await reconcileConversation(
      {
        api: {
          getAgent: vi.fn(async () => ({
            ...view("10"),
            resume_required: false,
          })),
          getHistory: vi.fn(async () => page("10")),
        },
        getBarrier: () => "10",
        isCurrent: () => latestRequest === 2,
        dispatch,
      },
      { agentId: AGENT_ID }
    )
    firstView.resolve({ ...view("10"), resume_required: true })
    await first

    expect(dispatch).toHaveBeenCalledTimes(1)
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({
        type: "view_reconciled",
        view: expect.objectContaining({ resume_required: false }),
      })
    )
  })

  it("does not let an older 404 override a newer equal-barrier result", async () => {
    let latestRequest = 1
    const firstView = deferred<AgentView>()
    const dispatch = vi.fn<(action: ConversationAction) => void>()

    const first = reconcileConversation(
      {
        api: {
          getAgent: vi.fn(() => firstView.promise),
          getHistory: vi.fn(async () => page("10")),
        },
        getBarrier: () => "10",
        isCurrent: () => latestRequest === 1,
        dispatch,
      },
      { agentId: AGENT_ID }
    )
    latestRequest = 2
    await reconcileConversation(
      {
        api: {
          getAgent: vi.fn(async () => view("10")),
          getHistory: vi.fn(async () => page("10")),
        },
        getBarrier: () => "10",
        isCurrent: () => latestRequest === 2,
        dispatch,
      },
      { agentId: AGENT_ID }
    )
    firstView.reject(new ApiError("agent_not_found", 404, "agent not found"))
    await first

    expect(dispatch).toHaveBeenCalledTimes(1)
    expect(dispatch).toHaveBeenCalledWith(
      expect.objectContaining({ type: "view_reconciled" })
    )
  })
})
