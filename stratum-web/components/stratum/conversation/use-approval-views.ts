"use client"

import { useCallback, useMemo, useRef, useState } from "react"

import {
  stringifyToolData,
  type ApprovalEntries,
  type ApprovalEntry,
} from "@/components/stratum/conversation/conversation-items"
import type {
  ApprovalRequest,
  ConversationState,
} from "@/features/agent-conversation/types"

/**
 * useApprovalViews —— 审批的提交状态与视图派生。
 *
 * submitting = 已点击等后端确认；outcomes = 本会话已决终态（agent 切换时
 * 一并重置，derive-state-during-render）。视图按 callId 索引：已决兜底
 * 展示，待决覆盖已决——若 resolve 失败审批仍在 state.approvals 中，
 * 自动回退为待决。submittingRef 是双击竞态的同步守卫（state 闭包读不到
 * 同帧最新值）；agent 切换无需重置：旧 id 必然不在新会话的 approvals 里，
 * 前置 guard 已挡。
 */
export function useApprovalViews(
  approvals: ConversationState["approvals"],
  agentRuntimeId: string | null,
  resolveApproval: (
    approvalId: string,
    decision: "approve" | "reject"
  ) => Promise<boolean>
): {
  entries: ApprovalEntries
  /** 待决/提交中的审批（浮层数据源）：已决的退场后由内联块展示终态 */
  pending: ApprovalEntry[]
  resolve: (approvalId: string, decision: "approve" | "reject") => void
} {
  const [submitting, setSubmitting] = useState<ReadonlySet<string>>(new Set())
  const submittingRef = useRef<Set<string>>(new Set())
  const [outcomes, setOutcomes] = useState<
    Record<string, { approval: ApprovalRequest; decision: "approve" | "reject" }>
  >({})
  const [outcomesAgentRuntimeId, setOutcomesAgentRuntimeId] =
    useState(agentRuntimeId)
  if (outcomesAgentRuntimeId !== agentRuntimeId) {
    setOutcomesAgentRuntimeId(agentRuntimeId)
    setSubmitting(new Set())
    setOutcomes({})
  }

  const entries = useMemo<ApprovalEntries>(() => {
    const map = new Map<string, ApprovalEntry>()
    for (const outcome of Object.values(outcomes)) {
      map.set(outcome.approval.callId, {
        view: {
          approvalId: outcome.approval.approvalId,
          callId: outcome.approval.callId,
          toolName: outcome.approval.toolName,
          toolKind: outcome.approval.toolKind,
          dangerLevel: outcome.approval.dangerLevel,
          status: outcome.decision === "approve" ? "approved" : "rejected",
        },
        argumentsText: stringifyToolData(outcome.approval.arguments) ?? "",
      })
    }
    for (const approval of Object.values(approvals)) {
      map.set(approval.callId, {
        view: {
          approvalId: approval.approvalId,
          callId: approval.callId,
          toolName: approval.toolName,
          toolKind: approval.toolKind,
          dangerLevel: approval.dangerLevel,
          status: submitting.has(approval.approvalId)
            ? "submitting"
            : "pending",
        },
        argumentsText: stringifyToolData(approval.arguments) ?? "",
      })
    }
    return map
  }, [outcomes, approvals, submitting])

  const pending = useMemo(
    () =>
      [...entries.values()].filter(
        (entry) =>
          entry.view.status === "pending" || entry.view.status === "submitting"
      ),
    [entries]
  )

  const resolve = useCallback(
    (approvalId: string, decision: "approve" | "reject") => {
      const approval = approvals[approvalId]
      // ref 同步 check-and-add：同帧双击必被挡（state 闭包读不到最新值）
      if (!approval || submittingRef.current.has(approvalId)) return
      submittingRef.current.add(approvalId)
      setSubmitting((prev) => new Set(prev).add(approvalId))
      // hook 返回 boolean：失败不记 outcome，审批留在 state.approvals 回退待决
      void resolveApproval(approvalId, decision).then((ok) => {
        submittingRef.current.delete(approvalId)
        setSubmitting((prev) => {
          const next = new Set(prev)
          next.delete(approvalId)
          return next
        })
        if (ok)
          setOutcomes((prev) => ({ ...prev, [approvalId]: { approval, decision } }))
      })
    },
    [resolveApproval, approvals]
  )

  return { entries, pending, resolve }
}
