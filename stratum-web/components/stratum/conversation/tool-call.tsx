"use client"

import { useMemo, useState } from "react"
import { ChevronDown, CircleAlert, Loader2, Wrench } from "lucide-react"

import { ApprovalCard } from "@/components/stratum/conversation/approval-card"
import {
  ExcalidrawResult,
  parseExcalidrawScene,
} from "@/components/stratum/conversation/excalidraw-result"
import type { ConversationToolCall } from "@/components/stratum/conversation/types"
import { cn } from "@/lib/utils"

/**
 * ToolCall —— 单个工具调用块（assistant-ui tool-fallback 底稿的数据驱动 fork，
 * 不用 runtime/MessagePrimitive）。渐进式透明：默认折叠为 trigger 行
 * （状态图标 + 工具名，streaming 时转圈 + shimmer），展开后显示
 * 审批状态（如有）/ 参数 / 结果 / 错误。
 *
 * 结果分发：excalidraw_render 的结果通过最小 scene 形状校验时渲染为只读
 * 白板（ExcalidrawResult），其余工具结果一律原始 JSON 文本。
 *
 * 审批：内联只读展示（ApprovalCard 不传 onResolve——待决"等待审批…"，
 * 已决"已批准/已拒绝"）；允许/拒绝操作在 composer 上方的 ApprovalDock 浮层。
 * 待决审批时块仍强制展开，状态保持可见。
 */

export function ToolCall({
  call,
  className,
}: {
  call: ConversationToolCall
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const approval = call.approval
  const approvalActive =
    approval?.status === "pending" || approval?.status === "submitting"
  const expanded = open || approvalActive
  // JSON.parse 只在结果变化时重算（excalidraw 分发判定）
  const excalidraw = useMemo(
    () => (isExcalidrawScene(call) ? call.result : null),
    [call]
  )

  return (
    <div
      data-slot="tool-call"
      className={cn(
        "rounded-lg border",
        approval?.dangerLevel === "high" && approvalActive
          ? "border-destructive/50"
          : "border-border",
        className
      )}
    >
      <button
        type="button"
        aria-expanded={expanded}
        onClick={() => setOpen((value) => !value)}
        className={cn(
          "flex w-full items-center gap-2 rounded-lg px-2.5 py-1.5 text-left text-sm transition-colors outline-none",
          "hover:bg-muted/60 focus-visible:ring-2 focus-visible:ring-ring/50"
        )}
      >
        {call.status === "streaming" ? (
          <Loader2
            aria-hidden
            className="size-3.5 shrink-0 animate-spin text-port-image motion-reduce:animate-none"
          />
        ) : call.status === "failed" ? (
          <CircleAlert
            aria-hidden
            className="size-3.5 shrink-0 text-destructive"
          />
        ) : call.status === "interrupted" ? (
          <CircleAlert
            aria-hidden
            className="size-3.5 shrink-0 text-muted-foreground"
          />
        ) : (
          <Wrench
            aria-hidden
            className="size-3.5 shrink-0 text-muted-foreground"
          />
        )}
        <span className="relative inline-block min-w-0 flex-1 truncate font-mono text-xs text-foreground">
          <span>{call.name ?? "工具调用"}</span>
          {call.status === "streaming" ? (
            <span
              aria-hidden
              className="pointer-events-none absolute inset-0 shimmer truncate text-port-image motion-reduce:animate-none"
            >
              {call.name ?? "工具调用"}
            </span>
          ) : null}
        </span>
        <ChevronDown
          aria-hidden
          className={cn(
            "size-4 shrink-0 text-muted-foreground transition-transform duration-200 motion-reduce:transition-none",
            expanded && "rotate-180"
          )}
        />
      </button>

      {expanded ? (
        <div className="flex flex-col gap-2 border-t border-border px-2.5 py-2">
          {approval ? <ApprovalCard approval={approval} /> : null}
          {call.argumentsText !== "" ? (
            <ToolCallSection label="参数" text={call.argumentsText} />
          ) : null}
          {call.errorText !== null ? (
            <ToolCallSection label="错误" text={call.errorText} destructive />
          ) : call.result !== null ? (
            excalidraw !== null ? (
              <ExcalidrawResult sceneText={excalidraw} />
            ) : (
              <ToolCallSection label="结果" text={call.result} />
            )
          ) : call.status === "interrupted" ? (
            <p className="text-xs text-muted-foreground">
              执行中断，未返回结果。
            </p>
          ) : null}
        </div>
      ) : null}
    </div>
  )
}

/** excalidraw_render 结果且通过最小 scene 形状校验时，分发为只读白板渲染 */
function isExcalidrawScene(call: ConversationToolCall): boolean {
  return (
    call.name === "excalidraw_render" &&
    call.result !== null &&
    parseExcalidrawScene(call.result) !== null
  )
}

function ToolCallSection({
  label,
  text,
  destructive = false,
}: {
  label: string
  text: string
  destructive?: boolean
}) {
  return (
    <div data-slot="tool-call-section">
      <p
        className={cn(
          "text-xs font-medium",
          destructive ? "text-destructive" : "text-muted-foreground"
        )}
      >
        {label}
      </p>
      <pre className="mt-1 max-h-48 overflow-y-auto rounded-md bg-muted/50 p-2.5 font-mono text-xs wrap-break-word whitespace-pre-wrap text-foreground/90">
        {text}
      </pre>
    </div>
  )
}
