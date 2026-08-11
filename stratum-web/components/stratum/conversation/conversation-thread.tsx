"use client"

import { useEffect, useRef, useState } from "react"
import { ArrowDown, Loader2 } from "lucide-react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import { ScrollToPlugin } from "gsap/ScrollToPlugin"

import { CompactionMarker } from "@/components/stratum/conversation/compaction-marker"
import {
  AssistantMessage,
  UserMessage,
} from "@/components/stratum/conversation/message"
import { TerminalMarker } from "@/components/stratum/conversation/terminal-marker"
import {
  initialTransitionState,
  reduceThreadTransition,
} from "@/components/stratum/conversation/thread-transition"
import type {
  ConversationItem,
  ConversationMessage,
} from "@/components/stratum/conversation/types"
import { Button } from "@/components/ui/button"
import {
  MOTION_DURATION,
  MOTION_EASE,
  motionDuration,
  prefersReducedMotion,
} from "@/lib/motion"
import { cn } from "@/lib/utils"

gsap.registerPlugin(useGSAP)
gsap.registerPlugin(ScrollToPlugin)

/** 空态 ⇄ 稳态双向过场的编排参数：composer 滑动与 welcome 淡入/淡出
 * 同起点（都挂在 timeline 0 位），方向只决定 y 差值；时长/缓动全站统一
 * 尺度（lib/motion.ts）：大位移 slow，其余 base */
const CHOREO = {
  ease: MOTION_EASE.enter,
  composerDuration: MOTION_DURATION.slow,
  welcomeDuration: MOTION_DURATION.base,
  messagesDuration: MOTION_DURATION.base,
  messagesOffset: 0.1,
} as const

/** 滚动到距顶部该阈值内且有更旧分页时自动加载 */
const OLDER_LOAD_THRESHOLD = 64

/** 距底部该阈值（px）内视为"近底"，新内容自动贴底跟随 */
const NEAR_BOTTOM_THRESHOLD = 80

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
 * items 由调用方持有。滚动模型（跟随与否只由用户意图决定）：
 * 发送 → 垫片（内容不够时在消息列底部垫出可滚空间）+ 平滑锚定到视口
 * 上 1/3 处，一次性；流式填满垫片 → 撤垫片并开启贴底跟随；
 * 跟随开启时新内容贴底；用户任何向上滚动手势（wheel）立即关跟随；
 * 滚回底部（<80px）再开；切换会话全部重置。
 *
 * items 混合三类条目（升序渲染，id = agentRuntimeId:eventSeq 十进制字符串）：
 * 普通消息、TranscriptCompacted 可折叠 marker、安全 terminal marker。
 * 向上分页：滚动接近顶部且 hasOlder 时调 onLoadOlder；旧页 prepend 后
 * 用预先记录的 scrollHeight 差值恢复滚动位置（不跳动）。
 *
 * 两种布局模式（单一 DOM 树，节点跨模式保留，composer 不 remount）。
 * 模式由 centeredEmpty = 无条目且 conversationId 为 null 决定——居中
 * 只属于"新对话空态"；会话切换/恢复途中的暂态空不翻转布局，composer
 * 全程钉在底部（杜绝历史会话间切换的中心 ⇄ 底部跳动）：
 * - 空态（新对话无条目）：composer 包装器脱离文档流（absolute inset-0 +
 *   items-center justify-center + pointer-events-none，内部恢复 auto），
 *   恒定容器正中心；欢迎语独立锚定在中线上方（bottom: 50% + 2rem），
 *   其高度/有无及未来新增内容都不改变 composer 位置。宽度与稳态一致。
 * - 稳态（有条目，或任何已有会话）：消息流 + composer sticky 底部。
 *
 * 双向过场（GSAP FLIP）：过场动作的判定全部在 thread-transition.ts 的
 * 纯函数里（render 相位与 GSAP 相位共用同一结论），本组件只负责播放。
 * passive effect 在每次提交后（绘制之后、布局干净，不产生 forced reflow）
 * 记录 composer rect；welcome 只在可见时记录。播放规则：
 * - 空 → 稳态（当前对话发出第一条消息，forward-flip）：composer 从中心
 *   滑到底部（y: 差值 → 0，expo.out 0.55s），同一 timeline 欢迎语 fixed
 *   在旧位置淡出上移、消息区淡入。sendVersion 信号使首发后的 null → 新
 *   runtime id 被视为同一对话的确立而非切换。
 * - 稳态 → 空（点新对话，reverse-flip）：同参数镜像滑回。
 * - 会话切换（switch/settle）：一律不播位置动画，直接落目标布局；
 *   切换/首发确立后的恢复填充翻转被抑制，不重播过场。
 * 完成后 clearProps 交还 CSS。打断：killTweensOf + settle 清理内联残留；
 * prefers-reduced-motion 全程瞬时。
 *
 * 会话切换过场（isSwitching）：切换期间新内容保持透明（渲染期内联 opacity 0
 * + layout phase gsap.set 兜底，挡住 recovering 中间帧），旧内容随 React 替换
 * 即时消失（无快照覆盖层——快照层无法复现旧滚动位置，且会透出底层新内容）。
 * 恢复结束后滚动落底、内容纯淡入（无位移，避免"从上往下飘"的二次定位感）。
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
  /** 当前会话 id（AgentRuntimeId）；变化 = 切换会话，一律不播位置动画 */
  conversationId?: string | null
  /** 用户发送递增信号；首发后的 null → 新 runtime id 视为同一对话的确立 */
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
  // 居中布局只属于"新对话空态"（无会话且无条目）。会话切换/恢复途中的
  // 暂态空不触发居中：composer 全程钉在底部，杜绝历史会话间切换时
  // 中心 ⇄ 底部的布局跳动；welcome 也只在 genuinely 新对话时出现
  const centeredEmpty = isEmpty && conversationId == null
  const rootRef = useRef<HTMLDivElement>(null)
  const viewportRef = useRef<HTMLDivElement>(null)
  const welcomeRef = useRef<HTMLDivElement>(null)
  const messagesRef = useRef<HTMLDivElement>(null)
  const composerRef = useRef<HTMLDivElement>(null)
  // FLIP first 帧：每次提交后记录，切换模式时即为旧布局位置（双向都需要）
  const prevComposerRectRef = useRef<DOMRect | null>(null)
  const prevWelcomeRectRef = useRef<DOMRect | null>(null)
  // 向上分页的滚动锚点：触发加载时记录，prepend 完成后按高度差恢复位置
  const prependAnchorRef = useRef<{
    scrollHeight: number
    scrollTop: number
  } | null>(null)
  const [nearBottom, setNearBottom] = useState(true)
  // 已锚定的 sendVersion：每次发送只锚定一次（1/3 处），流式增长不再贴底
  const anchoredSendRef = useRef(0)
  // 发送锚定的垫片（px）：发送时内容不足以把用户消息推到 1/3 处，
  // 在消息列底部垫出可滚空间；流式内容填满后自动撤掉。
  // 会话切换时经 derive-state-during-render 整体重置
  const [sendAnchor, setSendAnchor] = useState<{
    conversationId: string | null | undefined
    target: number | null
    spacer: number
  }>({ conversationId, target: null, spacer: 0 })
  if (sendAnchor.conversationId !== conversationId) {
    setSendAnchor({ conversationId, target: null, spacer: 0 })
  }

  // 过场信号的单一事实源（纯函数归约，详见 thread-transition.ts）：
  // render 相位（布局/welcome 可见性/switching 透明度）与 GSAP effect
  // 相位（编排播放）共用同一份结论，不再各自维护 prev/ref 镜像。
  // derive-state-during-render：信号变化时渲染期条件 setState，立即重渲染
  // 提交；动画完成的清除走函数式更新（不影响 action 结论）
  const [transition, setTransition] = useState(initialTransitionState)
  const reduced = reduceThreadTransition(transition, {
    conversationId,
    sendVersion,
    isEmpty,
  })
  if (reduced !== transition) setTransition(reduced)
  const { action, switching: isSwitching, leavingEmpty } = reduced

  // 发送锚定，相位 1（计算）：发送时用户消息先落位；内容不足以把它推到
  // 视口上 1/3 处时，在消息列底部垫出缺失的滚动空间（垫片）。锚定等
  // FLIP/切换过场结束才测量（过早量出的位置是错的）。滚动不在此执行——
  // 垫片随 setState 下一帧才进 DOM，立刻滚动会因最大滚动高度不足被钳住
  useEffect(() => {
    const viewport = viewportRef.current
    if (!viewport) return
    if (
      sendVersion === undefined ||
      sendVersion <= anchoredSendRef.current ||
      isSwitching ||
      leavingEmpty
    )
      return
    const userMessages = viewport.querySelectorAll(
      '[data-slot="user-message"]'
    )
    const last = userMessages[userMessages.length - 1]
    if (last === undefined) return
    anchoredSendRef.current = sendVersion
    const target = Math.max(
      0,
      viewport.scrollTop +
        last.getBoundingClientRect().top -
        viewport.getBoundingClientRect().top -
        viewport.clientHeight / 3
    )
    setSendAnchor({
      conversationId,
      target,
      spacer: Math.max(
        0,
        target - (viewport.scrollHeight - viewport.clientHeight)
      ),
    })
  }, [items, sendVersion, conversationId, isSwitching, leavingEmpty])

  // 发送锚定，相位 2（执行）：垫片已入 DOM、目标可达后平滑滚动到位
  //（GSAP base 档，reduced-motion 瞬时），完成后按最终位置判定近底
  useEffect(() => {
    const viewport = viewportRef.current
    const target = sendAnchor.target
    if (!viewport || target === null) return
    gsap.to(viewport, {
      scrollTo: { y: target },
      duration: motionDuration(MOTION_DURATION.base),
      ease: MOTION_EASE.enter,
      overwrite: "auto",
      onComplete: () =>
        setNearBottom(
          viewport.scrollHeight - target - viewport.clientHeight <
            NEAR_BOTTOM_THRESHOLD
        ),
    })
  }, [sendAnchor])

  // 垫片自持 + 近底跟随：流式内容填满垫片（天然可达锚点）即撤掉并恢复
  // 贴底跟随——新文段触底后继续自动滚动；否则仅在用户近底时跟随，
  // 上翻不打扰
  useEffect(() => {
    const viewport = viewportRef.current
    if (!viewport) return
    if (sendAnchor.spacer > 0 && sendAnchor.target !== null) {
      const naturalMax =
        viewport.scrollHeight - sendAnchor.spacer - viewport.clientHeight
      if (naturalMax >= sendAnchor.target) {
        setSendAnchor({ conversationId, target: null, spacer: 0 })
        setNearBottom(true)
      }
    }
    if (!nearBottom) return
    viewport.scrollTo({ top: viewport.scrollHeight, behavior: "auto" })
  }, [items, nearBottom, sendAnchor, conversationId])

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

  // 空态 ⇄ 稳态过场（纯播放器：动作结论全部来自 thread-transition 归约，
  // 这里不再做任何信号判定）。规则：同一会话内的空 ⇄ 非空翻转才播
  // 位置动画；会话切换一律直落目标布局
  useGSAP(
    () => {
      const composerElement = composerRef.current
      const welcomeElement = welcomeRef.current
      const messageList = messagesRef.current
      if (!composerElement) return
      const targets = [composerElement, welcomeElement, messageList].filter(
        (element): element is HTMLDivElement => element !== null
      )
      const reduce = prefersReducedMotion()
      gsap.killTweensOf(targets)

      // 完成/中止都要交还内联样式（composer translateY、welcome fixed
      // 幽灵态、消息列表中间 opacity）并释放欢迎语保持位
      const settle = () => {
        gsap.set(targets, { clearProps: "all" })
        setTransition((current) =>
          current.leavingEmpty ? { ...current, leavingEmpty: false } : current
        )
      }

      // 切换/恢复填充/无翻转：不播位置动画；killTweensOf 已中断在播
      // tween，这里只清理内联残留
      if (action !== "forward-flip" && action !== "reverse-flip") {
        settle()
        return
      }

      // 双向同构的编排：composer 滑动与 welcome 淡入/淡出同时长、同缓动、
      // 同起点（都挂在 timeline 0 位），方向只决定 y 差值与淡入/淡出
      const timeline = gsap.timeline({
        defaults: { ease: CHOREO.ease },
        onComplete: settle,
      })

      // FLIP：composer 从旧位置滑到新位置（deltaY 符号即方向）
      const first = prevComposerRectRef.current
      const last = composerElement.getBoundingClientRect()
      const deltaY = first ? first.top - last.top : 0
      if (deltaY !== 0)
        timeline.fromTo(
          composerElement,
          { y: deltaY },
          { y: 0, duration: reduce ? 0 : CHOREO.composerDuration },
          0
        )

      if (action === "reverse-flip") {
        if (welcomeElement)
          timeline.fromTo(
            welcomeElement,
            { autoAlpha: 0 },
            { autoAlpha: 1, duration: reduce ? 0 : CHOREO.welcomeDuration },
            0
          )
        return
      }

      // forward-flip：欢迎语固定在旧位置淡出（稳态下它是 hidden，临时复现），
      // 消息区淡入
      const firstWelcome = prevWelcomeRectRef.current
      if (welcomeElement && firstWelcome) {
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
    },
    {
      scope: rootRef,
      dependencies: [isEmpty, conversationId, sendVersion, action],
    }
  )

  // 会话切换过场：新内容在渲染期保持透明（style opacity 0），等恢复结束、
  // 滚动落底后纯淡入（无位移——位置动画是"从上往下飘"的来源）。
  // 旧内容随 React 替换即时消失，不做快照覆盖层。
  useGSAP(
    () => {
      if (!isSwitching) return
      const targets = [welcomeRef.current, messagesRef.current].filter(
        (element): element is HTMLDivElement => element !== null
      )
      if (targets.length === 0) return
      // 整个切换期（含 recovering 的多次重渲染）保持隐藏：FLIP 的 settle
      // clearProps 会清掉渲染期内联的 opacity 0，这里在 layout phase 兜底
      gsap.set(targets, { autoAlpha: 0 })
      if (recovering) return
      const reduce = prefersReducedMotion()
      // 历史会话进场落在底部（最新消息）；切换后总是回到近底态
      const viewport = viewportRef.current
      if (viewport && !isEmpty)
        viewport.scrollTo({ top: viewport.scrollHeight, behavior: "auto" })
      setNearBottom(true)
      gsap.fromTo(
        targets,
        { autoAlpha: 0 },
        {
          autoAlpha: 1,
          duration: reduce ? 0 : MOTION_DURATION.fast,
          ease: MOTION_EASE.enter,
          overwrite: "auto",
          onComplete: () =>
            setTransition((current) =>
              current.switching ? { ...current, switching: false } : current
            ),
        }
      )
    },
    { scope: rootRef, dependencies: [isSwitching, recovering, isEmpty] }
  )

  const handleScroll = () => {
    const viewport = viewportRef.current
    if (!viewport) return
    const gap =
      viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight
    setNearBottom(gap < NEAR_BOTTOM_THRESHOLD)

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
    const viewport = viewportRef.current
    viewport?.scrollTo({
      top: viewport.scrollHeight - sendAnchor.spacer,
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
        onWheel={(event) => {
          // 用户向上滚动手势立即关掉贴底跟随（程序滚动不触发 wheel）——
          // 否则流式每帧贴底会把用户拽回去，"向上翻"永远失效
          if (event.deltaY < 0) setNearBottom(false)
        }}
        className="relative flex flex-1 flex-col overflow-x-hidden overflow-y-auto"
      >
        <div className="relative mx-auto flex w-full max-w-[44rem] flex-1 flex-col px-4 pt-6">
          <div
            ref={welcomeRef}
            style={{ opacity: isSwitching ? 0 : undefined }}
            className={cn(
              centeredEmpty || leavingEmpty
                ? "absolute inset-x-0 bottom-[calc(50%+2rem)]"
                : "hidden"
            )}
          >
            {welcome}
          </div>

          <div
            ref={messagesRef}
            style={{ opacity: isSwitching ? 0 : undefined }}
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

          {/* 发送锚定垫片：流式回复的生长空间（填满即撤），见上方 effect */}
          {sendAnchor.spacer > 0 ? (
            <div aria-hidden style={{ height: sendAnchor.spacer }} />
          ) : null}

          {composer ? (
            <div
              className={cn(
                "flex flex-col gap-2",
                centeredEmpty
                  ? // 空态浮层透明（不能带 bg-background，否则盖住 welcome）
                    "pointer-events-none absolute inset-0 items-center justify-center"
                  : "sticky bottom-0 mt-auto bg-background pb-4 md:pb-6"
              )}
            >
              {!centeredEmpty && !nearBottom ? (
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
                className={cn(centeredEmpty && "pointer-events-auto w-full")}
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
