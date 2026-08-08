"use client"

import { useCallback, useEffect, useRef, useState } from "react"
import { ArrowDown, Loader2 } from "lucide-react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import { CompactionMarker } from "@/components/stratum/conversation/compaction-marker"
import {
  AssistantMessage,
  UserMessage,
} from "@/components/stratum/conversation/message"
import { TerminalMarker } from "@/components/stratum/conversation/terminal-marker"
import type {
  ConversationItem,
  ConversationMessage,
} from "@/components/stratum/conversation/types"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

gsap.registerPlugin(useGSAP)

/** 会话切换过场编排：旧内容淡出下沉 → 空场呼吸 → 新内容淡入上浮归位
 * （严格串行：进场只在退场 onComplete 且快照移除后开始） */
const SWITCH_CHOREO = {
  exitDuration: 0.28,
  exitEase: "power1.in",
  exitY: 6,
  enterDuration: 0.45,
  enterEase: "expo.out",
  enterY: 8,
  /** 退场与进场之间的呼吸间隔（s） */
  enterDelay: 0.06,
} as const

/** 滚动到距顶部该阈值内且有更旧分页时自动加载 */
const OLDER_LOAD_THRESHOLD = 64

function renderItem(
  item: ConversationItem,
  isLast: boolean,
  handlers: {
    onReload?: (message: ConversationMessage) => void
    onEditUserMessage?: (message: ConversationMessage) => void
    onRetryUserMessage?: (message: ConversationMessage) => void
  } = {}
) {
  if (item.kind === "compaction")
    return <CompactionMarker key={item.id} summary={item.summary} />
  if (item.kind === "terminal")
    return (
      <TerminalMarker
        key={item.id}
        terminal={item.terminal}
        errorText={item.errorText}
      />
    )
  const message = item.message
  return message.role === "user" ? (
    <UserMessage
      key={item.id}
      message={message}
      onEdit={handlers.onEditUserMessage}
      onRetry={handlers.onRetryUserMessage}
    />
  ) : (
    <AssistantMessage
      key={item.id}
      message={message}
      isLast={isLast}
      onReload={handlers.onReload}
    />
  )
}

/**
 * ConversationThread —— 对话消息列表（assistant-ui thread 底稿的展示层 fork）。
 * 结构：可滚动 viewport（消息列居中，max-w 44rem）+ composer。数据驱动：
 * items 由调用方持有；滚动到底部仅在用户本就近底时自动跟随，上翻后不打扰。
 *
 * items 混合三类条目（升序渲染，id = agentId:eventSeq 十进制字符串）：
 * 普通消息、TranscriptCompacted 可折叠 marker、安全 terminal marker。
 * 向上分页：滚动接近顶部且 hasOlder 时调 onLoadOlder；旧页 prepend 后
 * 用预先记录的 scrollHeight 差值恢复滚动位置（不跳动）。
 *
 * 两种布局模式（单一 DOM 树，节点跨模式保留，composer 不 remount）：
 * - 空态（无条目）：composer 包装器脱离文档流（absolute inset-0 +
 *   items-center justify-center + pointer-events-none，内部恢复 auto），
 *   恒定容器正中心；欢迎语独立锚定在中线上方（bottom: 50% + 2rem），
 *   其高度/有无及未来新增内容都不改变 composer 位置。宽度与稳态一致。
 * - 稳态（有条目）：消息流 + composer sticky 底部。
 *
 * 双向过场（GSAP FLIP）：passive effect 在每次提交后（绘制之后、布局干净，
 * 不产生 forced reflow）记录 composer rect；welcome 只在可见时记录。
 * 播放规则：只在同一会话内的空 ⇄ 非空翻转时播——
 * - 空 → 稳态（当前对话发出第一条消息）：composer 从中心滑到底部
 *   （y: 差值 → 0，expo.out 0.55s），同一 timeline 欢迎语 fixed 在旧位置
 *   淡出上移、消息区淡入。sendVersion 信号使首发后的 null → 新 agentId
 *   被视为同一对话的确立而非切换。
 * - 稳态 → 空（点新对话，目的地 conversationId 为 null）：同参数镜像滑回。
 * - 会话切换（conversationId 变化，含恢复填充引起的翻转）：一律不播位置
 *   动画，直接落目标布局，内容仅轻微淡入。
 * 完成后 clearProps 交还 CSS。打断：killTweensOf + 反向时从当前视觉位置
 * 续播；prefers-reduced-motion 全程瞬时。
 */
export function ConversationThread({
  items,
  composer,
  welcome,
  conversationId,
  sendVersion,
  recovering = false,
  hasOlder = false,
  olderLoading = false,
  onLoadOlder,
  onReload,
  onEditUserMessage,
  onRetryUserMessage,
  className,
}: {
  items: ConversationItem[]
  /** 底部 sticky 区域（如 PromptInput） */
  composer?: React.ReactNode
  /** 空状态内容（items 为空时居中展示） */
  welcome?: React.ReactNode
  /** 当前会话 id（state.agentId）；变化 = 切换会话，一律不播位置动画 */
  conversationId?: string | null
  /** 用户发送递增信号；首发后的 null → 新 agentId 视为同一对话的确立 */
  sendVersion?: number
  /** 会话恢复中（phase === "recovering"）：切换过场的进场等恢复结束再播 */
  recovering?: boolean
  /** 固定 through barrier 内还有更旧的分页 */
  hasOlder?: boolean
  /** 更旧分页加载中（顶部显示克制加载行） */
  olderLoading?: boolean
  /** 用户滚动接近顶部时请求更旧一页 */
  onLoadOlder?: () => void
  onReload?: (message: ConversationMessage) => void
  onEditUserMessage?: (message: ConversationMessage) => void
  /** 用户消息发送失败时的重发入口 */
  onRetryUserMessage?: (message: ConversationMessage) => void
  className?: string
}) {
  const isEmpty = items.length === 0
  const rootRef = useRef<HTMLDivElement>(null)
  const viewportRef = useRef<HTMLDivElement>(null)
  const welcomeRef = useRef<HTMLDivElement>(null)
  const messagesRef = useRef<HTMLDivElement>(null)
  const composerRef = useRef<HTMLDivElement>(null)
  const prevIsEmptyRef = useRef<boolean | null>(null)
  const prevConversationIdRef = useRef<string | null | undefined>(conversationId)
  const prevSendVersionRef = useRef(sendVersion)
  // 首发信号：用户在当前（新）对话发出第一条消息；其后一次 null → 新
  // agentId 是同一对话的确立而非切换，首次 isEmpty 翻转播正向过场
  const pendingSendRef = useRef(false)
  // 会话切换后的恢复填充不算"同一会话内空 → 有消息"，抑制下一次翻转动画
  const suppressNextFlipRef = useRef(false)
  // FLIP first 帧：每次提交后记录，切换模式时即为旧布局位置（双向都需要）
  const prevComposerRectRef = useRef<DOMRect | null>(null)
  const prevWelcomeRectRef = useRef<DOMRect | null>(null)
  // 向上分页的滚动锚点：触发加载时记录，prepend 完成后按高度差恢复位置
  const prependAnchorRef = useRef<{ scrollHeight: number; scrollTop: number } | null>(null)
  const [nearBottom, setNearBottom] = useState(true)

  // 会话切换过场：track.departing = 旧内容的最后已知快照（退场期间不再重渲染
  // 旧内容，借鉴 approval-dock 的 known/leaving）；enterPendingRef = 退场完成且
  // recovery 结束后待播的进场。快照内容随 items.length / isEmpty /
  // sendVersion 更新（不按 token 更新，保住流式热路径）
  type DepartingContent = { items: ConversationItem[]; isEmpty: boolean }
  const [track, setTrack] = useState<{
    conversationId: string | null | undefined
    sendVersion: number | undefined
    items: ConversationItem[]
    isEmpty: boolean
    departing: DepartingContent | null
  }>({ conversationId, sendVersion, items, isEmpty, departing: null })
  const enterPendingRef = useRef(false)
  const departingRef = useRef<HTMLDivElement>(null)
  // 已开始退场的快照（防止 recovering 翻转重跑 effect 时退场 tween 重头再播）
  const departingStartedRef = useRef<DepartingContent | null>(null)

  // derive-state-during-render：会话切换时快照旧内容为 departing（首发创建的
  // null → 新 agentId 不算切换，与 FLIP 的 pendingSend 规则一致）
  if (track.conversationId !== conversationId) {
    const createFlow =
      track.conversationId === null && sendVersion !== track.sendVersion
    setTrack({
      conversationId,
      sendVersion,
      items,
      isEmpty,
      departing: createFlow
        ? null
        : { items: track.items, isEmpty: track.isEmpty },
    })
  } else if (
    track.items.length !== items.length ||
    track.isEmpty !== isEmpty ||
    track.sendVersion !== sendVersion
  ) {
    setTrack({ ...track, items, isEmpty, sendVersion })
  }
  const departing = track.departing
  const clearDeparting = useCallback(
    () =>
      setTrack((prev) =>
        prev.departing ? { ...prev, departing: null } : prev
      ),
    []
  )
  // 退场完成信号（按快照 identity 清除）：过期 tween 的 onComplete 不会
  // 误清更新快照的退场——进场只可能在当前退场真正结束后开始
  const finishDeparting = useCallback(
    (snapshot: DepartingContent) =>
      setTrack((prev) =>
        prev.departing === snapshot ? { ...prev, departing: null } : prev
      ),
    []
  )

  // 近底时新内容（含流式增长）瞬时贴底——流式期间每 30ms 一帧，
  // smooth 会反复重启动画造成滞后抖动；平滑只留给用户主动点的「回到底部」
  useEffect(() => {
    const viewport = viewportRef.current
    if (!viewport || !nearBottom) return
    viewport.scrollTo({ top: viewport.scrollHeight, behavior: "auto" })
  }, [items, nearBottom])

  // 向上分页完成后恢复滚动位置：旧条目 prepend 在顶部，scrollHeight 增量
  // 加回 scrollTop，用户视口不动
  useEffect(() => {
    if (olderLoading) return
    const anchor = prependAnchorRef.current
    const viewport = viewportRef.current
    if (!anchor || !viewport) return
    prependAnchorRef.current = null
    viewport.scrollTop =
      anchor.scrollTop + (viewport.scrollHeight - anchor.scrollHeight)
  }, [olderLoading, items])

  // 每次提交后记录 composer rect（passive effect 在绘制之后运行，布局已干净，
  // 读取不产生 forced reflow）；welcome 只在可见时记录，隐藏态保留最后可见
  // 位置，供退场动画复现
  useEffect(() => {
    prevComposerRectRef.current =
      composerRef.current?.getBoundingClientRect() ?? null
    const welcomeElement = welcomeRef.current
    if (welcomeElement && welcomeElement.getClientRects().length > 0)
      prevWelcomeRectRef.current = welcomeElement.getBoundingClientRect()
  })

  // 空态 ⇄ 稳态过场（规则：同一会话内的空 ⇄ 非空翻转才播；
  // 会话切换一律不播位置动画）
  useGSAP(
    () => {
      const wasEmpty = prevIsEmptyRef.current
      const previousConversationId = prevConversationIdRef.current
      const conversationChanged = previousConversationId !== conversationId
      const sendHappened = prevSendVersionRef.current !== sendVersion
      prevIsEmptyRef.current = isEmpty
      prevConversationIdRef.current = conversationId
      prevSendVersionRef.current = sendVersion

      const composerElement = composerRef.current
      const welcomeElement = welcomeRef.current
      const messageList = messagesRef.current
      if (!composerElement) return
      const targets = [composerElement, welcomeElement, messageList].filter(
        (element): element is HTMLDivElement => element !== null
      )
      const reduce = window.matchMedia(
        "(prefers-reduced-motion: reduce)"
      ).matches
      gsap.killTweensOf(targets)

      // 双向同构的编排参数：composer 滑动与 welcome 淡入/淡出同时长、同缓动、
      // 同起点（都挂在 timeline 0 位），方向只决定 y 差值与淡入/淡出
      const CHOREO = {
        ease: "expo.out",
        composerDuration: 0.55,
        welcomeDuration: 0.35,
        messagesDuration: 0.4,
        messagesOffset: 0.1,
      } as const

      const first = prevComposerRectRef.current
      const last = composerElement.getBoundingClientRect()
      const deltaY = first ? first.top - last.top : 0
      const flipped = wasEmpty !== null && wasEmpty !== isEmpty

      const timeline = gsap.timeline({
        defaults: { ease: CHOREO.ease },
        onComplete: () => gsap.set(targets, { clearProps: "all" }),
      })
      if (deltaY !== 0)
        timeline.fromTo(
          composerElement,
          { y: deltaY },
          { y: 0, duration: reduce ? 0 : CHOREO.composerDuration },
          0
        )

      const settle = () => {
        timeline.kill()
        gsap.set(targets, { clearProps: "all" })
      }

      if (sendHappened) pendingSendRef.current = true

      if (flipped && isEmpty) {
        // 反向：仅目的地是新对话视图（conversationId 为 null）才播；
        // 切到其它会话途中的暂态空态不播，并抑制随后恢复填充的正向
        pendingSendRef.current = false
        if (conversationId === null) {
          if (welcomeElement)
            timeline.fromTo(
              welcomeElement,
              { autoAlpha: 0 },
              { autoAlpha: 1, duration: reduce ? 0 : CHOREO.welcomeDuration },
              0
            )
          return
        }
        suppressNextFlipRef.current = true
        settle()
        return
      }

      if (
        conversationChanged &&
        pendingSendRef.current &&
        previousConversationId === null
      ) {
        // 首发创建：send 后的 null → 新 agentId 是同一对话的确立（消费一次）
        pendingSendRef.current = false
      } else if (conversationChanged) {
        // 会话切换：composer 一律不做位置动画；内容淡入淡出由 departing/enter
        // 过场接管；恢复填充引起的下一次翻转也抑制
        suppressNextFlipRef.current = true
        pendingSendRef.current = false
        settle()
        return
      }

      if (flipped && !isEmpty) {
        if (suppressNextFlipRef.current) {
          // 恢复填充驱动的翻转：直接落稳态
          suppressNextFlipRef.current = false
          settle()
          return
        }
        // 同一会话内空 → 有消息（用户发出第一条消息）：播正向过场
        pendingSendRef.current = false
        const firstWelcome = prevWelcomeRectRef.current
        if (welcomeElement && firstWelcome) {
          // 欢迎语固定在旧位置淡出（稳态下它是 hidden，临时复现）
          gsap.set(welcomeElement, {
            display: "block",
            position: "fixed",
            top: firstWelcome.top,
            left: firstWelcome.left,
            width: firstWelcome.width,
            zIndex: 20,
          })
          timeline.to(
            welcomeElement,
            {
              autoAlpha: 0,
              y: -12,
              duration: reduce ? 0 : CHOREO.welcomeDuration,
            },
            0
          )
        }
        if (messageList)
          timeline.fromTo(
            messageList,
            { autoAlpha: 0 },
            { autoAlpha: 1, duration: reduce ? 0 : CHOREO.messagesDuration },
            reduce ? 0 : CHOREO.messagesOffset
          )
        return
      }

      // 无翻转、无切换：settle（kill + clearProps）——只 kill 会把被中断
      // tween 的内联样式（composer translateY、welcome fixed 幽灵态、消息
      // 列表中间 opacity）残留在元素上
      settle()
    },
    { scope: rootRef, dependencies: [isEmpty, conversationId, sendVersion] }
  )

  // 会话切换过场：旧内容（departing 快照）先淡出下沉 → 新内容（recovery
  // 结束后）淡入上浮归位；快速连切时新快照替换旧快照、overwrite 接管
  useGSAP(
    () => {
      const reduce = window.matchMedia(
        "(prefers-reduced-motion: reduce)"
      ).matches
      const incomingTargets = [welcomeRef.current, messagesRef.current].filter(
        (element): element is HTMLDivElement => element !== null
      )

      if (departing) {
        // 退场期间新内容保持不可见（干净的空场，不闪 skeleton）
        gsap.set(incomingTargets, { autoAlpha: 0 })
        enterPendingRef.current = true
        if (departingStartedRef.current === departing) return
        departingStartedRef.current = departing
        const departingElement = departingRef.current
        if (!departingElement) {
          // ref 未挂上（不应发生）：异步清理，避免 effect 内同步 setState
          void Promise.resolve().then(clearDeparting)
          return
        }
        gsap.fromTo(
          departingElement,
          { autoAlpha: 1, y: 0 },
          {
            autoAlpha: 0,
            y: SWITCH_CHOREO.exitY,
            duration: reduce ? 0 : SWITCH_CHOREO.exitDuration,
            ease: SWITCH_CHOREO.exitEase,
            overwrite: "auto",
            onComplete: () => finishDeparting(departing),
          }
        )
        return
      }

      if (enterPendingRef.current && !recovering) {
        enterPendingRef.current = false
        departingStartedRef.current = null
        // 历史会话进场落在底部（最新消息）；scroll 事件会同步 nearBottom
        const viewport = viewportRef.current
        if (viewport && !isEmpty)
          viewport.scrollTo({ top: viewport.scrollHeight, behavior: "auto" })
        gsap.fromTo(
          incomingTargets,
          { autoAlpha: 0, y: SWITCH_CHOREO.enterY },
          {
            autoAlpha: 1,
            y: 0,
            duration: reduce ? 0 : SWITCH_CHOREO.enterDuration,
            ease: SWITCH_CHOREO.enterEase,
            delay: reduce ? 0 : SWITCH_CHOREO.enterDelay,
            overwrite: "auto",
          }
        )
      }
    },
    { scope: rootRef, dependencies: [departing, recovering, isEmpty, clearDeparting, finishDeparting] }
  )

  const handleScroll = () => {
    const viewport = viewportRef.current
    if (!viewport) return
    const gap =
      viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight
    setNearBottom(gap < 80)

    // 接近顶部且还有更旧分页：记录滚动锚点后请求上一页
    if (
      viewport.scrollTop < OLDER_LOAD_THRESHOLD &&
      hasOlder &&
      !olderLoading &&
      onLoadOlder
    ) {
      prependAnchorRef.current = {
        scrollHeight: viewport.scrollHeight,
        scrollTop: viewport.scrollTop,
      }
      onLoadOlder()
    }
  }

  const scrollToBottom = () => {
    viewportRef.current?.scrollTo({
      top: viewportRef.current.scrollHeight,
      behavior: "smooth",
    })
  }

  return (
    <div
      ref={rootRef}
      data-slot="conversation-thread"
      className={cn("flex h-full flex-col bg-background", className)}
    >
      <div
        ref={viewportRef}
        onScroll={handleScroll}
        className="relative flex flex-1 flex-col overflow-x-hidden overflow-y-auto"
      >
        <div className="relative mx-auto flex w-full max-w-[44rem] flex-1 flex-col px-4 pt-6">
          <div
            ref={welcomeRef}
            className={cn(
              isEmpty
                ? "absolute inset-x-0 bottom-[calc(50%+2rem)]"
                : "hidden"
            )}
          >
            {welcome}
          </div>

          <div
            ref={messagesRef}
            className="mb-14 flex flex-col gap-y-6 empty:hidden"
          >
            {olderLoading ? (
              <p
                role="status"
                className="flex items-center justify-center gap-1.5 text-xs text-muted-foreground"
              >
                <Loader2
                  aria-hidden
                  className="size-3.5 animate-spin motion-reduce:animate-none"
                />
                加载更早的消息…
              </p>
            ) : null}
            {items.map((item, index) =>
              renderItem(item, index === items.length - 1, {
                onReload,
                onEditUserMessage,
                onRetryUserMessage,
              })
            )}
          </div>

          {/* 会话切换退场层：旧内容的最后已知快照，淡出期间覆盖在新内容之上
              （新内容此时 autoAlpha 0），动画完成后从 DOM 移除 */}
          {departing ? (
            <div
              ref={departingRef}
              aria-hidden
              className="pointer-events-none absolute inset-0 px-4 pt-6"
            >
              {departing.isEmpty ? (
                <div className="absolute inset-x-0 bottom-[calc(50%+2rem)]">
                  {welcome}
                </div>
              ) : (
                <div className="flex flex-col gap-y-6">
                  {departing.items.map((item, index) =>
                    renderItem(item, index === departing.items.length - 1)
                  )}
                </div>
              )}
            </div>
          ) : null}

          {composer ? (
            <div
              className={cn(
                "flex flex-col gap-2",
                isEmpty
                  ? // 空态浮层透明（不能带 bg-background，否则盖住 welcome）
                    "pointer-events-none absolute inset-0 items-center justify-center"
                  : "sticky bottom-0 mt-auto bg-background pb-4 md:pb-6"
              )}
            >
              {!isEmpty && !nearBottom ? (
                <Button
                  variant="outline"
                  size="icon"
                  className="absolute -top-12 z-10 self-center rounded-full"
                  aria-label="回到底部"
                  onClick={scrollToBottom}
                >
                  <ArrowDown aria-hidden />
                </Button>
              ) : null}
              <div
                ref={composerRef}
                className={cn(isEmpty && "pointer-events-auto w-full")}
              >
                {composer}
              </div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )
}
