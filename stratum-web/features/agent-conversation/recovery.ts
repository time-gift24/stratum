import type {
  ConversationAction,
  DurableFrame,
} from "@/features/agent-conversation/types"
import {
  ApiError,
  compareEventSeq,
  type AgentRuntimeDurableRecordV1,
  type AgentRuntimeHistoryPage,
  type AgentRuntimeView,
  type StratumApi,
} from "@/lib/stratum/api"
import { subscribeToAgentRuntimeEvents } from "@/lib/stratum/event-stream"

/** Subscribe-before-snapshot recovery and PG reconciliation. */

const HISTORY_PAGE_LIMIT = 50
const RECONCILE_PAGE_LIMIT = 256
const RECONCILE_FETCH_TIMEOUT_MS = 30_000
const COLD_BUFFER_MAX_FRAMES = 256
const COLD_BUFFER_MAX_CHARS = 4 * 1024 * 1024

const STREAM_RESET_CODE = "stream_reset"
const PROTOCOL_IDENTITY_CODE = "protocol_identity_error"

export type RecoveryDependencies = {
  api: Pick<StratumApi, "getAgentRuntime" | "getAgentRuntimeHistory">
  subscribe: typeof subscribeToAgentRuntimeEvents
  loadCursor(agentRuntimeId: string): string | undefined
  saveCursor(agentRuntimeId: string, cursor: string): void
  clearCursor(agentRuntimeId: string): void
  dispatch(action: ConversationAction): void
}

type SessionState = {
  cursor: string | undefined
  agentId: string | undefined
  /** One no-cursor retry is allowed after a dual-identity failure. */
  identityRecoveryAttempts: number
}

export async function runConversationSession(
  dependencies: RecoveryDependencies,
  input: { agentRuntimeId: string; signal: AbortSignal }
): Promise<void> {
  const session: SessionState = {
    cursor: dependencies.loadCursor(input.agentRuntimeId),
    agentId: undefined,
    identityRecoveryAttempts: 0,
  }

  while (!input.signal.aborted) {
    try {
      if (session.cursor === undefined)
        await coldBootstrap(dependencies, input, session)
      else await resumeTail(dependencies, input, session)
    } catch (error) {
      if (input.signal.aborted) return
      if (isApiErrorCode(error, PROTOCOL_IDENTITY_CODE)) {
        session.cursor = undefined
        session.agentId = undefined
        dependencies.clearCursor(input.agentRuntimeId)
        session.identityRecoveryAttempts += 1
        if (session.identityRecoveryAttempts <= 1) continue
        dependencies.dispatch({
          type: "connection_error",
          error: protocolIdentityError(),
        })
        return
      }
      if (
        isApiErrorCode(error, STREAM_RESET_CODE) ||
        isApiErrorCode(error, "cursor_expired")
      ) {
        session.cursor = undefined
        session.agentId = undefined
        session.identityRecoveryAttempts = 0
        dependencies.clearCursor(input.agentRuntimeId)
        continue
      }
      if (error instanceof ApiError && error.status === 404) {
        dependencies.dispatch({ type: "missing", error })
        return
      }
      if (
        isApiErrorCode(error, "realtime_unavailable") ||
        session.agentId !== undefined
      ) {
        dependencies.dispatch({ type: "realtime_degraded", degraded: true })
        dependencies.dispatch({ type: "recovery_ready" })
        await abortDriven(input.signal)
        return
      }
      dependencies.dispatch({
        type: "connection_error",
        error: toConnectionError(error),
      })
      return
    }
  }
}

async function coldBootstrap(
  dependencies: RecoveryDependencies,
  input: { agentRuntimeId: string; signal: AbortSignal },
  session: SessionState
): Promise<void> {
  const { agentRuntimeId, signal } = input
  dependencies.dispatch({ type: "recovery_started", agentRuntimeId })

  const buffered: DurableFrame[] = []
  let bufferedChars = 0
  let live = false
  let sawReset = false
  let latestCursor: string | null = null
  let streamAgentId: string | undefined

  const streamControl = new AbortController()
  const unlink = linkAbort(signal, streamControl)

  let readyResolve!: () => void
  let readyReject!: (error: unknown) => void
  const ready = new Promise<void>((resolve, reject) => {
    readyResolve = resolve
    readyReject = reject
  })

  const subscription = dependencies.subscribe({
    baseUrl: "",
    agentRuntimeId,
    signal: streamControl.signal,
    onFrame: (frame, frameCursor) => {
      assertFrameIdentity(frame, agentRuntimeId, streamAgentId)
      streamAgentId ??= frame.agent_id

      if (frame.kind === "control") {
        if (frame.event.type === "stream_reset") {
          sawReset = true
          streamControl.abort()
          readyReject(new ApiError(STREAM_RESET_CODE, 0, "stream reset"))
        } else readyResolve()
        return
      }

      if (frameCursor !== null) latestCursor = frameCursor
      if (!live) {
        // A cold telemetry buffer can never prove a complete call prefix.
        if (frame.kind === "durable") {
          const frameChars = JSON.stringify(frame).length
          if (
            buffered.length >= COLD_BUFFER_MAX_FRAMES ||
            bufferedChars + frameChars > COLD_BUFFER_MAX_CHARS
          ) {
            sawReset = true
            streamControl.abort()
            readyReject(new ApiError(STREAM_RESET_CODE, 0, "stream reset"))
            return
          }
          buffered.push(frame)
          bufferedChars += frameChars
        }
        return
      }

      // Live application has a PG-established pinned definition fence.
      assertFrameIdentity(frame, agentRuntimeId, session.agentId)
      if (frameCursor !== null) {
        session.cursor = frameCursor
        dependencies.saveCursor(agentRuntimeId, frameCursor)
      }
      dependencies.dispatch(
        frame.kind === "durable"
          ? { type: "durable_frame", frame }
          : { type: "telemetry_frame", frame }
      )
    },
  })

  const streamEnded: Promise<never> = subscription.done.then(
    () => {
      if (!live) streamControl.abort()
      throw new ApiError(
        "stream_closed",
        0,
        "event stream closed during recovery"
      )
    },
    (error: unknown) => {
      if (!live) streamControl.abort()
      throw error
    }
  )
  void streamEnded.catch((error: unknown) => {
    if (!live && !sawReset) readyReject(error)
  })

  try {
    await ready
  } catch (error) {
    streamControl.abort()
    unlink()
    if (isApiErrorCode(error, "realtime_unavailable")) {
      const snapshot = await readSnapshot(dependencies, input)
      session.agentId = snapshot.view.agent_id
      session.identityRecoveryAttempts = 0
      dependencies.dispatch(snapshot)
      dependencies.dispatch({ type: "realtime_degraded", degraded: true })
      dependencies.dispatch({ type: "recovery_ready" })
      return abortDriven(signal)
    }
    if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
    throw error
  }

  try {
    const snapshot = await Promise.race([
      readSnapshot(dependencies, {
        agentRuntimeId,
        signal: streamControl.signal,
        expectedAgentId: streamAgentId,
      }),
      streamEnded,
    ])
    throwIfAborted(signal)
    if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")

    session.agentId = snapshot.view.agent_id
    for (const frame of buffered)
      assertFrameIdentity(frame, agentRuntimeId, session.agentId)

    dependencies.dispatch(snapshot)
    for (const frame of buffered)
      dependencies.dispatch({ type: "durable_frame", frame })

    if (latestCursor !== null) {
      session.cursor = latestCursor
      dependencies.saveCursor(agentRuntimeId, latestCursor)
    }
    session.identityRecoveryAttempts = 0
    live = true
    dependencies.dispatch({ type: "realtime_degraded", degraded: false })
    dependencies.dispatch({ type: "recovery_ready" })
  } catch (error) {
    streamControl.abort()
    unlink()
    if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
    throw error
  }

  return streamEnded
    .catch((error: unknown) => {
      if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
      throw error
    })
    .finally(unlink)
}

function resumeTail(
  dependencies: RecoveryDependencies,
  input: { agentRuntimeId: string; signal: AbortSignal },
  session: SessionState
): Promise<void> {
  const { agentRuntimeId, signal } = input
  let sawReset = false
  const streamControl = new AbortController()
  const unlink = linkAbort(signal, streamControl)

  const subscription = dependencies.subscribe({
    baseUrl: "",
    agentRuntimeId,
    afterCursor: session.cursor,
    signal: streamControl.signal,
    onFrame: (frame, frameCursor) => {
      assertFrameIdentity(frame, agentRuntimeId, session.agentId)
      if (frame.kind === "control") {
        if (frame.event.type === "stream_reset") {
          sawReset = true
          streamControl.abort()
        }
        return
      }
      if (frameCursor !== null) {
        session.cursor = frameCursor
        dependencies.saveCursor(agentRuntimeId, frameCursor)
      }
      dependencies.dispatch(
        frame.kind === "durable"
          ? { type: "durable_frame", frame }
          : { type: "telemetry_frame", frame }
      )
    },
  })

  return subscription.done
    .catch((error: unknown) => {
      if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
      throw error
    })
    .finally(unlink)
}

async function readSnapshot(
  dependencies: RecoveryDependencies,
  input: {
    agentRuntimeId: string
    signal: AbortSignal
    expectedAgentId?: string
  }
): Promise<Extract<ConversationAction, { type: "snapshot_loaded" }>> {
  const view = await dependencies.api.getAgentRuntime(input.agentRuntimeId, {
    signal: input.signal,
  })
  throwIfAborted(input.signal)
  assertViewIdentity(view, input.agentRuntimeId, input.expectedAgentId)

  const page = await dependencies.api.getAgentRuntimeHistory(
    input.agentRuntimeId,
    { throughSeq: view.snapshot_event_seq, limit: HISTORY_PAGE_LIMIT },
    { signal: input.signal }
  )
  throwIfAborted(input.signal)
  assertHistoryPage(page, view.snapshot_event_seq)
  return {
    type: "snapshot_loaded",
    view,
    items: page.items,
    historyBefore: page.next_before_event_seq,
    historyHasMore: page.has_more,
  }
}

export type ReconcileDependencies = {
  api: Pick<StratumApi, "getAgentRuntime" | "getAgentRuntimeHistory">
  getPgConfirmedEventSeq(): string | null
  getPinnedAgentId(): string | null
  isCurrent(): boolean
  dispatch(action: ConversationAction): void
}

/** Read the complete public product window `(B,T]` and atomically rebase. */
export async function reconcileConversation(
  dependencies: ReconcileDependencies,
  input: { agentRuntimeId: string; signal?: AbortSignal }
): Promise<void> {
  const base = dependencies.getPgConfirmedEventSeq()
  const pinnedAgentId = dependencies.getPinnedAgentId()
  if (base === null || pinnedAgentId === null || !dependencies.isCurrent())
    return

  let view: AgentRuntimeView
  try {
    view = await withReconcileFetchDeadline(input.signal, (signal) =>
      dependencies.api.getAgentRuntime(input.agentRuntimeId, { signal })
    )
  } catch (error) {
    if (
      error instanceof ApiError &&
      error.status === 404 &&
      dependencies.isCurrent() &&
      dependencies.getPgConfirmedEventSeq() === base
    )
      dependencies.dispatch({ type: "missing", error })
    return
  }
  if (!dependencies.isCurrent()) return
  try {
    assertViewIdentity(view, input.agentRuntimeId, pinnedAgentId)
  } catch (error) {
    dependencies.dispatch({
      type: "connection_error",
      error: toConnectionError(error),
    })
    return
  }
  if (dependencies.getPgConfirmedEventSeq() !== base) return

  const items: AgentRuntimeDurableRecordV1[] = []
  if (compareEventSeq(view.snapshot_event_seq, base) > 0) {
    let before: string | undefined
    try {
      for (;;) {
        const page = await withReconcileFetchDeadline(input.signal, (signal) =>
          dependencies.api.getAgentRuntimeHistory(
            input.agentRuntimeId,
            {
              throughSeq: view.snapshot_event_seq,
              beforeSeq: before,
              limit: RECONCILE_PAGE_LIMIT,
            },
            { signal }
          )
        )
        if (!dependencies.isCurrent()) return
        assertHistoryPage(page, view.snapshot_event_seq, before)

        const fresh = page.items.filter(
          (item) => compareEventSeq(item.event_seq, base) > 0
        )
        items.unshift(...fresh)
        const oldest = page.items[0]
        if (
          !page.has_more ||
          oldest === undefined ||
          compareEventSeq(oldest.event_seq, base) <= 0 ||
          page.next_before_event_seq === null
        )
          break
        before = page.next_before_event_seq
      }
    } catch {
      return
    }
  }

  if (
    !dependencies.isCurrent() ||
    dependencies.getPgConfirmedEventSeq() !== base
  )
    return
  dependencies.dispatch({
    type: "view_reconciled",
    basePgConfirmedEventSeq: base,
    view,
    items,
  })
}

async function withReconcileFetchDeadline<T>(
  outerSignal: AbortSignal | undefined,
  operation: (signal: AbortSignal) => Promise<T>
): Promise<T> {
  if (outerSignal?.aborted)
    throw abortReason(outerSignal, "reconcile fetch aborted")

  const controller = new AbortController()
  const onOuterAbort = () => controller.abort(outerSignal?.reason)
  outerSignal?.addEventListener("abort", onOuterAbort, { once: true })
  const timeout = setTimeout(
    () =>
      controller.abort(
        new DOMException("reconcile fetch timed out", "AbortError")
      ),
    RECONCILE_FETCH_TIMEOUT_MS
  )

  let rejectAbort!: (error: unknown) => void
  const aborted = new Promise<never>((_resolve, reject) => {
    rejectAbort = reject
  })
  const onAbort = () =>
    rejectAbort(abortReason(controller.signal, "reconcile fetch aborted"))
  controller.signal.addEventListener("abort", onAbort, { once: true })

  try {
    // The losing request remains observed by Promise.race. A client that
    // ignores abort can therefore reject later without becoming unhandled.
    const result = Promise.resolve().then(() => operation(controller.signal))
    return await Promise.race([result, aborted])
  } finally {
    clearTimeout(timeout)
    controller.signal.removeEventListener("abort", onAbort)
    outerSignal?.removeEventListener("abort", onOuterAbort)
  }
}

function abortReason(signal: AbortSignal, fallbackMessage: string): Error {
  return signal.reason instanceof Error
    ? signal.reason
    : new DOMException(fallbackMessage, "AbortError")
}

export type HistoryWindow = {
  through: string
  before: string | null
  hasMore: boolean
  loading: boolean
}

export type HistoryDependencies = {
  api: Pick<StratumApi, "getAgentRuntimeHistory">
  getWindow(): HistoryWindow | null
  dispatch(action: ConversationAction): void
}

/** User-driven upward pagination at one fixed cold-bootstrap barrier. */
export async function loadOlderHistoryPage(
  dependencies: HistoryDependencies,
  input: { agentRuntimeId: string; signal?: AbortSignal }
): Promise<void> {
  const window = dependencies.getWindow()
  if (window === null || !window.hasMore || window.loading) return

  dependencies.dispatch({ type: "history_page_started" })
  if (window.before === null) {
    dependencies.dispatch({ type: "history_page_failed" })
    return
  }
  try {
    const page = await dependencies.api.getAgentRuntimeHistory(
      input.agentRuntimeId,
      {
        throughSeq: window.through,
        beforeSeq: window.before,
        limit: HISTORY_PAGE_LIMIT,
      },
      { signal: input.signal }
    )
    assertHistoryPage(page, window.through, window.before)
    dependencies.dispatch({
      type: "history_page_loaded",
      items: page.items,
      historyBefore: page.next_before_event_seq,
      historyHasMore: page.has_more,
    })
  } catch (error) {
    if (input.signal?.aborted) return
    dependencies.dispatch({ type: "history_page_failed" })
    if (error instanceof ApiError && error.status === 404)
      dependencies.dispatch({ type: "missing", error })
  }
}

function assertHistoryPage(
  page: AgentRuntimeHistoryPage,
  through: string,
  before?: string
): void {
  if (page.through_event_seq !== through)
    throw new ApiError(
      "invalid_response",
      0,
      "history used a different snapshot barrier"
    )
  if (
    before !== undefined &&
    (page.items.some((item) => compareEventSeq(item.event_seq, before) >= 0) ||
      (page.next_before_event_seq !== null &&
        compareEventSeq(page.next_before_event_seq, before) >= 0))
  )
    throw new ApiError(
      "invalid_response",
      0,
      "history pagination did not move backwards"
    )
}

function assertFrameIdentity(
  frame: { agent_runtime_id: string; agent_id: string },
  expectedRuntimeId: string,
  expectedAgentId?: string
): void {
  if (
    frame.agent_runtime_id !== expectedRuntimeId ||
    (expectedAgentId !== undefined && frame.agent_id !== expectedAgentId)
  )
    throw protocolIdentityError()
}

function assertViewIdentity(
  view: AgentRuntimeView,
  expectedRuntimeId: string,
  expectedAgentId?: string
): void {
  if (
    view.agent_runtime_id !== expectedRuntimeId ||
    (expectedAgentId !== undefined && view.agent_id !== expectedAgentId)
  )
    throw protocolIdentityError()
}

function protocolIdentityError(): ApiError {
  return new ApiError(
    PROTOCOL_IDENTITY_CODE,
    0,
    "the stream and snapshot identities do not match"
  )
}

function linkAbort(outer: AbortSignal, inner: AbortController): () => void {
  if (outer.aborted) {
    inner.abort(outer.reason)
    return () => {}
  }
  const onAbort = () => inner.abort(outer.reason)
  outer.addEventListener("abort", onAbort)
  return () => outer.removeEventListener("abort", onAbort)
}

function abortDriven(signal: AbortSignal): Promise<void> {
  return new Promise<void>((resolve) => {
    if (signal.aborted) resolve()
    else signal.addEventListener("abort", () => resolve(), { once: true })
  })
}

function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted)
    throw signal.reason instanceof Error
      ? signal.reason
      : new DOMException("recovery aborted", "AbortError")
}

function isApiErrorCode(error: unknown, code: string): boolean {
  return error instanceof ApiError && error.code === code
}

function toConnectionError(error: unknown): ApiError {
  if (error instanceof ApiError && error.code !== "cursor_expired") return error
  return new ApiError(
    "connection_error",
    error instanceof ApiError ? error.status : 0,
    error instanceof Error ? error.message : "connection failed"
  )
}
