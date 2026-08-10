"use client"

import { useCallback, useMemo, useRef, useState } from "react"

import { ApprovalDock } from "@/components/stratum/conversation/approval-dock"
import { ConversationThread } from "@/components/stratum/conversation/conversation-thread"
import {
  RealtimeDegradedNotice,
  ResumeNotice,
} from "@/components/stratum/conversation/notices"
import { ThreadListRail } from "@/components/stratum/conversation/thread-list-rail"
import type {
  ConversationItem,
  ConversationMessage,
  ConversationToolCall,
  ToolCallApproval,
} from "@/components/stratum/conversation/types"
import { ModelSelector } from "@/components/stratum/model-selector"
import { AgentSelector } from "@/components/stratum/agent-selector"
import { PromptInput } from "@/components/stratum/prompt-input"
import type {
  ApprovalRequest,
  StableMessage,
} from "@/features/agent-conversation/types"
import { useAgentConversation } from "@/hooks/use-agent-conversation"
import { compareEventSeq } from "@/lib/stratum/api"
import {
  currentThinkingLevel,
  thinkingLevels,
} from "@/lib/stratum/model-config"

/**
 * DIRECTION CONTRACT —— /conversation 展示页
 * THESIS: conversation 组件库在真实界面里工作——消息流、流式生成、
 *         会话切换，一屏看完；拒绝拆成孤立的 demo 格子。
 * OWN-WORLD: 双 nav 悬浮体系内的完整 chat 界面；消息体走我们自己的
 *            streamdown + Medium 排版（compact），语义色全部 token。
 * STORY: 访客发一条消息，看到真实 agent 的流式回复；左侧切换历史会话。
 * FIRST VIEWPORT: 左侧会话列表 + 右侧消息流 + 底部 PromptInput（模型选择 + 电弧激活）。
 * FORM: 整屏 Operate 界面（assistant-ui 底稿的展示层 fork），非 section 展示页。
 *
 * 数据来自 Stratum 后端（Postgres-first REST + AgentRuntime-scoped SSE）：durable
 * identity 是 (agentRuntimeId, eventSeq 十进制字符串)；reasoning 与 tool calls 在
 * 正文上方渐进式透明展示（默认折叠，待决审批强制展开可操作）；
 * TranscriptCompacted 渲染为可折叠"上下文已压缩" marker；failed/cancelled
 * 渲染为安全 terminal marker；向上滚动按固定 through barrier 分页更旧历史。
 */

/** 工具 result/arguments（unknown）→ 可显示文本 */
function stringifyToolData(value: unknown): string | null {
  if (value === null || value === undefined) return null
  if (typeof value === "string") return value
  try {
    return JSON.stringify(value, null, 2) ?? null
  } catch {
    return String(value)
  }
}

type ApprovalEntry = { view: ToolCallApproval; argumentsText: string }

const WELCOME = (
  <h1 className="text-center font-heading text-2xl tracking-tight">
    今天想聊点什么？
  </h1>
)

type SettledViewCacheEntry = {
  inputs: readonly unknown[]
  view: ConversationMessage
}

// StableMessage 对象在 reducer 不可变更新中保持引用；视图构建的输入
// （历史标记、工具 progress、审批视图）逐项 === 校验，命中即复用视图对象——
// 流式 token 与消息落成都不改变未变消息的引用，下游 memo 才能命中
const settledViewCache = new WeakMap<StableMessage, SettledViewCacheEntry>()

export default function ConversationPage() {
  const {
    state,
    recentAgentRuntimes,
    composerConfiguration,
    selectAgentRuntime,
    createConversation,
    sendMessage,
    cancel,
    resume,
    resolveApproval,
    loadOlderHistory,
  } = useAgentConversation()

  const threads = useMemo(
    () =>
      recentAgentRuntimes.map((runtime) => ({
        id: runtime.agentRuntimeId,
        title: runtime.title,
      })),
    [recentAgentRuntimes]
  )

  // 历史/新消息区分：recovery 完成（ready）时快照当时的 barrier；seq ≤ 该
  // barrier 的消息为历史（reasoning 默认折叠，含之后向上分页加载的旧页），
  // 之后到达的为本轮新消息（默认简略预览）。
  // derive-state-during-render 模式：渲染期条件 setState，立即重渲染提交。
  const [historical, setHistorical] = useState<{
    agentRuntimeId: string | null
    historyThrough: string | null
    pgConfirmedEventSeq: string
  }>({
    agentRuntimeId: null,
    historyThrough: null,
    pgConfirmedEventSeq: "0",
  })
  if (
    state.phase === "ready" &&
    (historical.agentRuntimeId !== state.agentRuntimeId ||
      historical.historyThrough !== state.historyThrough)
  ) {
    setHistorical({
      agentRuntimeId: state.agentRuntimeId,
      historyThrough: state.historyThrough,
      pgConfirmedEventSeq: state.pgConfirmedEventSeq,
    })
  }

  // 审批提交状态：submitting = 已点击等后端确认；outcomes = 本会话已决终态。
  // agent 切换时一并重置（同为 derive-state-during-render）。
  const [submittingApprovals, setSubmittingApprovals] = useState<Set<string>>(
    new Set()
  )
  // 双击竞态的同步守卫（state 闭包读不到同帧最新值）。agent 切换无需重置：
  // 旧 id 必然不在新会话的 state.approvals 里，前置 guard 已挡
  const submittingRef = useRef<Set<string>>(new Set())
  const [approvalOutcomes, setApprovalOutcomes] = useState<
    Record<
      string,
      { approval: ApprovalRequest; decision: "approve" | "reject" }
    >
  >({})
  const [approvalAgentRuntimeId, setApprovalAgentRuntimeId] = useState(
    state.agentRuntimeId
  )
  if (approvalAgentRuntimeId !== state.agentRuntimeId) {
    setApprovalAgentRuntimeId(state.agentRuntimeId)
    setSubmittingApprovals(new Set())
    setApprovalOutcomes({})
  }

  // 审批视图（按 callId 索引）：已决兜底展示，待决覆盖已决
  // —— 若 resolve 失败审批仍在 state.approvals 中，自动回退为待决。
  const approvalEntries = useMemo(() => {
    const entries = new Map<string, ApprovalEntry>()
    for (const outcome of Object.values(approvalOutcomes)) {
      entries.set(outcome.approval.callId, {
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
    for (const approval of Object.values(state.approvals)) {
      entries.set(approval.callId, {
        view: {
          approvalId: approval.approvalId,
          callId: approval.callId,
          toolName: approval.toolName,
          toolKind: approval.toolKind,
          dangerLevel: approval.dangerLevel,
          status: submittingApprovals.has(approval.approvalId)
            ? "submitting"
            : "pending",
        },
        argumentsText: stringifyToolData(approval.arguments) ?? "",
      })
    }
    return entries
  }, [approvalOutcomes, state.approvals, submittingApprovals])

  // 待决/提交中的审批（浮层数据源）：已决的退场后由内联块展示终态
  const pendingApprovals = useMemo(
    () =>
      [...approvalEntries.values()].filter(
        (entry) =>
          entry.view.status === "pending" || entry.view.status === "submitting"
      ),
    [approvalEntries]
  )

  const handleResolveApproval = useCallback(
    (approvalId: string, decision: "approve" | "reject") => {
      const approval = state.approvals[approvalId]
      // ref 同步 check-and-add：同帧双击必被挡（state 闭包读不到最新值）
      if (!approval || submittingRef.current.has(approvalId)) return
      submittingRef.current.add(approvalId)
      setSubmittingApprovals((prev) => new Set(prev).add(approvalId))
      // hook 返回 boolean：失败不记 outcome，审批留在 state.approvals 回退待决
      void resolveApproval(approvalId, decision).then((ok) => {
        submittingRef.current.delete(approvalId)
        setSubmittingApprovals((prev) => {
          const next = new Set(prev)
          next.delete(approvalId)
          return next
        })
        if (ok)
          setApprovalOutcomes((prev) => ({
            ...prev,
            [approvalId]: { approval, decision },
          }))
      })
    },
    [resolveApproval, state.approvals]
  )

  // 冷段：timeline 已落成部分 → 视图条目（只在消息落成/工具结果/审批/历史
  // 标记变化时重跑；WeakMap 缓存保证未变消息复用旧对象引用）
  const settled = useMemo(() => {
    const views: ConversationItem[] = []
    for (const entry of state.timeline) {
      if (entry.kind === "compaction") {
        views.push({
          kind: "compaction",
          id: `${entry.marker.agentRuntimeId}:${entry.marker.eventSeq}`,
          summary: entry.marker.summary,
          compactedIteration: entry.marker.compactedIteration,
        })
        continue
      }
      if (entry.kind === "terminal") {
        views.push({
          kind: "terminal",
          id: `${entry.marker.agentRuntimeId}:${entry.marker.eventSeq}`,
          terminal: entry.marker.terminal,
          errorText: entry.marker.errorText,
        })
        continue
      }

      const message = entry.message
      if (message.role !== "user" && message.role !== "assistant") continue
      const id = `${message.agentRuntimeId}:${message.eventSeq}`
      const isHistorical =
        compareEventSeq(message.eventSeq, historical.pgConfirmedEventSeq) <= 0
      const inputs: readonly unknown[] = [
        isHistorical,
        ...message.toolCalls.map(
          (toolCall) => state.tools[toolCall.callId] ?? null
        ),
        ...message.toolCalls.map(
          (toolCall) => approvalEntries.get(toolCall.callId)?.view ?? null
        ),
      ]
      const cached = settledViewCache.get(message)
      if (
        cached &&
        cached.inputs.length === inputs.length &&
        cached.inputs.every((input, index) => input === inputs[index])
      ) {
        views.push({ kind: "message", id, message: cached.view })
        continue
      }

      const view: ConversationMessage = {
        id,
        role: message.role as "user" | "assistant",
        content: message.text ?? "",
        status: "done",
        ...(message.reasoning
          ? {
              reasoning: message.reasoning,
              reasoningDefaultView: isHistorical
                ? ("collapsed" as const)
                : ("preview" as const),
            }
          : {}),
        ...(message.toolCalls.length > 0
          ? {
              toolCalls: message.toolCalls.map((toolCall) => {
                // 结果/状态从实时 tools 配对（含 tool 角色消息带回的 result）；
                // 配不上就只做 name + arguments
                const progress = state.tools[toolCall.callId]
                const approval = approvalEntries.get(toolCall.callId)?.view
                return {
                  callId: toolCall.callId,
                  name: toolCall.name,
                  argumentsText:
                    progress?.argumentsText ||
                    (stringifyToolData(toolCall.arguments) ?? ""),
                  result: stringifyToolData(progress?.result),
                  errorText: progress?.errorText ?? null,
                  status: progress?.status ?? ("finished" as const),
                  ...(approval ? { approval } : {}),
                }
              }),
            }
          : {}),
      }
      settledViewCache.set(message, { inputs, view })
      views.push({ kind: "message", id, message: view })
    }
    // 已落成消息里的工具 callId（用于把已提交的调用从实时 tools 中排除）
    const callIds = new Set(
      views.flatMap((item) =>
        item.kind === "message"
          ? (item.message.toolCalls ?? []).map((toolCall) => toolCall.callId)
          : []
      )
    )
    return { items: views, callIds }
  }, [state.timeline, state.tools, approvalEntries, historical])

  // 热段：draft/实时 tools/连接错误（流式 token 每帧重跑，但只追加新对象，
  // settled 部分整体复用）
  const items = useMemo<ConversationItem[]>(() => {
    const result: ConversationItem[] = [...settled.items]
    const stableCallIds = settled.callIds
    const attachApproval = (
      call: ConversationToolCall
    ): ConversationToolCall => {
      const entry = approvalEntries.get(call.callId)
      return entry ? { ...call, approval: entry.view } : call
    }

    // 实时工具调用（draft 消息展示）：state.tools 中尚未落成到消息的
    const liveToolCalls: ConversationToolCall[] = Object.values(state.tools)
      .filter((tool) => !stableCallIds.has(tool.callId))
      .map((tool) =>
        attachApproval({
          callId: tool.callId,
          name: tool.name,
          argumentsText: tool.argumentsText,
          result: stringifyToolData(tool.result),
          errorText: tool.errorText,
          status: tool.status,
        })
      )
    // 无工具块匹配的审批：以伪工具块渲染（审批必须直接可见可操作）
    for (const entry of approvalEntries.values()) {
      if (
        !stableCallIds.has(entry.view.callId) &&
        !liveToolCalls.some((call) => call.callId === entry.view.callId)
      ) {
        liveToolCalls.push({
          callId: entry.view.callId,
          name: entry.view.toolName,
          argumentsText: entry.argumentsText,
          result: null,
          errorText: null,
          status: "streaming",
          approval: entry.view,
        })
      }
    }

    // 流式 text/reasoning：各 draft 按到达顺序拼接（不分段，单趟循环）
    let draftText = ""
    let draftReasoning = ""
    for (const draft of Object.values(state.drafts)) {
      draftText += draft.text
      draftReasoning += draft.reasoning
    }
    const status = state.view?.status
    if (status === "running") {
      result.push({
        kind: "message",
        id: "draft",
        message: {
          id: "draft",
          role: "assistant",
          content: draftText,
          status: "streaming",
          ...(draftReasoning
            ? {
                reasoning: draftReasoning,
                reasoningDefaultView: "preview" as const,
              }
            : {}),
          ...(liveToolCalls.length > 0 ? { toolCalls: liveToolCalls } : {}),
        },
      })
    } else if (liveToolCalls.length > 0) {
      // 回合结束但工具未落成到消息（含等待审批的空闲态）：挂到最后一条 assistant 消息
      for (let index = result.length - 1; index >= 0; index -= 1) {
        const item = result[index]
        if (item.kind !== "message" || item.message.role !== "assistant")
          continue
        result[index] = {
          ...item,
          message: {
            ...item.message,
            toolCalls: [...(item.message.toolCalls ?? []), ...liveToolCalls],
          },
        }
        break
      }
    }

    if (state.phase === "connection_error" || state.phase === "missing") {
      result.push({
        kind: "message",
        id: "connection-error",
        message: {
          id: "connection-error",
          role: "assistant",
          content:
            state.phase === "missing"
              ? "会话不存在或已被删除（404）。"
              : `连接出错：${state.error?.message ?? "无法连接到 Stratum 后端"}`,
          status: "error",
        },
      })
    }

    return result
  }, [
    settled,
    state.drafts,
    state.tools,
    state.view?.status,
    state.phase,
    state.error,
    approvalEntries,
  ])

  const selectedModelConfig = composerConfiguration.selectedModelConfig
  const selectedDescriptor = useMemo(
    () =>
      composerConfiguration.models.find(
        (descriptor) => descriptor.model === selectedModelConfig?.model
      ),
    [composerConfiguration.models, selectedModelConfig?.model]
  )
  const levels = useMemo(
    () => thinkingLevels(selectedDescriptor?.parameters_schema),
    [selectedDescriptor]
  )
  const selectedLevel =
    selectedModelConfig === null
      ? null
      : currentThinkingLevel(selectedModelConfig.parameters)

  const [sendVersion, setSendVersion] = useState(0)
  // 受控 composer：发送成功才清空；首条消息失败等场景保留用户原文
  const [composerValue, setComposerValue] = useState("")
  const handleSubmit = (value: string) => {
    // 发送信号：让 thread 把随后的 null → 新 runtime id 识别为同一对话的首发
    setSendVersion((version) => version + 1)
    const sent =
      state.agentRuntimeId === null
        ? createConversation(value)
        : sendMessage(value)
    void sent.then((ok) => {
      // 请求期间用户可能已经继续输入；只清掉本次实际发送的原值。
      if (ok) setComposerValue((current) => (current === value ? "" : current))
    })
  }

  const handleNewConversation = useCallback(
    () => selectAgentRuntime(null),
    [selectAgentRuntime]
  )

  const turnRunning = composerConfiguration.turnRunning
  const resumeRequired = state.view?.resume_required === true

  return (
    <div className="flex h-svh pt-24 font-sans sm:pt-28">
      <main className="relative min-w-0 flex-1">
        <ThreadListRail
          threads={threads}
          activeId={state.agentRuntimeId ?? undefined}
          onSelect={selectAgentRuntime}
          onNew={handleNewConversation}
        />

        <ConversationThread
          items={items}
          conversationId={state.agentRuntimeId}
          sendVersion={sendVersion}
          recovering={state.phase === "recovering"}
          hasOlder={state.historyHasMore}
          olderLoading={state.historyLoading}
          onLoadOlder={loadOlderHistory}
          welcome={WELCOME}
          composer={
            <div className="relative">
              <ApprovalDock
                approvals={pendingApprovals}
                onResolve={handleResolveApproval}
              />
              {resumeRequired ||
              state.realtimeDegraded ||
              (state.cancelRequested && turnRunning) ? (
                <div className="mb-2 flex flex-col gap-1.5">
                  {resumeRequired ? (
                    <ResumeNotice onResume={() => void resume()} />
                  ) : null}
                  {state.cancelRequested && turnRunning ? (
                    <p
                      role="status"
                      className="px-2 text-xs text-muted-foreground"
                    >
                      取消请求已发送
                    </p>
                  ) : null}
                  {state.realtimeDegraded ? <RealtimeDegradedNotice /> : null}
                </div>
              ) : null}
              <PromptInput
                placeholder="问问 Stratum"
                value={composerValue}
                onChange={setComposerValue}
                onSubmit={handleSubmit}
                running={turnRunning && !resumeRequired}
                cancelRequested={state.cancelRequested}
                onCancel={() => void cancel()}
                leading={
                  !composerConfiguration.existingRuntime &&
                  composerConfiguration.agentTemplates.length > 0 ? (
                    <AgentSelector
                      templates={composerConfiguration.agentTemplates}
                      selectedTemplate={
                        composerConfiguration.selectedTemplate
                      }
                      onSelectTemplate={composerConfiguration.selectTemplate}
                    />
                  ) : null
                }
                trailing={
                  <div className="flex items-center gap-1.5">
                    <ModelSelector
                      models={composerConfiguration.models}
                      selectedModelId={selectedModelConfig?.model ?? null}
                      onSelectModel={composerConfiguration.selectModel}
                      thinkingLevels={levels}
                      selectedThinkingLevel={selectedLevel}
                      onSelectThinkingLevel={
                        composerConfiguration.setThinkingLevel
                      }
                      loading={composerConfiguration.metadataLoading}
                      error={composerConfiguration.metadataError !== null}
                    />
                  </div>
                }
              />
            </div>
          }
        />
      </main>
    </div>
  )
}
