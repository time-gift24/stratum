"use client"

import { memo } from "react"
import { CircleAlert, OctagonX } from "lucide-react"

import { Notice } from "@/components/stratum/conversation/notice"

/**
 * TerminalMarker —— 安全 terminal marker（LoopFailed / LoopCancelled）。
 * 统一走 Notice（左对齐 tinted 横幅）：failed = error 红，cancelled =
 * neutral 灰；不伪造 assistant 文本或 tool result；finished 无
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
    <Notice
      tone={failed ? "error" : "neutral"}
      icon={failed ? CircleAlert : OctagonX}
      className={className}
    >
      {failed ? (errorText ?? "执行失败") : "已取消"}
    </Notice>
  )
})
