import type {
  AgentRuntimeDurableRecordV1,
  AgentRuntimeStreamFrameV1,
  AgentRuntimeView,
  ApiError,
} from "@/lib/stratum/api"

/** UI-independent projection of one AgentRuntime conversation. */

export type StableMessage = {
  agentRuntimeId: string
  eventSeq: string
  role: "user" | "assistant" | "tool" | "system"
  text: string | null
  json: unknown | null
  reasoning: string | null
  toolCalls: readonly { callId: string; name: string; arguments: unknown }[]
  timestamp: string
}

export type CompactionMarker = {
  agentRuntimeId: string
  eventSeq: string
  summary: string
  compactedIteration: number
  timestamp: string
}

export type TerminalMarker = {
  agentRuntimeId: string
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
  turnId: string
  llmCallId: string | null
  name: string | null
  argumentsText: string
  result: unknown | null
  errorText: string | null
  status: "streaming" | "finished" | "failed" | "interrupted"
}

export type DraftState = {
  llmCallId: string
  turnId: string
  durableBeforeEventSeq: string
  text: string
  reasoning: string
  /** Next expected call-local unsigned decimal telemetry sequence. */
  nextTelemetrySeq: string
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

export type DurableFrame = Extract<
  AgentRuntimeStreamFrameV1,
  { kind: "durable" }
>
export type TelemetryFrame = Extract<
  AgentRuntimeStreamFrameV1,
  { kind: "telemetry" }
>

export type ConversationState = {
  /** Long-lived conversation/runtime identity. */
  agentRuntimeId: string | null
  /** Immutable template-version fence learned from the PG view. */
  agentId: string | null
  view: AgentRuntimeView | null
  /** Only successful PG snapshot/reconcile may advance this barrier. */
  pgConfirmedEventSeq: string
  /** Realtime product frames above the PG-confirmed barrier. */
  unconfirmedDurableFrames: Readonly<Record<string, DurableFrame>>
  timeline: readonly TimelineEntry[]
  appliedEventSeqs: ReadonlySet<string>
  drafts: Readonly<Record<string, DraftState>>
  activeLlmCallId: string | null
  closedLlmCallIds: ReadonlySet<string>
  telemetryFloorEventSeq: string
  tools: Readonly<Record<string, ToolProgress>>
  approvals: Readonly<Record<string, ApprovalRequest>>
  historyThrough: string | null
  historyBefore: string | null
  historyHasMore: boolean
  historyLoading: boolean
  realtimeDegraded: boolean
  cancelRequested: boolean
  /** Message 202 accepted, not yet proven by view or exact durable Turn frame. */
  acceptedTurnId: string | null
  phase: "empty" | "recovering" | "ready" | "connection_error" | "missing"
  error: ApiError | null
}

export type ConversationAction =
  | { type: "runtime_selected"; agentRuntimeId: string | null }
  | { type: "recovery_started"; agentRuntimeId: string }
  | {
      type: "snapshot_loaded"
      view: AgentRuntimeView
      items: readonly AgentRuntimeDurableRecordV1[]
      historyBefore: string | null
      historyHasMore: boolean
    }
  | { type: "history_page_started" }
  | {
      type: "history_page_loaded"
      items: readonly AgentRuntimeDurableRecordV1[]
      historyBefore: string | null
      historyHasMore: boolean
    }
  | { type: "history_page_failed" }
  | { type: "durable_frame"; frame: DurableFrame }
  | { type: "telemetry_frame"; frame: TelemetryFrame }
  | {
      type: "turn_accepted"
      agentRuntimeId: string
      agentId: string
      turnId: string
    }
  | {
      type: "view_reconciled"
      basePgConfirmedEventSeq: string
      view: AgentRuntimeView
      /** Complete public product window `(base,T]`, in event order. */
      items: readonly AgentRuntimeDurableRecordV1[]
    }
  | { type: "approval_resolved"; approvalId: string }
  | { type: "cancel_requested" }
  | { type: "realtime_degraded"; degraded: boolean }
  | { type: "recovery_ready" }
  | { type: "operation_error"; error: ApiError }
  | { type: "operation_succeeded" }
  | { type: "connection_error"; error: ApiError }
  | { type: "missing"; error: ApiError | null }
