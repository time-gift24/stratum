"use client"

import { memo } from "react"
import { CloudOff, CirclePause, Play } from "lucide-react"

import { Notice } from "@/components/stratum/conversation/notice"
import { Button } from "@/components/ui/button"

/**
 * 会话状态提示面（composer 上方、文档流内的小横幅），统一走 Notice
 * （左对齐 tinted 横幅；warning 黄 = 需要知晓的进行中/降级状态）：
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
    <Notice
      tone="warning"
      icon={CirclePause}
      className={className}
      action={
        <Button size="xs" className="shrink-0 rounded-full" onClick={onResume}>
          <Play aria-hidden />
          恢复执行
        </Button>
      }
    >
      执行在当前环境已暂停，恢复后将继续。
    </Notice>
  )
})

export const RealtimeDegradedNotice = memo(function RealtimeDegradedNotice({
  className,
}: {
  className?: string
}) {
  return (
    <Notice tone="warning" icon={CloudOff} className={className}>
      实时连接降级，将定期同步最新进展。
    </Notice>
  )
})
