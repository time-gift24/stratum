"use client"

import { useCallback, useMemo, useRef, useState } from "react"

import { ApprovalDock } from "@/components/stratum/conversation/approval-dock"
import { ConversationThread } from "@/components/stratum/conversation/conversation-thread"
import { ThreadListRail } from "@/components/stratum/conversation/thread-list-rail"
import type {
  ConversationMessage,
  ConversationToolCall,
  ToolCallApproval,
} from "@/components/stratum/conversation/types"
import { ModelSelector } from "@/components/stratum/model-selector"
import { PromptInput } from "@/components/stratum/prompt-input"
import type {
  ApprovalRequest,
  StableMessage,
} from "@/features/agent-conversation/types"
import { useAgentConversation } from "@/hooks/use-agent-conversation"
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
 * 数据来自 Stratum 后端（REST + SSE）：reasoning 与 tool calls 在正文上方
 * 渐进式透明展示（默认折叠，待决审批强制展开可操作）。
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
    recentAgents,
    composerConfiguration,
    selectAgent,
    createConversation,
    sendMessage,
    resolveApproval,
  } = useAgentConversation()

  const threads = useMemo(
    () =>
      recentAgents.map((agent) => ({
        id: agent.agentId,
        title: agent.title,
      })),
    [recentAgents]
  )

  // 历史/新消息区分：recovery 完成（ready）时快照已有消息 id 为历史
  // （reasoning 默认折叠），之后出现的 id 为本轮新消息（默认简略预览）。
  // derive-state-during-render 模式：渲染期条件 setState，立即重渲染提交。
  const [historical, setHistorical] = useState<{
    agentId: string | null
    ids: Set<string>
  }>({ agentId: null, ids: new Set() })
  if (state.phase === "ready" && historical.agentId !== state.agentId) {
    setHistorical({
      agentId: state.agentId,
      ids: new Set(
        state.messages.map(
          (message) => `${message.agentId}:${message.messageSeq}`
        )
      ),
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
    Record<string, { approval: ApprovalRequest; decision: "approve" | "reject" }>
  >({})
  const [approvalAgentId, setApprovalAgentId] = useState(state.agentId)
  if (approvalAgentId !== state.agentId) {
    setApprovalAgentId(state.agentId)
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

  // 冷段：已落成消息 → 视图对象（只在消息落成/工具结果/审批/历史标记变化时
  // 重跑；WeakMap 缓存保证未变消息复用旧对象引用）
  const settled = useMemo(() => {
    const views: ConversationMessage[] = state.messages
      .filter(
        (message) => message.role === "user" || message.role === "assistant"
      )
      .map((message) => {
        const id = `${message.agentId}:${message.messageSeq}`
        const isHistorical = historical.ids.has(id)
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
        )
          return cached.view

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
        return view
      })
    // 已落成消息里的工具 callId（用于把已提交的调用从实时 tools 中排除）
    const callIds = new Set(
      views.flatMap((message) =>
        (message.toolCalls ?? []).map((toolCall) => toolCall.callId)
      )
    )
    return { messages: views, callIds }
  }, [state.messages, state.tools, approvalEntries, historical])

  // 热段：draft/实时 tools/连接错误（流式 token 每帧重跑，但只追加新对象，
  // settled 部分整体复用）
  const messages = useMemo<ConversationMessage[]>(() => {
    const result: ConversationMessage[] = [...settled.messages]
    const stableCallIds = settled.callIds
    const attachApproval = (call: ConversationToolCall): ConversationToolCall => {
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
        id: "draft",
        role: "assistant",
        content: draftText,
        status: "streaming",
        ...(draftReasoning
          ? { reasoning: draftReasoning, reasoningDefaultView: "preview" as const }
          : {}),
        ...(liveToolCalls.length > 0 ? { toolCalls: liveToolCalls } : {}),
      })
    } else if (status === "failed") {
      result.push({
        id: "draft",
        role: "assistant",
        content: draftText || (state.error?.message ?? "生成失败"),
        status: "error",
        ...(draftReasoning
          ? { reasoning: draftReasoning, reasoningDefaultView: "preview" as const }
          : {}),
        ...(liveToolCalls.length > 0 ? { toolCalls: liveToolCalls } : {}),
      })
    } else if (liveToolCalls.length > 0) {
      // 回合结束但工具未落成到消息（含等待审批的空闲态）：挂到最后一条 assistant 消息
      for (let index = result.length - 1; index >= 0; index -= 1) {
        if (result[index].role !== "assistant") continue
        result[index] = {
          ...result[index],
          toolCalls: [...(result[index].toolCalls ?? []), ...liveToolCalls],
        }
        break
      }
    }

    if (state.phase === "connection_error" || state.phase === "missing") {
      result.push({
        id: "connection-error",
        role: "assistant",
        content:
          state.phase === "missing"
            ? "会话不存在或已被删除（404）。"
            : `连接出错：${state.error?.message ?? "无法连接到 Stratum 后端"}`,
        status: "error",
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
  const handleSubmit = (value: string) => {
    // 发送信号：让 thread 把随后的 null → 新 agentId 识别为同一对话的首发
    setSendVersion((version) => version + 1)
    if (state.agentId === null) void createConversation(value)
    else void sendMessage(value)
  }

  const handleNewConversation = useCallback(
    () => selectAgent(null),
    [selectAgent]
  )

  return (
    <div className="flex h-svh pt-24 font-sans sm:pt-28">
      <main className="relative min-w-0 flex-1">
        <ThreadListRail
          threads={threads}
          activeId={state.agentId ?? undefined}
          onSelect={selectAgent}
          onNew={handleNewConversation}
        />

        <ConversationThread
          messages={messages}
          conversationId={state.agentId}
          sendVersion={sendVersion}
          recovering={state.phase === "recovering"}
          welcome={WELCOME}
          composer={
            <div className="relative">
              <ApprovalDock
                approvals={pendingApprovals}
                onResolve={handleResolveApproval}
              />
              <PromptInput
                placeholder="问问 Stratum"
                onSubmit={handleSubmit}
                trailing={
                  <ModelSelector
                    models={composerConfiguration.models}
                    selectedModelId={selectedModelConfig?.model ?? null}
                    onSelectModel={composerConfiguration.selectModel}
                    thinkingLevels={levels}
                    selectedThinkingLevel={selectedLevel}
                    onSelectThinkingLevel={composerConfiguration.setThinkingLevel}
                    loading={composerConfiguration.metadataLoading}
                    error={composerConfiguration.metadataError !== null}
                  />
                }
              />
            </div>
          }
        />
      </main>
    </div>
  )
}
