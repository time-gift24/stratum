"use client"

import { useEffect, useRef, useState } from "react"
import { ArrowDown } from "lucide-react"

import {
  AssistantMessage,
  UserMessage,
} from "@/components/stratum/conversation/message"
import type { ConversationMessage } from "@/components/stratum/conversation/types"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * ConversationThread —— 对话消息列表（assistant-ui thread 底稿的展示层 fork）。
 * 结构：可滚动 viewport（消息列居中，max-w 44rem）+ sticky 底部（回到底部按钮 +
 * 调用方传入的 composer）。数据驱动：messages 由调用方持有；滚动到底部仅在
 * 用户本就近底时自动跟随（流式生成时平滑跟随），上翻后不打扰。
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
  const viewportRef = useRef<HTMLDivElement>(null)
  const [nearBottom, setNearBottom] = useState(true)

  // 近底时新内容（含流式增长）瞬时贴底——流式期间每 30ms 一帧，
  // smooth 会反复重启动画造成滞后抖动；平滑只留给用户主动点的「回到底部」
  useEffect(() => {
    const viewport = viewportRef.current
    if (!viewport || !nearBottom) return
    viewport.scrollTo({ top: viewport.scrollHeight, behavior: "auto" })
  }, [messages, nearBottom])

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
            messages.length === 0 && "justify-center"
          )}
        >
          {messages.length === 0 ? welcome : null}

          <div className="mb-14 flex flex-col gap-y-6 empty:hidden">
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
            <div className="sticky bottom-0 mt-auto flex flex-col gap-2 bg-background pb-4 md:pb-6">
              {!nearBottom ? (
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
              {composer}
            </div>
          ) : null}
        </div>
      </div>
    </div>
  )
}
