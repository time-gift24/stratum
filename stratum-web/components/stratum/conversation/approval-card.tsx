"use client"

import { Check, Loader2, ShieldAlert, X } from "lucide-react"

import type { ToolCallApproval } from "@/components/stratum/conversation/types"
import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * ApprovalCard —— 工具审批卡片的共享内容组件：浮层（composer 上方
 * ApprovalDock，传 onResolve）与内联工具块（不传 onResolve，只读）共用。
 *
 * 信息：工具名 + 读/写类型 + dangerLevel 编码（high → destructive，
 * medium → port-image 蓝，low → 中性，均配中文文案，不只靠颜色）+
 * 可选参数摘要。交互模式显示「允许/拒绝」（submitting 禁用防重复）；
 * 只读模式显示状态：待决"等待审批…"，已决"已批准/已拒绝"终态。
 */

const DANGER_LABEL: Record<ToolCallApproval["dangerLevel"], string> = {
  low: "低风险",
  medium: "需注意",
  high: "高风险",
}

export function ApprovalCard({
  approval,
  argumentsText,
  onResolve,
  className,
}: {
  approval: ToolCallApproval
  /** 参数摘要（JSON 文本）；不传则不显示 */
  argumentsText?: string
  /** 传入为交互模式（浮层）；不传为只读状态展示（内联工具块） */
  onResolve?: (approvalId: string, decision: "approve" | "reject") => void
  className?: string
}) {
  const interactive = onResolve !== undefined
  const high = approval.dangerLevel === "high"
  const medium = approval.dangerLevel === "medium"
  const pendingLike =
    approval.status === "pending" || approval.status === "submitting"
  const dangerColor = high
    ? "text-destructive"
    : medium
      ? "text-port-image"
      : "text-muted-foreground"

  return (
    <div
      data-slot="approval-card"
      className={cn(
        "rounded-lg border px-3 py-2.5",
        high && pendingLike
          ? "border-destructive/50 bg-destructive/10"
          : "border-border bg-muted/40",
        className
      )}
    >
      <div className="flex items-center gap-1.5 text-xs">
        <ShieldAlert aria-hidden className={cn("size-3.5 shrink-0", dangerColor)} />
        <span className="min-w-0 flex-1 truncate font-mono text-foreground">
          {approval.toolName}
        </span>
        <span className={cn("shrink-0", dangerColor)}>
          {approval.toolKind === "write" ? "写入" : "读取"} ·{" "}
          {DANGER_LABEL[approval.dangerLevel]}
        </span>
      </div>

      {argumentsText ? (
        <pre className="mt-1.5 max-h-16 overflow-y-auto wrap-break-word whitespace-pre-wrap rounded-md bg-muted/50 p-2 font-mono text-xs text-foreground/90">
          {argumentsText}
        </pre>
      ) : null}

      {interactive ? (
        <div className="mt-2 flex items-center gap-1.5">
          <Button
            size="xs"
            className="rounded-full"
            disabled={approval.status === "submitting"}
            onClick={() => onResolve(approval.approvalId, "approve")}
          >
            允许
          </Button>
          <Button
            size="xs"
            variant="outline"
            className="rounded-full"
            disabled={approval.status === "submitting"}
            onClick={() => onResolve(approval.approvalId, "reject")}
          >
            拒绝
          </Button>
          {approval.status === "submitting" ? (
            <span className="text-xs text-muted-foreground">提交中…</span>
          ) : null}
        </div>
      ) : (
        <p className="mt-1.5 flex items-center gap-1.5 text-xs text-muted-foreground">
          {pendingLike ? (
            <>
              <Loader2
                aria-hidden
                className="size-3.5 shrink-0 animate-spin text-port-image motion-reduce:animate-none"
              />
              等待审批…
            </>
          ) : approval.status === "approved" ? (
            <>
              <Check aria-hidden className="size-3.5 shrink-0" />
              已批准
            </>
          ) : (
            <>
              <X aria-hidden className="size-3.5 shrink-0" />
              已拒绝
            </>
          )}
        </p>
      )}
    </div>
  )
}
