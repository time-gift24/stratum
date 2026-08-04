"use client"

import { useState } from "react"
import { ChevronDown, Loader2, Wrench } from "lucide-react"

import { ToolCall } from "@/components/stratum/conversation/tool-call"
import type { ConversationToolCall } from "@/components/stratum/conversation/types"
import { cn } from "@/lib/utils"

/**
 * ToolGroup —— 连续工具调用的分组容器（assistant-ui tool-group 底稿的数据驱动
 * fork，outline 变体）。单个调用直接渲染 ToolCall；多个调用收成一组
 * （"使用了 N 个工具"），组内可逐个展开。默认折叠；任一调用有待决审批时
 * 组强制展开（审批必须直接可见可操作），用户未手动操作时跟随该推导。
 */

export function ToolGroup({
  calls,
  className,
}: {
  calls: ConversationToolCall[]
  className?: string
}) {
  // null = 用户未手动操作，组跟随审批状态自动推导
  const [userOpen, setUserOpen] = useState<boolean | null>(null)
  const hasPendingApproval = calls.some(
    (call) =>
      call.approval?.status === "pending" ||
      call.approval?.status === "submitting"
  )
  const open = userOpen ?? hasPendingApproval
  const active = calls.some((call) => call.status === "streaming")

  if (calls.length === 0) return null

  if (calls.length === 1) {
    return <ToolCall call={calls[0]} className={className} />
  }

  return (
    <div
      data-slot="tool-group"
      className={cn("rounded-lg border border-border", className)}
    >
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setUserOpen((prev) => !(prev ?? hasPendingApproval))}
        className={cn(
          "flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm outline-none transition-colors",
          "hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50"
        )}
      >
        {active ? (
          <Loader2
            aria-hidden
            className="size-3.5 shrink-0 animate-spin text-port-image motion-reduce:animate-none"
          />
        ) : (
          <Wrench aria-hidden className="size-3.5 shrink-0 text-muted-foreground" />
        )}
        <span className="relative inline-block flex-1 text-xs text-muted-foreground">
          <span>使用了 {calls.length} 个工具</span>
          {active ? (
            <span
              aria-hidden
              className="shimmer pointer-events-none absolute inset-0 text-port-image motion-reduce:animate-none"
            >
              使用了 {calls.length} 个工具
            </span>
          ) : null}
        </span>
        <ChevronDown
          aria-hidden
          className={cn(
            "size-4 shrink-0 text-muted-foreground transition-transform duration-200 motion-reduce:transition-none",
            open && "rotate-180"
          )}
        />
      </button>

      {open ? (
        <div className="flex flex-col gap-1.5 border-t border-border p-1.5">
          {calls.map((call) => (
            <ToolCall key={call.callId} call={call} />
          ))}
        </div>
      ) : null}
    </div>
  )
}
