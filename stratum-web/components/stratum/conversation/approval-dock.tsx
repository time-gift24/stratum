"use client"

import { useCallback, useRef, useState } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import { ApprovalCard } from "@/components/stratum/conversation/approval-card"
import type { ToolCallApproval } from "@/components/stratum/conversation/types"
import { MOTION_DURATION, MOTION_EASE, motionDuration } from "@/lib/motion"
import { cn } from "@/lib/utils"

gsap.registerPlugin(useGSAP)

/**
 * ApprovalDock —— 待决审批的浮层：absolute 定位在 composer 容器内、
 * PromptInput 正上方（inset-x-0 bottom-full），不推挤消息区；容器
 * pointer-events-none，只有卡片本身接收交互。多个待决按到达顺序竖排
 * （新的靠近 composer）。
 *
 * 动画（GSAP，useGSAP 带 scope）：进场 y 20 + autoAlpha 0 + scale 0.98 →
 * 归位（全站尺度 base，多卡 stagger 0.05s）；退场 y 16 + 淡出下滑（fast），
 * onComplete 后才从 DOM 移除（父组件把消失的审批暂存进 leaving 集合，
 * derive-state-during-render 推导）。卡片在去留间快速切换可打断续播
 * （overwrite: "auto"）。prefers-reduced-motion 时 duration/delay 为 0。
 */

export type ApprovalDockEntry = {
  view: ToolCallApproval
  argumentsText: string
}

export function ApprovalDock({
  approvals,
  onResolve,
  className,
}: {
  /** 待决/提交中的审批（pending + submitting），按到达顺序 */
  approvals: readonly ApprovalDockEntry[]
  onResolve: (approvalId: string, decision: "approve" | "reject") => void
  className?: string
}) {
  // known：最近见到的审批数据（供退场期间继续渲染）；leaving：正在退场的
  const [track, setTrack] = useState<{
    known: Map<string, ApprovalDockEntry>
    leaving: Map<string, ApprovalDockEntry>
  }>({ known: new Map(), leaving: new Map() })

  // derive-state-during-render：消失的审批移入 leaving（保留最后已知数据），
  // 重新出现的从 leaving 捞回；计算幂等，第二轮渲染无变化即停
  const currentIds = new Set(approvals.map((entry) => entry.view.approvalId))
  let changed = false
  const known = new Map(track.known)
  for (const entry of approvals) {
    const id = entry.view.approvalId
    const prev = known.get(id)
    if (
      prev?.view.status !== entry.view.status ||
      prev.argumentsText !== entry.argumentsText
    ) {
      known.set(id, entry)
      changed = true
    }
  }
  const leaving = new Map(track.leaving)
  for (const [id, entry] of known) {
    if (!currentIds.has(id) && !leaving.has(id)) {
      leaving.set(id, entry)
      changed = true
    }
  }
  for (const id of [...leaving.keys()]) {
    if (currentIds.has(id)) {
      leaving.delete(id)
      changed = true
    }
  }
  if (changed) setTrack({ known, leaving })

  const handleExitDone = useCallback((approvalId: string) => {
    setTrack((prev) => {
      if (!prev.leaving.has(approvalId)) return prev
      const known = new Map(prev.known)
      const leaving = new Map(prev.leaving)
      known.delete(approvalId)
      leaving.delete(approvalId)
      return { known, leaving }
    })
  }, [])

  const rendered = [
    ...approvals.map((entry) => ({ entry, leaving: false })),
    ...[...track.leaving.values()].map((entry) => ({ entry, leaving: true })),
  ]

  if (rendered.length === 0) return null

  return (
    <div
      data-slot="approval-dock"
      className={cn(
        "pointer-events-none absolute inset-x-0 bottom-full z-10 mb-2 flex flex-col gap-2",
        className
      )}
    >
      {rendered.map(({ entry, leaving: isLeaving }, index) => (
        <ApprovalDockItem
          key={entry.view.approvalId}
          entry={entry}
          leaving={isLeaving}
          index={index}
          onExitDone={handleExitDone}
          onResolve={onResolve}
        />
      ))}
    </div>
  )
}

function ApprovalDockItem({
  entry,
  leaving,
  index,
  onExitDone,
  onResolve,
}: {
  entry: ApprovalDockEntry
  leaving: boolean
  index: number
  onExitDone: (approvalId: string) => void
  onResolve: (approvalId: string, decision: "approve" | "reject") => void
}) {
  const rootRef = useRef<HTMLDivElement>(null)
  const mountedRef = useRef(false)

  useGSAP(
    () => {
      const element = rootRef.current
      if (!element) return

      if (leaving) {
        gsap.to(element, {
          y: 16,
          autoAlpha: 0,
          scale: 0.98,
          duration: motionDuration(MOTION_DURATION.fast),
          ease: MOTION_EASE.exit,
          overwrite: "auto",
          onComplete: () => onExitDone(entry.view.approvalId),
        })
      } else {
        // 首次挂载从偏移处浮入；中途从退场被打断时从当前位置续播
        if (!mountedRef.current)
          gsap.set(element, { y: 20, autoAlpha: 0, scale: 0.98 })
        gsap.to(element, {
          y: 0,
          autoAlpha: 1,
          scale: 1,
          duration: motionDuration(MOTION_DURATION.base),
          ease: MOTION_EASE.enter,
          delay: mountedRef.current ? 0 : motionDuration(index * 0.05),
          overwrite: "auto",
        })
      }
      mountedRef.current = true
    },
    { scope: rootRef, dependencies: [leaving] }
  )

  return (
    <div ref={rootRef} className="pointer-events-auto">
      <ApprovalCard
        approval={entry.view}
        argumentsText={entry.argumentsText}
        onResolve={onResolve}
        className="border-border bg-card shadow-lg"
      />
    </div>
  )
}
