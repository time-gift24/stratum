"use client"

import { useEffect, useRef, useState } from "react"
import { ArrowDown } from "lucide-react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import {
  AssistantMessage,
  UserMessage,
} from "@/components/stratum/conversation/message"
import type { ConversationMessage } from "@/components/stratum/conversation/types"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

gsap.registerPlugin(useGSAP)

/**
 * ConversationThread —— 对话消息列表（assistant-ui thread 底稿的展示层 fork）。
 * 结构：可滚动 viewport（消息列居中，max-w 44rem）+ composer。数据驱动：
 * messages 由调用方持有；滚动到底部仅在用户本就近底时自动跟随，上翻后不打扰。
 *
 * 两种布局模式（单一 DOM 树，节点跨模式保留，composer 不 remount）：
 * - 空态（无消息）：列 justify-center + composer 去掉 sticky/mt-auto，
 *   欢迎语 + composer 作为整体垂直水平居中（Gemini 式开场），宽度与稳态一致。
 * - 稳态（有消息）：消息流 + composer sticky 底部。
 *
 * 首发过场（空 → 有消息，GSAP FLIP）：每次提交后记录 composer/welcome 的
 * 位置（rect），模式切换后手动 invert——composer 从旧位置平移到新位置
 * （y: 差值 → 0，expo.out 0.55s），同一 timeline 里欢迎语 fixed 在旧位置
 * 淡出上移、消息区淡入；完成后 clearProps 交还 CSS。回空态不做反向动画，
 * 仅快速淡入。打断：killTweensOf + clearProps；prefers-reduced-motion 全程瞬时。
 */
export function ConversationThread({
  messages,
  composer,
  welcome,
  onReload,
  onEditUserMessage,
  onRetryUserMessage,
  className,
}: {
  messages: ConversationMessage[]
  /** 底部 sticky 区域（如 PromptInput） */
  composer?: React.ReactNode
  /** 空状态内容（messages 为空时居中展示） */
  welcome?: React.ReactNode
  onReload?: (message: ConversationMessage) => void
  onEditUserMessage?: (message: ConversationMessage) => void
  /** 用户消息发送失败时的重发入口 */
  onRetryUserMessage?: (message: ConversationMessage) => void
  className?: string
}) {
  const isEmpty = messages.length === 0
  const rootRef = useRef<HTMLDivElement>(null)
  const viewportRef = useRef<HTMLDivElement>(null)
  const welcomeRef = useRef<HTMLDivElement>(null)
  const messagesRef = useRef<HTMLDivElement>(null)
  const composerRef = useRef<HTMLDivElement>(null)
  const prevIsEmptyRef = useRef<boolean | null>(null)
  // FLIP first 帧：每次提交后记录，切换模式时即为旧布局位置
  const prevComposerRectRef = useRef<DOMRect | null>(null)
  const prevWelcomeRectRef = useRef<DOMRect | null>(null)
  const [nearBottom, setNearBottom] = useState(true)

  // 近底时新内容（含流式增长）瞬时贴底——流式期间每 30ms 一帧，
  // smooth 会反复重启动画造成滞后抖动；平滑只留给用户主动点的「回到底部」
  useEffect(() => {
    const viewport = viewportRef.current
    if (!viewport || !nearBottom) return
    viewport.scrollTo({ top: viewport.scrollHeight, behavior: "auto" })
  }, [messages, nearBottom])

  // 记录 FLIP first 帧：rect 只在空态（居中布局）时有意义，流式热路径不读
  // （deps 收窄为 isEmpty，避免每个 token 一次 forced reflow）。welcome 只在
  // 可见时记录，隐藏态保留最后可见位置，供退场动画复现
  useEffect(() => {
    if (!isEmpty) return
    prevComposerRectRef.current =
      composerRef.current?.getBoundingClientRect() ?? null
    const welcomeElement = welcomeRef.current
    if (welcomeElement && welcomeElement.getClientRects().length > 0)
      prevWelcomeRectRef.current = welcomeElement.getBoundingClientRect()
  }, [isEmpty])

  // 空态 ⇄ 稳态过场
  useGSAP(
    () => {
      const wasEmpty = prevIsEmptyRef.current
      prevIsEmptyRef.current = isEmpty
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

      if (wasEmpty === true && !isEmpty) {
        // 首发过场：一次编排的 timeline（FLIP + 欢迎语退场 + 消息淡入）
        const first = prevComposerRectRef.current
        const last = composerElement.getBoundingClientRect()
        const deltaY = first ? first.top - last.top : 0
        const firstWelcome = prevWelcomeRectRef.current

        const timeline = gsap.timeline({
          defaults: { ease: "expo.out" },
          onComplete: () => gsap.set(targets, { clearProps: "all" }),
        })
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
            { autoAlpha: 0, y: -12, duration: reduce ? 0 : 0.35 },
            0
          )
        }
        if (deltaY !== 0)
          timeline.fromTo(
            composerElement,
            { y: deltaY },
            { y: 0, duration: reduce ? 0 : 0.55 },
            0
          )
        if (messageList)
          timeline.fromTo(
            messageList,
            { autoAlpha: 0 },
            { autoAlpha: 1, duration: reduce ? 0 : 0.4 },
            reduce ? 0 : 0.1
          )
      } else if (wasEmpty === false && isEmpty) {
        // 回空态（新建/切空会话）：清掉可能残留的过场内联样式，快速淡入
        gsap.set(targets, { clearProps: "all" })
        if (welcomeElement && !reduce)
          gsap.fromTo(
            welcomeElement,
            { autoAlpha: 0 },
            {
              autoAlpha: 1,
              duration: 0.25,
              onComplete: () =>
                gsap.set(welcomeElement, { clearProps: "opacity,visibility" }),
            }
          )
      }
    },
    { scope: rootRef, dependencies: [isEmpty] }
  )

  const handleScroll = () => {
    const viewport = viewportRef.current
    if (!viewport) return
    const gap =
      viewport.scrollHeight - viewport.scrollTop - viewport.clientHeight
    setNearBottom(gap < 80)
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
        <div
          className={cn(
            "mx-auto flex w-full max-w-[44rem] flex-1 flex-col px-4 pt-6",
            isEmpty && "justify-center"
          )}
        >
          <div ref={welcomeRef} className={cn("mb-6", !isEmpty && "hidden")}>
            {welcome}
          </div>

          <div
            ref={messagesRef}
            className="mb-14 flex flex-col gap-y-6 empty:hidden"
          >
            {messages.map((message, index) =>
              message.role === "user" ? (
                <UserMessage
                  key={message.id}
                  message={message}
                  onEdit={onEditUserMessage}
                  onRetry={onRetryUserMessage}
                />
              ) : (
                <AssistantMessage
                  key={message.id}
                  message={message}
                  isLast={index === messages.length - 1}
                  onReload={onReload}
                />
              )
            )}
          </div>

          {composer ? (
            <div
              className={cn(
                "flex flex-col gap-2 bg-background",
                isEmpty ? "w-full" : "sticky bottom-0 mt-auto pb-4 md:pb-6"
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
              <div ref={composerRef}>{composer}</div>
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )
}
