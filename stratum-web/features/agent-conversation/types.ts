import type {
  AgentDurableRecordV1,
  AgentStreamFrameV1,
  AgentView,
  ApiError,
} from "@/lib/stratum/api"

/**
 * agent-conversation 状态模型（Postgres-first 协议投影，UI 无关）。
 *
 * - durable identity = (agentId, eventSeq 十进制字符串)：去重 + 容忍可见序号间隔。
 * - telemetry identity = (llmCallId, telemetrySeq)：volatile draft，
 *   final durable assistant message 是 draft 的唯一 truth。
 * - timeline 混合 message / compaction marker / 安全 terminal marker，
 *   全部按 eventSeq 升序。
 */

export type StableMessage = {
  agentId: string
  /** 十进制字符串 durable 序号 */
  eventSeq: string
  role: "user" | "assistant" | "tool" | "system"
  text: string | null
  json: unknown | null
  reasoning: string | null
  toolCalls: readonly { callId: string; name: string; arguments: unknown }[]
  timestamp: string
}

/** TranscriptCompacted 的可折叠 marker（summary 完整保留，原消息不删除） */
export type CompactionMarker = {
  agentId: string
  eventSeq: string
  summary: string
  compactedIteration: number
  timestamp: string
}

/** 安全 terminal marker（failed / cancelled；finished 由最终 assistant 消息自然收尾） */
export type TerminalMarker = {
  agentId: string
  eventSeq: string
  terminal: "failed" | "cancelled"
  errorText: string | null
  timestamp: string
}

export type TimelineEntry =
  | { kind: "message"; message: StableMessage }
  | { kind: "compaction"; marker: CompactionMarker }
  | { kind: "terminal"; marker: TerminalMarker }

export type ToolProgress = {
  callId: string
  llmCallId: string | null
  name: string | null
  argumentsText: string
  result: unknown | null
  errorText: string | null
  /**
   * streaming：等待 durable tool message；finished：已收到 role=tool 的最终
   * 结果；failed：保留给显式错误结果；interrupted：terminal 时仍无结果
   * （不伪造 result）。
   */
  status: "streaming" | "finished" | "failed" | "interrupted"
}

/** 进行中的 LLM call 的 volatile draft */
export type DraftState = {
  llmCallId: string
  text: string
  reasoning: string
  /** 下一期待的 telemetry_seq（低于它是重复，高于它标记 incomplete） */
  nextTelemetrySeq: number
  incomplete: boolean
}

export type ApprovalRequest = {
  approvalId: string
  callId: string
  toolName: string
  arguments: unknown
  toolKind: "read" | "write"
  dangerLevel: "low" | "medium" | "high"
}

export type ConversationState = {
  agentId: string | null
  view: AgentView | null
  /** 已应用的 durable barrier（十进制字符串），初始 "0" */
  barrier: string
  timeline: readonly TimelineEntry[]
  /** 已应用的 durable event_seq 集合（去重；仅页面内存） */
  appliedEventSeqs: ReadonlySet<string>
  drafts: Readonly<Record<string, DraftState>>
  activeLlmCallId: string | null
  /**
   * 已被 durable final 收敛或 terminal 清理关闭的 llm_call_id 集合（仅页面
   * 内存）。closed call 的迟到 telemetry 一律忽略，不重新创建 draft。
   */
  closedLlmCallIds: ReadonlySet<string>
  tools: Readonly<Record<string, ToolProgress>>
  approvals: Readonly<Record<string, ApprovalRequest>>
  /** 向上分页的固定 through barrier（cold bootstrap 时确定） */
  historyThrough: string | null
  /** 下一页的 exclusive before cursor */
  historyBefore: string | null
  historyHasMore: boolean
  historyLoading: boolean
  /** NATS 不可用等导致的实时降级；核心命令与 PG reconcile 继续工作 */
  realtimeDegraded: boolean
  /** cancel 202 已接受但 durable terminal 尚未确认 */
  cancelRequested: boolean
  phase: "empty" | "recovering" | "ready" | "connection_error" | "missing"
  error: ApiError | null
}

export type DurableFrame = Extract<AgentStreamFrameV1, { kind: "durable" }>
export type TelemetryFrame = Extract<AgentStreamFrameV1, { kind: "telemetry" }>

export type ConversationAction =
  | { type: "agent_selected"; agentId: string | null }
  | { type: "recovery_started"; agentId: string }
  | {
      type: "snapshot_loaded"
      view: AgentView
      items: readonly AgentDurableRecordV1[]
      historyBefore: string | null
      historyHasMore: boolean
    }
  | { type: "history_page_started" }
  | {
      type: "history_page_loaded"
      items: readonly AgentDurableRecordV1[]
      historyBefore: string | null
      historyHasMore: boolean
    }
  | { type: "history_page_failed" }
  | { type: "durable_frame"; frame: DurableFrame }
  | { type: "telemetry_frame"; frame: TelemetryFrame }
  /** 增量 reconcile：items 为 (旧 barrier, 新 barrier] 的可见 product items */
  | {
      type: "view_reconciled"
      view: AgentView
      items: readonly AgentDurableRecordV1[]
    }
  /** approval resolve 204 后的本地先行移除（reconcile 随后确认） */
  | { type: "approval_resolved"; approvalId: string }
  | { type: "cancel_requested" }
  | { type: "realtime_degraded"; degraded: boolean }
  | { type: "recovery_ready" }
  | { type: "connection_error"; error: ApiError }
  | { type: "missing"; error: ApiError | null }
