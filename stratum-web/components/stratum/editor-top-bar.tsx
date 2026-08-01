import * as React from "react"
import { ChevronLeft, ChevronRight, X } from "lucide-react"

import { cn } from "@/lib/utils"

/**
 * EditorTopBar —— 画布编辑器顶栏：三段式（左组 / 中央标题导航 / 右组）。
 * 内容由调用方组合（菜单、图标按钮、分享动作等）。
 */
function EditorTopBar({
  className,
  children,
  ...props
}: React.ComponentProps<"header">) {
  return (
    <header
      data-slot="editor-top-bar"
      className={cn(
        "relative flex h-11 items-center justify-between gap-3 rounded-xl border border-border bg-card px-2",
        className
      )}
      {...props}
    >
      {children}
    </header>
  )
}

function EditorTopBarGroup({
  className,
  children,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="editor-top-bar-group"
      className={cn("flex min-w-0 items-center gap-1", className)}
      {...props}
    >
      {children}
    </div>
  )
}

/**
 * EditorTopBarTitle —— 中央标题导航：上一步 / 标题 + 关闭 / 下一步。
 */
function EditorTopBarTitle({
  title,
  className,
}: {
  title: string
  className?: string
}) {
  return (
    <div
      data-slot="editor-top-bar-title"
      className={cn(
        "absolute left-1/2 flex h-full -translate-x-1/2 items-center gap-0.5",
        className
      )}
    >
      <button
        type="button"
        aria-label="上一个工作流"
        className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
      >
        <ChevronLeft aria-hidden className="size-3.5" />
      </button>
      <span className="flex items-center gap-2 rounded-md bg-muted px-3 py-1.5 font-sans text-xs font-medium">
        {title}
        <X aria-hidden className="size-3 text-muted-foreground" />
      </span>
      <button
        type="button"
        aria-label="下一个工作流"
        className="rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
      >
        <ChevronRight aria-hidden className="size-3.5" />
      </button>
    </div>
  )
}

export { EditorTopBar, EditorTopBarGroup, EditorTopBarTitle }
