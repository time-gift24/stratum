import type {
  AgentTemplateView,
  ModelConfig,
  ModelDescriptor,
} from "@/lib/stratum/model-config"

/**
 * Stratum Agent Runtime API（Postgres-first 协议）的 REST client 与全部协议类型。
 *
 * 协议要点（openspec complete-postgres-agent-runtime）：
 * - 所有跨越边界的 event sequence 都是无符号十进制字符串（JS number 精度安全）。
 * - durable identity = (agent_id, event_seq)；telemetry identity =
 *   (llm_call_id, telemetry_seq)；SSE id 只是不透明 NATS cursor。
 * - create 需要客户端 UUID `Idempotency-Key`；message 用显式可空
 *   `expected_current_turn_id` 做 current-Turn CAS。
 * - 错误统一为 `{"error":{"code","message"}}`，客户端只分支稳定的 code。
 */

export const STRATUM_API_BASE_URL =
  process.env.NEXT_PUBLIC_STRATUM_API_BASE_URL ?? "http://127.0.0.1:18080"

export class ApiError extends Error {
  constructor(
    readonly code: string,
    readonly status: number,
    message: string
  ) {
    super(message)
    this.name = "ApiError"
  }
}

export type AgentStatus = "idle" | "running" | "finished" | "failed" | "cancelled"

export type TokenUsage = {
  input_tokens: number
  output_tokens: number
  total_tokens: number
}

export type ToolCall = { call_id: string; name: string; arguments: unknown }

export type ChatMessage = {
  role: "user" | "assistant" | "tool" | "system"
  content: { type: "text"; data: string } | { type: "json"; data: unknown }
  tool_calls?: readonly ToolCall[]
  reasoning_content?: string
  tool_call_id?: string
}

export type ApprovalDecision = "approve" | "reject"

/** AgentView.pending_approvals 与 tool_approval_requested 共享的审批视图 */
export type PendingApprovalView = {
  approval_id: string
  call_id: string
  tool_name: string
  arguments: unknown
  tool_kind: "read" | "write"
  danger_level: "low" | "medium" | "high"
}

/**
 * `GET /v1/agents/{id}` 的冷视图：除 advisory `resume_required` 外全部来自
 * 同一 Postgres MVCC snapshot；`snapshot_event_seq` 是恢复 barrier。
 */
export type AgentView = {
  agent_id: string
  agent_name: string
  status: AgentStatus
  model_config: ModelConfig
  session_id: string | null
  current_turn_id: string | null
  /** 十进制字符串 barrier，等于 snapshot 中的 agent_state.last_event_seq */
  snapshot_event_seq: string
  pending_approvals: readonly PendingApprovalView[]
  latest_usage: TokenUsage | null
  /** 进程内 advisory：running 且本进程未托管 exact current Turn */
  resume_required: boolean
}

/**
 * 公开 product event union（API 拥有的安全映射）。
 * ToolExecutionStarted 与 Hook journal events 永不发布，因此相邻可见
 * event_seq 允许有数值间隔。
 */
export type AgentProductEventV1 =
  | { type: "loop_started"; data: { extension_set_version_id?: string } }
  | { type: "message_appended"; data: { message: ChatMessage } }
  | { type: "tool_approval_requested"; data: PendingApprovalView }
  | {
      type: "tool_approval_resolved"
      data: { approval_id: string; decision: ApprovalDecision }
    }
  | {
      type: "transcript_compacted"
      data: { summary: ChatMessage; compacted_iteration: number }
    }
  | {
      type: "iteration_completed"
      data: { iteration: number; usage: TokenUsage }
    }
  | { type: "loop_finished"; data: { finish_reason: string; usage: TokenUsage } }
  | { type: "loop_failed"; data: { error_text: string; usage: TokenUsage } }
  | { type: "loop_cancelled"; data: { usage: TokenUsage } }

/** history item 与 SSE durable frame 共享的 durable record 形状 */
export type AgentDurableRecordV1 = {
  /** 十进制字符串 durable 序号 */
  event_seq: string
  event_version: number
  event: AgentProductEventV1
}

/** `GET /v1/agents/{id}/history` 响应：items 按 event_seq 升序 */
export type HistoryPage = {
  items: readonly AgentDurableRecordV1[]
  through_event_seq: string
  next_before_event_seq: string | null
  has_more: boolean
}

/** typed LLM telemetry event（call-local，不进入 durable history） */
export type LlmTelemetryEventV1 =
  | { type: "llm_started"; data: Record<string, unknown> }
  | { type: "text_delta"; data: { delta: string } }
  | { type: "reasoning_delta"; data: { delta: string } }
  | {
      type: "tool_call_delta"
      data: { call_id: string; name?: string | null; arguments_delta: string }
    }
  | {
      type: "llm_finished"
      data: { finish_reason: string; usage?: TokenUsage | null }
    }

type FrameIdentity = {
  agent_id: string
  session_id: string | null
  turn_id: string | null
  created_at: string
}

/** SSE `GET /v1/agents/{id}/events` 的唯一公开 frame（protocol_version = 1） */
export type AgentStreamFrameV1 =
  | (FrameIdentity & {
      protocol_version: 1
      kind: "control"
      event:
        | { type: "stream_ready" }
        | { type: "stream_reset"; reason: "buffer_overflow" }
    })
  | (FrameIdentity & {
      protocol_version: 1
      kind: "durable"
      event_seq: string
      event_version: number
      event: AgentProductEventV1
    })
  | (FrameIdentity & {
      protocol_version: 1
      kind: "telemetry"
      llm_call_id: string
      telemetry_seq: number
      event: LlmTelemetryEventV1
    })

export type CreateAgentResult = {
  agent_id: string
  agent_name: string
}

export type SendMessageResult = {
  agent_id: string
  session_id: string
  turn_id: string
}

export type StratumApi = {
  createAgent(input: {
    agentName: string
    modelConfig?: ModelConfig
    /** 客户端生成的 UUID；同一 pending intent 重试必须复用同一 key */
    idempotencyKey: string
  }): Promise<CreateAgentResult>
  getAgentTemplates(): Promise<readonly AgentTemplateView[]>
  getModels(): Promise<readonly ModelDescriptor[]>
  getAgent(agentId: string): Promise<AgentView>
  getHistory(
    agentId: string,
    query: { throughSeq: string; beforeSeq?: string; limit?: number }
  ): Promise<HistoryPage>
  sendMessage(
    agentId: string,
    input: {
      text: string
      /** 显式可空 CAS：首个 Turn 传 null，之后传最近 TurnId */
      expectedCurrentTurnId: string | null
      modelConfig?: ModelConfig
    }
  ): Promise<SendMessageResult>
  resume(agentId: string, turnId: string): Promise<void>
  cancel(agentId: string, turnId: string): Promise<void>
  resolveApproval(
    agentId: string,
    approvalId: string,
    input: { turnId: string; decision: ApprovalDecision }
  ): Promise<void>
}

type ApiErrorBody = { error?: { code?: unknown; message?: unknown } }

const isApiErrorBody = (value: unknown): value is ApiErrorBody =>
  typeof value === "object" && value !== null

export async function apiErrorFromResponse(
  response: Response
): Promise<ApiError> {
  try {
    const body: unknown = await response.json()
    if (
      isApiErrorBody(body) &&
      typeof body.error?.code === "string" &&
      typeof body.error.message === "string"
    ) {
      return new ApiError(body.error.code, response.status, body.error.message)
    }
  } catch {
    // Invalid or missing error JSON intentionally receives the safe fallback.
  }

  return new ApiError("http_error", response.status, "request failed")
}

/**
 * 十进制字符串 event_seq 比较（string-safe，不经 JS number）。
 * 返回负数 / 0 / 正数，语义同 comparator。
 */
export function compareEventSeq(left: string, right: string): number {
  const a = BigInt(left)
  const b = BigInt(right)
  return a < b ? -1 : a > b ? 1 : 0
}

const EVENT_SEQ_PATTERN = /^(0|[1-9]\d*)$/

export function isEventSeq(value: unknown): value is string {
  return typeof value === "string" && EVENT_SEQ_PATTERN.test(value)
}

export function createStratumApi(options: {
  baseUrl: string
  fetcher?: typeof fetch
}): StratumApi {
  const baseUrl = options.baseUrl.replace(/\/$/, "")
  const fetcher = options.fetcher ?? fetch

  const request = async <T>(path: string, init?: RequestInit): Promise<T> => {
    const response = await fetcher(`${baseUrl}${path}`, init)
    if (!response.ok) throw await apiErrorFromResponse(response)
    return response.json() as Promise<T>
  }

  const command = async (
    path: string,
    body: unknown,
    headers?: Record<string, string>
  ): Promise<void> => {
    const response = await fetcher(`${baseUrl}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json", ...headers },
      body: JSON.stringify(body),
    })
    if (!response.ok) throw await apiErrorFromResponse(response)
  }

  return {
    createAgent: async (input) => {
      const response = await fetcher(`${baseUrl}/v1/agents`, {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "idempotency-key": input.idempotencyKey,
        },
        body: JSON.stringify({
          agent_name: input.agentName,
          ...(input.modelConfig === undefined
            ? {}
            : { model_config: input.modelConfig }),
        }),
      })
      if (!response.ok) throw await apiErrorFromResponse(response)
      return response.json() as Promise<CreateAgentResult>
    },
    getAgentTemplates: async () => {
      const response = await request<{ agents: readonly AgentTemplateView[] }>(
        "/v1/agent-templates"
      )
      return response.agents
    },
    getModels: async () => {
      const response = await request<{ models: readonly ModelDescriptor[] }>(
        "/v1/models"
      )
      return response.models
    },
    getAgent: (agentId) => request(`/v1/agents/${agentId}`),
    getHistory: (agentId, query) => {
      const search = new URLSearchParams({
        through_event_seq: query.throughSeq,
        limit: String(query.limit ?? 50),
      })
      if (query.beforeSeq !== undefined)
        search.set("before_event_seq", query.beforeSeq)
      return request(`/v1/agents/${agentId}/history?${search}`)
    },
    sendMessage: async (agentId, input) => {
      const response = await fetcher(`${baseUrl}/v1/agents/${agentId}/messages`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          text: input.text,
          expected_current_turn_id: input.expectedCurrentTurnId,
          ...(input.modelConfig === undefined
            ? {}
            : { model_config: input.modelConfig }),
        }),
      })
      if (!response.ok) throw await apiErrorFromResponse(response)
      return response.json() as Promise<SendMessageResult>
    },
    resume: (agentId, turnId) =>
      command(`/v1/agents/${agentId}/resume`, { turn_id: turnId }),
    cancel: (agentId, turnId) =>
      command(`/v1/agents/${agentId}/cancel`, { turn_id: turnId }),
    resolveApproval: (agentId, approvalId, input) =>
      command(`/v1/agents/${agentId}/approvals/${approvalId}`, {
        turn_id: input.turnId,
        decision: input.decision,
      }),
  }
}
