"use client"

import { memo, useEffect, useState } from "react"
import { ChevronsLeft, ChevronsRight, MessageCircle, SquarePen } from "lucide-react"

import type { ConversationThreadMeta } from "@/components/stratum/conversation/types"
import { cn } from "@/lib/utils"

/**
 * ThreadListRail —— 紧凑会话栏：默认收成一条图标列（卡片浮于消息区左上，
 * 消息列保持视口居中），点开关向右展开出完整标题；点选会话后立即收回。
 * light 选中态使用稀缺 sage accent，dark 保留 primary 染底；收起时也有
 * 形状与颜色双重标记。无遮罩，Esc 收回。
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

  // 开关与列表项共用同一几何；窄屏保留 44px 触控目标，桌面收紧为 32px。
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
        "flex h-11 w-full items-center gap-2 rounded-lg px-1.5 text-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/60 sm:h-8",
        active
          ? "bg-accent/60 text-accent-foreground dark:bg-primary/15 dark:text-primary"
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
        "absolute top-2 left-3 z-10 flex flex-col gap-1 overflow-hidden rounded-2xl border border-border bg-card p-1.5 shadow-sm transition-[width] duration-300 ease-out dark:bg-card/95 dark:shadow-xl dark:backdrop-blur",
        expanded ? "w-64" : "w-14 sm:w-11",
        className
      )}
    >
      <div className="flex items-center justify-between">
        <button
          type="button"
          aria-label={expanded ? "收起会话列表" : "展开会话列表"}
          onClick={() => setExpanded((v) => !v)}
          className="flex h-11 w-full items-center gap-2 rounded-lg px-1.5 text-sm text-muted-foreground outline-none transition-colors hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/60 sm:h-8 dark:hover:text-primary"
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
