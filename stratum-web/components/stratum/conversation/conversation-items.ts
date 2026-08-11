import type {
  ConversationItem,
  ConversationMessage,
  ConversationToolCall,
  ToolCallApproval,
} from "@/components/stratum/conversation/types"
import type {
  ConversationState,
  StableMessage,
} from "@/features/agent-conversation/types"
import { compareEventSeq } from "@/lib/stratum/api"

/**
 * conversation-items —— ConversationState → 视图条目（ConversationItem[]）
 * 的纯函数映射，供 /conversation 页 useMemo 调用。
 *
 * 冷段（buildSettledItems）：timeline 已落成部分 → 视图条目。WeakMap 缓存
 * 以 StableMessage 引用为键（reducer 不可变更新保持引用），输入逐项 ===
 * 校验，命中即复用视图对象——流式 token 与消息落成都不改变未变消息的
 * 引用，下游 memo 才能命中。
 * 热段（composeLiveItems）：draft/实时 tools/连接错误，流式期间每帧重跑，
 * 但只追加新对象，settled 部分整体复用。
 */

/** 工具 result/arguments（unknown）→ 可显示文本 */
export function stringifyToolData(value: unknown): string | null {
  if (value === null || value === undefined) return null
  if (typeof value === "string") return value
  try {
    return JSON.stringify(value, null, 2) ?? null
  } catch {
    return String(value)
  }
}

/** 审批视图条目（按 callId 索引）：已决兜底展示，待决覆盖已决 */
export type ApprovalEntry = { view: ToolCallApproval; argumentsText: string }
export type ApprovalEntries = ReadonlyMap<string, ApprovalEntry>

type SettledViewCacheEntry = {
  inputs: readonly unknown[]
  view: ConversationMessage
}

const settledViewCache = new WeakMap<StableMessage, SettledViewCacheEntry>()

export type SettledItems = {
  items: ConversationItem[]
  /** 已落成消息里的工具 callId（用于把已提交的调用从实时 tools 中排除） */
  callIds: ReadonlySet<string>
}

export function buildSettledItems(
  timeline: ConversationState["timeline"],
  tools: ConversationState["tools"],
  approvalEntries: ApprovalEntries,
  /** recovery 完成时快照的 barrier：seq ≤ 该值为历史（reasoning 默认折叠） */
  pgConfirmedEventSeq: string
): SettledItems {
  const views: ConversationItem[] = []
  for (const entry of timeline) {
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
      compareEventSeq(message.eventSeq, pgConfirmedEventSeq) <= 0
    const inputs: readonly unknown[] = [
      isHistorical,
      ...message.toolCalls.map((toolCall) => tools[toolCall.callId] ?? null),
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
              const progress = tools[toolCall.callId]
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
  const callIds = new Set(
    views.flatMap((item) =>
      item.kind === "message"
        ? (item.message.toolCalls ?? []).map((toolCall) => toolCall.callId)
        : []
    )
  )
  return { items: views, callIds }
}

export function composeLiveItems(
  settled: SettledItems,
  drafts: ConversationState["drafts"],
  tools: ConversationState["tools"],
  status: string | undefined,
  phase: ConversationState["phase"],
  error: ConversationState["error"],
  approvalEntries: ApprovalEntries
): ConversationItem[] {
  const result: ConversationItem[] = [...settled.items]
  const stableCallIds = settled.callIds
  const attachApproval = (call: ConversationToolCall): ConversationToolCall => {
    const entry = approvalEntries.get(call.callId)
    return entry ? { ...call, approval: entry.view } : call
  }

  // 实时工具调用（draft 消息展示）：state.tools 中尚未落成到消息的
  const liveToolCalls: ConversationToolCall[] = Object.values(tools)
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
  for (const draft of Object.values(drafts)) {
    draftText += draft.text
    draftReasoning += draft.reasoning
  }
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

  if (phase === "connection_error" || phase === "missing") {
    result.push({
      kind: "message",
      id: "connection-error",
      message: {
        id: "connection-error",
        role: "assistant",
        content:
          phase === "missing"
            ? "会话不存在或已被删除（404）。"
            : `连接出错：${error?.message ?? "无法连接到 Stratum 后端"}`,
        status: "error",
      },
    })
  }

  return result
}
