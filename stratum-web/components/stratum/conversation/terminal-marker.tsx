"use client"

import { memo } from "react"
import { CircleAlert, OctagonX } from "lucide-react"

import { cn } from "@/lib/utils"

/**
 * TerminalMarker —— 安全 terminal marker（LoopFailed / LoopCancelled）。
 * 居中的克制单行标记：不伪造 assistant 文本或 tool result；finished 无
 * marker（最终 assistant 消息自然收尾）。
 */
export const TerminalMarker = memo(function TerminalMarker({
  terminal,
  errorText,
  className,
}: {
  terminal: "failed" | "cancelled"
  errorText: string | null
  className?: string
}) {
  const failed = terminal === "failed"
  return (
    <div
      data-slot="terminal-marker"
      role="status"
      className={cn(
        "flex items-center justify-center gap-1.5 px-2 text-xs",
        failed ? "text-destructive" : "text-muted-foreground",
        className
      )}
    >
      {failed ? (
        <CircleAlert aria-hidden className="size-3.5 shrink-0" />
      ) : (
        <OctagonX aria-hidden className="size-3.5 shrink-0" />
      )}
      <span className="min-w-0 truncate">
        {failed ? (errorText ?? "执行失败") : "已取消"}
      </span>
    </div>
  )
})
