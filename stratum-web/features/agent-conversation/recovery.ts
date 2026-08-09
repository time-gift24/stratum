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
/** 单次 AgentView/history fetch 的最大等待；每一页单独重置。 */
const RECONCILE_FETCH_TIMEOUT_MS = 30_000
/** cold bootstrap 最多暂存与 server SSE queue 相同数量的 durable frames */
const COLD_BUFFER_MAX_FRAMES = 256
/** 限制 parsed frame 对象在 bootstrap 期间占用的近似 JSON 字符数 */
const COLD_BUFFER_MAX_CHARS = 4 * 1024 * 1024

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
      // stream 干净结束后携 page-memory cursor 进入下一轮 retained tail。
      if (session.cursor === undefined)
        await coldBootstrap(dependencies, input, session)
      else await resumeTail(dependencies, input, session)
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
      if (isApiErrorCode(error, "realtime_unavailable")) {
        // live / retained-tail 阶段的 NATS 失败只降级 realtime；已经加载的
        // PG snapshot、历史与 transient UI 保持，后续由低频 reconcile 收敛。
        dependencies.dispatch({ type: "realtime_degraded", degraded: true })
        dependencies.dispatch({ type: "recovery_ready" })
        await abortDriven(input.signal)
        return
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
 * Cold bootstrap（无 cursor），随后持续到 live stream 结束；degraded
 *（realtime_unavailable）时持续到外层 abort。
 */
async function coldBootstrap(
  dependencies: RecoveryDependencies,
  input: { agentId: string; signal: AbortSignal },
  session: SessionState
): Promise<void> {
  const { agentId, signal } = input
  dependencies.dispatch({ type: "recovery_started", agentId })

  const buffered: DurableFrame[] = []
  let bufferedChars = 0
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
        if (frame.kind === "durable") {
          const frameChars = JSON.stringify(frame).length
          if (
            buffered.length >= COLD_BUFFER_MAX_FRAMES ||
            bufferedChars + frameChars > COLD_BUFFER_MAX_CHARS
          ) {
            // A slow PG snapshot must not turn the browser into an unbounded
            // second event buffer. Treat local overflow exactly like the
            // server reset contract and restart without a cursor.
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

  // 立即把 stream 终止变成可竞争的 rejection。它不只保护 ready：ready
  // 之后的 PG snapshot 也必须仍被同一条 live subscription 包围，否则在
  // snapshot barrier 与下一条 DeliverPolicy::New subscription 之间会出现空洞。
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
  // 安装 rejection handler，避免 subscription 在 ready 之前失败时产生无人
  // 消费的 rejected promise；snapshot 阶段另由 Promise.race 直接观察。
  void streamEnded.catch((error: unknown) => {
    if (!live && !sawReset) readyReject(error)
  })

  try {
    // (1) 等 stream_ready：subscription 已建立、server 已开始 buffering
    await ready
  } catch (error) {
    streamControl.abort()
    unlink()
    if (isApiErrorCode(error, "realtime_unavailable")) {
      // NATS 不可用：PG-only degraded bootstrap，核心命令不受影响
      dependencies.dispatch(await readSnapshot(dependencies, input))
      dependencies.dispatch({ type: "realtime_degraded", degraded: true })
      dependencies.dispatch({ type: "recovery_ready" })
      return abortDriven(signal)
    }
    if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
    throw error
  }

  try {
    // (2) 读 AgentView，以 snapshot_event_seq 为固定 through 读最新 history page
    const snapshot = await Promise.race([
      readSnapshot(dependencies, {
        agentId,
        signal: streamControl.signal,
      }),
      streamEnded,
    ])
    throwIfAborted(signal)
    // reset 终止整个增量路径；即使 PG 请求已经完成，也不能提交该连接的
    // snapshot、buffer 或 cursor。
    if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
    dependencies.dispatch(snapshot)

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
    if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
    throw error
  }
  // live 期间保持 abort 链路（外层 abort 必须断开 stream），done 落定后解绑
  return subscription.done
    .catch((error: unknown) => {
      if (sawReset) throw new ApiError(STREAM_RESET_CODE, 0, "stream reset")
      throw error
    })
    .finally(unlink)
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
async function readSnapshot(
  dependencies: RecoveryDependencies,
  input: { agentId: string; signal: AbortSignal }
): Promise<Extract<ConversationAction, { type: "snapshot_loaded" }>> {
  const view = await dependencies.api.getAgent(input.agentId, {
    signal: input.signal,
  })
  throwIfAborted(input.signal)
  const page = await dependencies.api.getHistory(
    input.agentId,
    {
      throughSeq: view.snapshot_event_seq,
      limit: HISTORY_PAGE_LIMIT,
    },
    { signal: input.signal }
  )
  throwIfAborted(input.signal)
  return {
    type: "snapshot_loaded",
    view,
    items: page.items,
    historyBefore: page.next_before_event_seq,
    historyHasMore: page.has_more,
  }
}

export type ReconcileDependencies = {
  api: Pick<StratumApi, "getAgent" | "getHistory">
  /** 当前已应用 barrier；尚未完成 bootstrap 时返回 null（跳过本次 reconcile） */
  getBarrier(): string | null
  /** 同 barrier 的 process-local advisory 也会变化；仅最新请求可提交 */
  isCurrent(): boolean
  dispatch(action: ConversationAction): void
}

/**
 * 增量 reconcile：新 barrier T > 已应用 B 时，从 through=T 反向分页直到越过
 * B，只合并 (B,T] 的可见 items，并替换 status/pending approvals/latest
 * usage 等 barrier-governed 字段。best-effort：瞬时失败等下一次触发。
 */
export async function reconcileConversation(
  dependencies: ReconcileDependencies,
  input: { agentId: string; signal?: AbortSignal }
): Promise<void> {
  const barrier = dependencies.getBarrier()
  if (barrier === null || !dependencies.isCurrent()) return

  let view: AgentView
  try {
    view = await withReconcileFetchDeadline(input.signal, (signal) =>
      dependencies.api.getAgent(input.agentId, { signal })
    )
  } catch (error) {
    const currentBarrier = dependencies.getBarrier()
    if (
      error instanceof ApiError &&
      error.status === 404 &&
      dependencies.isCurrent() &&
      currentBarrier !== null &&
      compareEventSeq(currentBarrier, barrier) === 0
    )
      dependencies.dispatch({ type: "missing", error })
    return
  }
  const barrierAfterView = dependencies.getBarrier()
  if (
    !dependencies.isCurrent() ||
    barrierAfterView === null ||
    compareEventSeq(barrierAfterView, barrier) !== 0
  )
    return

  const items: AgentDurableRecordV1[] = []
  if (compareEventSeq(view.snapshot_event_seq, barrier) > 0) {
    let before: string | undefined
    try {
      for (;;) {
        const page = await withReconcileFetchDeadline(input.signal, (signal) =>
          dependencies.api.getHistory(
            input.agentId,
            {
              throughSeq: view.snapshot_event_seq,
              beforeSeq: before,
              limit: RECONCILE_PAGE_LIMIT,
            },
            { signal }
          )
        )
        if (!dependencies.isCurrent()) return
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

  const currentBarrier = dependencies.getBarrier()
  if (
    !dependencies.isCurrent() ||
    currentBarrier === null ||
    compareEventSeq(currentBarrier, barrier) !== 0
  )
    return
  dependencies.dispatch({
    type: "view_reconciled",
    baseBarrier: barrier,
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
    // Start through a microtask so a synchronous throw from a mock/client is
    // captured by the promise that already participates in the race. The
    // losing operation stays observed by Promise.race, so a late rejection
    // after timeout cannot become unhandled even when the client ignores abort.
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
  api: Pick<StratumApi, "getHistory">
  getWindow(): HistoryWindow | null
  dispatch(action: ConversationAction): void
}

/** 向上滚动加载更旧一页：固定 through barrier + exclusive before cursor */
export async function loadOlderHistoryPage(
  dependencies: HistoryDependencies,
  input: { agentId: string; signal?: AbortSignal }
): Promise<void> {
  const window = dependencies.getWindow()
  if (window === null || !window.hasMore || window.loading) return

  dependencies.dispatch({ type: "history_page_started" })
  try {
    const page = await dependencies.api.getHistory(
      input.agentId,
      {
        throughSeq: window.through,
        beforeSeq: window.before ?? undefined,
        limit: HISTORY_PAGE_LIMIT,
      },
      { signal: input.signal }
    )
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
