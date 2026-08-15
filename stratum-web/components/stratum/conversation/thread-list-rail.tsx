"use client"

import { memo, useEffect, useState } from "react"
import { ChevronsLeft, ChevronsRight, MessageCircle, SquarePen } from "lucide-react"

import type { ConversationThreadMeta } from "@/components/stratum/conversation/types"
import { cn } from "@/lib/utils"

/**
 * ThreadListRail —— 紧凑会话栏：默认收成一条图标列（卡片浮于消息区左上，
 * 消息列保持视口居中），点开关向右展开出完整标题；点选会话后立即收回。
 * 选中态用 primary 染底（与 dock 的 hover 语言一致：bg-primary/15 + text-primary），
 * 收起时也有颜色标记。无遮罩，Esc 收回。
 */
export const ThreadListRail = memo(function ThreadListRail({
  threads,
  activeId,
  onSelect,
  onNew,
  className,
}: {
  threads: ConversationThreadMeta[]
  activeId?: string
  onSelect?: (id: string) => void
  onNew?: () => void
  className?: string
}) {
  const [expanded, setExpanded] = useState(false)

  useEffect(() => {
    if (!expanded) return
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setExpanded(false)
    }
    window.addEventListener("keydown", onKeyDown)
    return () => window.removeEventListener("keydown", onKeyDown)
  }, [expanded])

  // 开关与列表项共用同一几何：h-8 + px-1.5 + size-5 图标槽，图标列严格对齐
  const iconSlot = "flex size-5 shrink-0 items-center justify-center"

  const row = (
    key: string,
    icon: React.ReactNode,
    label: string,
    {
      active,
      onClick,
      primary,
    }: { active?: boolean; onClick?: () => void; primary?: boolean }
  ) => (
    <button
      key={key}
      type="button"
      title={label}
      onClick={onClick}
      className={cn(
        "flex h-8 items-center gap-2 rounded-lg px-1.5 text-sm transition-colors",
        active
          ? "bg-primary/15 text-primary"
          : "text-muted-foreground hover:bg-muted/60 hover:text-foreground"
      )}
    >
      <span className={cn(iconSlot, primary && "text-primary")}>{icon}</span>
      <span
        className={cn(
          "truncate whitespace-nowrap transition-opacity duration-200",
          expanded ? "opacity-100" : "opacity-0"
        )}
      >
        {label}
      </span>
    </button>
  )

  return (
    <div
      data-slot="thread-list-rail"
      className={cn(
        "absolute top-2 left-3 z-10 flex flex-col gap-1 overflow-hidden rounded-2xl border border-border bg-card/95 p-1.5 shadow-xl backdrop-blur transition-[width] duration-300 ease-out",
        expanded ? "w-64" : "w-11",
        className
      )}
    >
      <div className="flex items-center justify-between">
        <button
          type="button"
          aria-label={expanded ? "收起会话列表" : "展开会话列表"}
          onClick={() => setExpanded((v) => !v)}
          className="flex h-8 items-center gap-2 rounded-lg px-1.5 text-sm text-muted-foreground transition-colors hover:text-primary"
        >
          <span className={iconSlot}>
            {expanded ? (
              <ChevronsLeft aria-hidden className="size-4" />
            ) : (
              <ChevronsRight aria-hidden className="size-4" />
            )}
          </span>
          <span
            className={cn(
              "truncate whitespace-nowrap transition-opacity duration-200",
              expanded ? "opacity-100" : "opacity-0"
            )}
          >
            会话
          </span>
        </button>
        {expanded ? (
          <p className="pr-2 font-mono text-xs text-muted-foreground">
            {threads.length}
          </p>
        ) : null}
      </div>

      {row("new", <SquarePen aria-hidden className="size-4" />, "新对话", {
        primary: true,
        onClick: () => onNew?.(),
      })}

      {threads.map((thread) =>
        row(
          thread.id,
          <MessageCircle aria-hidden className="size-4" />,
          thread.title,
          {
            active: thread.id === activeId,
            onClick: () => onSelect?.(thread.id),
          }
        )
      )}
    </div>
  )
})
