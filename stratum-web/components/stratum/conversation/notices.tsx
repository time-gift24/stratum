"use client"

import { memo } from "react"
import { CloudOff, Play } from "lucide-react"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * 会话状态提示面（composer 上方、文档流内的小横幅，restrained、token-only）：
 * - ResumeNotice：`resume_required` advisory —— Turn 仍为 running 但当前进程
 *   未托管；显式「恢复执行」按钮，绝不自动 resume。
 * - RealtimeDegradedNotice：NATS 不可用导致 realtime 降级；核心命令与 PG
 *   reconcile 继续工作，不做成错误横幅。
 */

export const ResumeNotice = memo(function ResumeNotice({
  onResume,
  className,
}: {
  onResume: () => void
  className?: string
}) {
  return (
    <div
      data-slot="resume-notice"
      role="status"
      className={cn(
        "flex items-center gap-2 rounded-xl border border-border bg-card px-3 py-2 text-sm shadow-sm",
        className
      )}
    >
      <p className="min-w-0 flex-1 text-muted-foreground">
        执行在当前环境已暂停，恢复后将继续。
      </p>
      <Button
        size="xs"
        className="shrink-0 rounded-full"
        onClick={onResume}
      >
        <Play aria-hidden />
        恢复执行
      </Button>
    </div>
  )
})

export const RealtimeDegradedNotice = memo(function RealtimeDegradedNotice({
  className,
}: {
  className?: string
}) {
  return (
    <p
      data-slot="realtime-degraded-notice"
      role="status"
      className={cn(
        "flex items-center gap-1.5 px-2 text-xs text-muted-foreground",
        className
      )}
    >
      <CloudOff aria-hidden className="size-3.5 shrink-0" />
      实时连接降级，将定期同步最新进展
    </p>
  )
})
