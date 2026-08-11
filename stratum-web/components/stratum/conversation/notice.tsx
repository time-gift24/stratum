"use client"

import { memo } from "react"
import type { LucideIcon } from "lucide-react"

import { cn } from "@/lib/utils"

/**
 * Notice —— 会话页所有状态提示的统一展示：左对齐 tinted 横幅，
 * 结构恒定（图标 + 左对齐正文 + 可选尾部动作），只换色调。
 *
 * tone 语义：
 * - error：destructive 红——失败/中断（生成中断、执行失败、连接错误）
 * - warning：warning 黄——需要知晓的进行中/降级状态（恢复执行、实时降级）
 * - neutral：muted——取消类终态与纯信息（已取消、取消请求已发送）
 */

const TONE = {
  error: {
    container: "border-destructive/40 bg-destructive/10",
    icon: "text-destructive",
  },
  warning: {
    container: "border-warning/40 bg-warning/10",
    icon: "text-warning",
  },
  neutral: {
    container: "border-border bg-muted",
    icon: "text-muted-foreground",
  },
} as const

export type NoticeTone = keyof typeof TONE

export const Notice = memo(function Notice({
  tone = "neutral",
  icon: Icon,
  action,
  children,
  className,
}: {
  tone?: NoticeTone
  icon?: LucideIcon
  /** 尾部动作（如重试/恢复按钮） */
  action?: React.ReactNode
  children: React.ReactNode
  className?: string
}) {
  return (
    <div
      data-slot="notice"
      role="status"
      className={cn(
        "flex items-center gap-2 rounded-lg border px-3 py-2 text-sm",
        TONE[tone].container,
        className
      )}
    >
      {Icon ? (
        <Icon aria-hidden className={cn("size-4 shrink-0", TONE[tone].icon)} />
      ) : null}
      <div className="min-w-0 flex-1 text-muted-foreground">{children}</div>
      {action}
    </div>
  )
})
