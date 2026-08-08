import type {
  ConversationAction,
  DurableFrame,
} from "@/features/agent-conversation/types"
import {
  ApiError,
  compareEventSeq,
  type AgentDurableRecordV1,
  type AgentView,
  type StratumApi,
} from "@/lib/stratum/api"
import { subscribeToAgentEvents } from "@/lib/stratum/event-stream"

/**
 * Web 恢复与收敛（runtime-event-protocol spec 的固定算法）：
 *
 * - cold bootstrap 固定顺序：建立并 buffer SSE → 等 `stream_ready` → 读
 *   AgentView + barrier 内最新 history page → 应用 PG snapshot → 只应用
 *   `event_seq > barrier` 的 buffered durable frame → 丢弃全部 buffered
 *   telemetry → 提交最新 cursor 进入 live mode。
 * - cursor 只是不透明 NATS transport position，只存当前页面内存；410 /
 *   `stream_reset` 后丢弃 buffer、draft 与 cursor，从无 cursor cold
 *   bootstrap 重新开始。
 * - reconcile 是增量的：旧 barrier B → 新 barrier T 时反向分页 history，
 *   只合并 (B,T] 的可见 items 并替换 view 字段。
 * - NATS 不可用（503 realtime_unavailable）进入 degraded：PG snapshot /
 *   reconcile 继续工作，realtime 标记降级。
 */

/** 首屏与向上分页的页大小（协议默认 50） */
const HISTORY_PAGE_LIMIT = 50
/** reconcile 反向补页的上限 */
const RECONCILE_PAGE_LIMIT = 256

const STREAM_RESET_CODE = "stream_reset"

export type RecoveryDependencies = {
  api: Pick<StratumApi, "getAgent" | "getHistory">
  subscribe: typeof subscribeToAgentEvents
  /** cursor 只存页面内存（hook 提供 Map），禁止持久化 */
  loadCursor(agentId: string): string | undefined
  saveCursor(agentId: string, cursor: string): void
  clearCursor(agentId: string): void
  dispatch(action: ConversationAction): void
}

type SessionState = { cursor: string | undefined }

/**
 * 驱动一个 Agent 的 realtime 会话：cold bootstrap → live tail → 断流后携
 * page-memory cursor 重连短 tail；410/stream_reset 时回到无 cursor cold
 * bootstrap。返回时机：不可恢复的连接错误、404 或外层 abort。
 */
export async function runConversationSession(
  dependencies: RecoveryDependencies,
  input: { agentId: string; signal: AbortSignal }
): Promise<void> {
  const session: SessionState = {
    cursor: dependencies.loadCursor(input.agentId),
  }

  while (!input.signal.aborted) {
    try {
      const done =
        session.cursor === undefined
          ? await coldBootstrap(dependencies, input, session)
          : resumeTail(dependencies, input, session)
      // live mode 已建立：等待 stream 结束，干净结束后携 cursor 重连
      await done
    } catch (error) {
      if (input.signal.aborted) return
      if (
        isApiErrorCode(error, STREAM_RESET_CODE) ||
        isApiErrorCode(error, "cursor_expired")
      ) {
        // 丢弃该连接的 buffer、transient draft 与 page cursor，重新 cold bootstrap
        session.cursor = undefined
        dependencies.clearCursor(input.agentId)
        continue
      }
      if (error instanceof ApiError && error.status === 404) {
        dependencies.dispatch({ type: "missing", error })
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

/**
 * Cold bootstrap（无 cursor）。返回进入 live mode 后 stream 的完成 promise；
 * degraded（realtime_unavailable）时返回随外层 abort 结束的 promise。
 */
async function coldBootstrap(
  dependencies: RecoveryDependencies,
  input: { agentId: string; signal: AbortSignal },
  session: SessionState
): Promise<Promise<void>> {
  const { agentId, signal } = input
  dependencies.dispatch({ type: "recovery_started", agentId })

  const buffered: DurableFrame[] = []
  let live = false
  let sawReset = false
  let latestCursor: string | null = null

  const streamControl = new AbortController()
  const unlink = linkAbort(signal, streamControl)

  let readyResolve!: () => void
  let readyReject!: (error: unknown) => void
  const ready = new Promise<void>((resolve, reject) => {
    readyResolve = resolve
    readyReject = reject
  })

  const subscription = dependencies.subscribe({
    // The hook binds the configured base URL before this reaches fetch.
    baseUrl: "",
    agentId,
    signal: streamControl.signal,
    onFrame: (frame, frameCursor) => {
      if (frame.agent_id !== agentId) return
      if (frame.kind === "control") {
        if (frame.event.type === "stream_reset") {
          // server buffer overflow：主动断开，丢弃该连接的 buffer/draft/cursor
          sawReset = true
          streamControl.abort()
          readyReject(new ApiError(STREAM_RESET_CODE, 0, "stream reset"))
        } else {
          readyResolve()
        }
        return
      }
      if (frameCursor !== null) latestCursor = frameCursor
      if (!live) {
        // bootstrap 期间只 buffer durable frame；telemetry 没有可证明完整的
        // call prefix，全部丢弃
        if (frame.kind === "durable") buffered.push(frame)
        return
      }
      if (frameCursor !== null) {
        session.cursor = frameCursor
        dependencies.saveCursor(agentId, frameCursor)
      }
      dependencies.dispatch(
        frame.kind === "durable"
          ? { type: "durable_frame", frame }
          : { type: "telemetry_frame", frame }
      )
    },
  })

  const done = subscription.done.catch((error: unknown) => {
    if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
    throw error
  })
  subscription.done.then(
    () => {
      if (!live && !sawReset)
        readyReject(
          new ApiError("stream_closed", 0, "event stream closed before ready")
        )
    },
    (error: unknown) => {
      if (!live) readyReject(error)
    }
  )

  try {
    // (1) 等 stream_ready：subscription 已建立、server 已开始 buffering
    await ready
  } catch (error) {
    unlink()
    if (isApiErrorCode(error, "realtime_unavailable")) {
      // NATS 不可用：PG-only degraded bootstrap，核心命令不受影响
      await loadSnapshot(dependencies, input)
      dependencies.dispatch({ type: "realtime_degraded", degraded: true })
      dependencies.dispatch({ type: "recovery_ready" })
      return abortDriven(signal)
    }
    throw error
  }

  try {
    // (2) 读 AgentView，以 snapshot_event_seq 为固定 through 读最新 history page
    await loadSnapshot(dependencies, input)
    throwIfAborted(signal)

    // (3) 应用 buffered durable frames（reducer 跳过 event_seq <= barrier）
    for (const frame of buffered)
      dependencies.dispatch({ type: "durable_frame", frame })

    // (5) 全部 merge 成功后才提交最新 cursor，进入 live mode
    if (latestCursor !== null) {
      session.cursor = latestCursor
      dependencies.saveCursor(agentId, latestCursor)
    }
    live = true
    dependencies.dispatch({ type: "realtime_degraded", degraded: false })
    dependencies.dispatch({ type: "recovery_ready" })
  } catch (error) {
    // bootstrap 失败不提交 cursor：断开 stream，由外层决定 cold 重试或报错
    streamControl.abort()
    unlink()
    throw error
  }
  // live 期间保持 abort 链路（外层 abort 必须断开 stream），done 落定后解绑
  return done.finally(unlink)
}

/** 页面内携 cursor 续传短 tail（resume within page），返回 stream 完成 promise */
function resumeTail(
  dependencies: RecoveryDependencies,
  input: { agentId: string; signal: AbortSignal },
  session: SessionState
): Promise<void> {
  const { agentId, signal } = input
  let sawReset = false
  const streamControl = new AbortController()
  const unlink = linkAbort(signal, streamControl)

  const subscription = dependencies.subscribe({
    baseUrl: "",
    agentId,
    afterCursor: session.cursor,
    signal: streamControl.signal,
    onFrame: (frame, frameCursor) => {
      if (frame.agent_id !== agentId) return
      if (frame.kind === "control") {
        if (frame.event.type === "stream_reset") {
          sawReset = true
          streamControl.abort()
        }
        return
      }
      if (frameCursor !== null) {
        session.cursor = frameCursor
        dependencies.saveCursor(agentId, frameCursor)
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

/** cold bootstrap 第 (2) 步：AgentView + barrier 内最新一页 history */
async function loadSnapshot(
  dependencies: RecoveryDependencies,
  input: { agentId: string; signal: AbortSignal }
): Promise<void> {
  const view = await dependencies.api.getAgent(input.agentId)
  throwIfAborted(input.signal)
  const page = await dependencies.api.getHistory(input.agentId, {
    throughSeq: view.snapshot_event_seq,
    limit: HISTORY_PAGE_LIMIT,
  })
  throwIfAborted(input.signal)
  dependencies.dispatch({
    type: "snapshot_loaded",
    view,
    items: page.items,
    historyBefore: page.next_before_event_seq,
    historyHasMore: page.has_more,
  })
}

export type ReconcileDependencies = {
  api: Pick<StratumApi, "getAgent" | "getHistory">
  /** 当前已应用 barrier；尚未完成 bootstrap 时返回 null（跳过本次 reconcile） */
  getBarrier(): string | null
  dispatch(action: ConversationAction): void
}

/**
 * 增量 reconcile：新 barrier T > 已应用 B 时，从 through=T 反向分页直到越过
 * B，只合并 (B,T] 的可见 items，并替换 status/pending approvals/latest
 * usage 等 barrier-governed 字段。best-effort：瞬时失败等下一次触发。
 */
export async function reconcileConversation(
  dependencies: ReconcileDependencies,
  input: { agentId: string }
): Promise<void> {
  const barrier = dependencies.getBarrier()
  if (barrier === null) return

  let view: AgentView
  try {
    view = await dependencies.api.getAgent(input.agentId)
  } catch (error) {
    if (error instanceof ApiError && error.status === 404)
      dependencies.dispatch({ type: "missing", error })
    return
  }
  if (dependencies.getBarrier() === null) return

  const items: AgentDurableRecordV1[] = []
  if (compareEventSeq(view.snapshot_event_seq, barrier) > 0) {
    let before: string | undefined
    try {
      for (;;) {
        const page = await dependencies.api.getHistory(input.agentId, {
          throughSeq: view.snapshot_event_seq,
          beforeSeq: before,
          limit: RECONCILE_PAGE_LIMIT,
        })
        const fresh = page.items.filter(
          (item) => compareEventSeq(item.event_seq, barrier) > 0
        )
        items.unshift(...fresh)
        const oldest = page.items[0]
        if (
          !page.has_more ||
          oldest === undefined ||
          compareEventSeq(oldest.event_seq, barrier) <= 0 ||
          page.next_before_event_seq === null
        )
          break
        before = page.next_before_event_seq
      }
    } catch {
      return
    }
  }

  dependencies.dispatch({ type: "view_reconciled", view, items })
}

export type HistoryWindow = {
  through: string
  before: string | null
  hasMore: boolean
  loading: boolean
}

export type HistoryDependencies = {
  api: Pick<StratumApi, "getHistory">
  getWindow(): HistoryWindow | null
  dispatch(action: ConversationAction): void
}

/** 向上滚动加载更旧一页：固定 through barrier + exclusive before cursor */
export async function loadOlderHistoryPage(
  dependencies: HistoryDependencies,
  input: { agentId: string }
): Promise<void> {
  const window = dependencies.getWindow()
  if (window === null || !window.hasMore || window.loading) return

  dependencies.dispatch({ type: "history_page_started" })
  try {
    const page = await dependencies.api.getHistory(input.agentId, {
      throughSeq: window.through,
      beforeSeq: window.before ?? undefined,
      limit: HISTORY_PAGE_LIMIT,
    })
    dependencies.dispatch({
      type: "history_page_loaded",
      items: page.items,
      historyBefore: page.next_before_event_seq,
      historyHasMore: page.has_more,
    })
  } catch (error) {
    dependencies.dispatch({ type: "history_page_failed" })
    if (error instanceof ApiError && error.status === 404)
      dependencies.dispatch({ type: "missing", error })
  }
}

function linkAbort(outer: AbortSignal, inner: AbortController): () => void {
  if (outer.aborted) {
    inner.abort()
    return () => {}
  }
  const onAbort = () => inner.abort()
  outer.addEventListener("abort", onAbort)
  return () => outer.removeEventListener("abort", onAbort)
}

/** degraded 模式没有 live stream：返回一个随外层 abort 结束的 promise */
function abortDriven(signal: AbortSignal): Promise<void> {
  return new Promise<void>((resolve) => {
    if (signal.aborted) resolve()
    else signal.addEventListener("abort", () => resolve(), { once: true })
  })
}

function throwIfAborted(signal: AbortSignal): void {
  if (signal.aborted) throw new DOMException("recovery aborted", "AbortError")
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
