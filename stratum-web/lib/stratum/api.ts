import type {
  AgentTemplateView,
  ModelConfig,
  ModelDescriptor,
} from "@/lib/stratum/model-config"
import {
  parseAgentRuntimeCreated,
  parseAgentRuntimeHistoryPage,
  parseAgentRuntimeTurnAccepted,
  parseAgentRuntimeView,
  parseAgentTemplatesResponse,
  parseModelsResponse,
} from "@/lib/stratum/protocol-codec"

/**
 * Stratum Agent Runtime API（Postgres-first 协议）的 REST client 与协议类型。
 *
 * `AgentRuntimeId` 标识长期运行聚合；`AgentId` 只标识它 pin 住的不可变
 * template version。durable identity 是 `(agent_runtime_id,event_seq)`；SSE
 * cursor 只是不透明、页面内存级的 NATS transport position。
 */
import type {
  OntologyDocument,
  OntologyListPage,
  OntologyNeighborhood,
  OntologyViolation,
} from "@/features/ontology-editor/types"

export const STRATUM_API_BASE_URL =
  process.env.NEXT_PUBLIC_STRATUM_API_BASE_URL ?? "http://127.0.0.1:18080"

export class ApiError extends Error {
  readonly details: {
    violations: readonly FieldViolation[]
    blockers: readonly ResourceBlocker[]
  }

  constructor(
    readonly code: string,
    readonly status: number,
    message: string,
    readonly violations?: readonly OntologyViolation[]
  ) {
    super(message)
    this.name = "ApiError"
    this.details = { violations: [], blockers: [] }
  }
}

export type AgentRuntimeStatus =
  "idle" | "running" | "finished" | "failed" | "cancelled"

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

/** AgentRuntimeView.pending_approvals 与 product Requested 共享的安全视图。 */
export type PendingApprovalView = {
  /** Requested row 的 AgentRuntime-wide event sequence。 */
  requested_event_seq: string
  approval_id: string
  call_id: string
  tool_name: string
  arguments: unknown
  tool_kind: "read" | "write"
  danger_level: "low" | "medium" | "high"
}

/** `GET /v1/agent-runtimes/{agent_runtime_id}` 的固定 PG 屏障视图。 */
export type AgentRuntimeView = {
  agent_runtime_id: string
  /** Pinned immutable template-version identity (`agents.id`). */
  agent_id: string
  agent_name: string
  agent_version: string
  status: AgentRuntimeStatus
  model_config: ModelConfig
  session_id: string | null
  current_turn_id: string | null
  snapshot_event_seq: string
  telemetry_floor_event_seq: string
  pending_approvals: readonly PendingApprovalView[]
  latest_usage: TokenUsage | null
  /** Process-local advisory; it is not a durable state field. */
  resume_required: boolean
}

/** History 与 SSE durable frame 共享的完整、安全 product union。 */
export type AgentRuntimeProductEventV1 =
  | { type: "loop_started" }
  | { type: "message_appended"; data: { message: ChatMessage } }
  | {
      type: "tool_approval_requested"
      data: Omit<PendingApprovalView, "requested_event_seq">
    }
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
  | {
      type: "loop_finished"
      data: { finish_reason: string; usage: TokenUsage }
    }
  | { type: "loop_failed"; data: { error_text: string; usage: TokenUsage } }
  | { type: "loop_cancelled"; data: { usage: TokenUsage } }

/** One product-visible durable row returned by history. */
export type AgentRuntimeDurableRecordV1 = {
  event_seq: string
  event_version: number
  session_id: string
  turn_id: string
  created_at: string
  event: AgentRuntimeProductEventV1
}

export type AgentRuntimeHistoryPage = {
  items: readonly AgentRuntimeDurableRecordV1[]
  through_event_seq: string
  next_before_event_seq: string | null
  has_more: boolean
}

/** Typed, volatile LLM telemetry; it never enters durable history. */
export type LlmTelemetryEventV1 =
  | { type: "llm_started" }
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

type BaseFrameIdentity = {
  agent_runtime_id: string
  /** Pinned immutable template-version identity. */
  agent_id: string
  created_at: string
}

type ControlFrameIdentity = BaseFrameIdentity & {
  session_id: string | null
  turn_id: string | null
}

type TurnFrameIdentity = BaseFrameIdentity & {
  session_id: string
  turn_id: string
}

/** The only public frame of an AgentRuntime SSE tail. */
export type AgentRuntimeStreamFrameV1 =
  | (ControlFrameIdentity & {
      protocol_version: 1
      kind: "control"
      event:
        | { type: "stream_ready" }
        | { type: "stream_reset"; reason: "buffer_overflow" }
    })
  | (TurnFrameIdentity & {
      protocol_version: 1
      kind: "durable"
      event_seq: string
      event_version: number
      event: AgentRuntimeProductEventV1
    })
  | (TurnFrameIdentity & {
      protocol_version: 1
      kind: "telemetry"
      llm_call_id: string
      /** Call-local unsigned decimal sequence. */
      telemetry_seq: string
      durable_before_event_seq: string
      event: LlmTelemetryEventV1
    })

/** Immutable result of key-only AgentRuntime creation. */
export type AgentRuntimeCreated = {
  agent_runtime_id: string
  agent_id: string
  agent_name: string
  agent_version: string
  created_at: string
}

export type ProviderKind = "openai" | "deepseek"
export type AgentDefinitionInput = {
  agent_name: string
  agent_version: string
  model: string
  model_parameters: Record<string, unknown>
  tools: string[]
  prompt: string
}
export type AgentDefinitionView = AgentDefinitionInput & { updated_at: string }
export type ProviderView = {
  provider: ProviderKind
  credential_configured: boolean
  models_count: number
  updated_at: string
}
export type ManagedModelView = {
  model_id: string
  provider: ProviderKind
  name: string
  parameter_schema: unknown
  updated_at: string
  is_default: boolean
}
export type FieldViolation = { field: string; code: string; message: string }
export type ResourceBlocker = { resource_type: string; name: string }
export type PageEnvelope<T> = {
  data: readonly T[]
  pagination: { page: number; per_page: number; total: number; total_pages: number }
}
export type StudioResource<T> = { data: T; etag: string; location: string | null }

export type AgentRuntimeTurnAccepted = {
  agent_runtime_id: string
  agent_id: string
  session_id: string
  turn_id: string
}

export type StratumApi = {
  createAgentRuntime(input: {
    agentName: string
    modelConfig?: ModelConfig
    /** A pending create intent reuses this UUID until the outcome is known. */
    idempotencyKey: string
  }): Promise<AgentRuntimeCreated>
  getAgentTemplates(): Promise<readonly AgentTemplateView[]>
  getModels(): Promise<readonly ModelDescriptor[]>
  getAgentRuntime(
    agentRuntimeId: string,
    options?: { signal?: AbortSignal }
  ): Promise<AgentRuntimeView>
  getAgentRuntimeHistory(
    agentRuntimeId: string,
    query: { throughSeq: string; beforeSeq?: string; limit?: number },
    options?: { signal?: AbortSignal }
  ): Promise<AgentRuntimeHistoryPage>
  sendMessage(
    agentRuntimeId: string,
    input: {
      text: string
      expectedCurrentTurnId: string | null
      sessionId?: string
      modelConfig?: ModelConfig
    }
  ): Promise<AgentRuntimeTurnAccepted>
  resume(
    agentRuntimeId: string,
    turnId: string
  ): Promise<AgentRuntimeTurnAccepted | null>
  cancel(agentRuntimeId: string, turnId: string): Promise<void>
  resolveApproval(
    agentRuntimeId: string,
    approvalId: string,
    input: { turnId: string; decision: ApprovalDecision }
  ): Promise<void>
  listOntologies(query?: {
    page?: number
    perPage?: number
    sort?: string
  }): Promise<OntologyListPage>
  createOntology(input: {
    name: string
    displayName: string
    description?: string
  }): Promise<OntologyResource>
  getOntology(ontologyId: string): Promise<OntologyResource>
  replaceOntology(
    ontologyId: string,
    document: OntologyDocument,
    etag: string
  ): Promise<{ etag: string }>
  deleteOntology(ontologyId: string, etag: string): Promise<void>
  getObjectTypeNeighborhood(
    ontologyId: string,
    objectTypeId: string,
    depth?: number
  ): Promise<OntologyNeighborhood>
  listAgentDefinitions(query?: { page?: number; perPage?: number; search?: string; sort?: string }): Promise<PageEnvelope<AgentDefinitionView>>
  createAgentDefinition(input: AgentDefinitionInput): Promise<StudioResource<AgentDefinitionView>>
  getAgentDefinition(name: string): Promise<StudioResource<AgentDefinitionView>>
  updateAgentDefinition(name: string, input: Omit<AgentDefinitionInput, "agent_name">, etag: string): Promise<StudioResource<AgentDefinitionView>>
  deleteAgentDefinition(name: string, etag: string): Promise<void>
  listProviders(query?: { page?: number; perPage?: number }): Promise<PageEnvelope<ProviderView>>
  createProvider(input: { provider: ProviderKind; api_key?: string }): Promise<StudioResource<ProviderView>>
  getProvider(provider: ProviderKind): Promise<StudioResource<ProviderView>>
  updateProvider(provider: ProviderKind, input: { api_key?: string }, etag: string): Promise<StudioResource<ProviderView>>
  deleteProvider(provider: ProviderKind, etag: string): Promise<void>
  listProviderModels(provider: ProviderKind, query?: { page?: number; perPage?: number }): Promise<PageEnvelope<ManagedModelView>>
  createProviderModel(provider: ProviderKind, input: { name: string }): Promise<StudioResource<ManagedModelView>>
  getProviderModel(provider: ProviderKind, name: string): Promise<StudioResource<ManagedModelView>>
  deleteProviderModel(provider: ProviderKind, name: string, etag: string): Promise<void>
  listManagedModels(query?: { page?: number; perPage?: number; search?: string }): Promise<PageEnvelope<ManagedModelView>>
  getManagedModel(provider: ProviderKind, name: string): Promise<StudioResource<ManagedModelView>>
  createManagedModel(input: { provider: ProviderKind; name: string }): Promise<StudioResource<ManagedModelView>>
  deleteManagedModel(provider: ProviderKind, name: string, etag: string): Promise<void>
  testProvider(provider: ProviderKind): Promise<{ success: boolean; completed_at: string; message?: string }>
}

// 携带强 ETag 的 Ontology 资源读取结果（GET / POST 201）。
export type OntologyResource = {
  document: OntologyDocument
  etag: string
  location: string | null
}

type ApiErrorBody = {
  error?: { code?: unknown; message?: unknown; violations?: unknown }
}

const isApiErrorBody = (value: unknown): value is ApiErrorBody =>
  typeof value === "object" && value !== null

const isOntologyViolation = (value: unknown): value is OntologyViolation => {
  if (typeof value !== "object" || value === null) return false
  const violation = value as Record<string, unknown>
  return (
    typeof violation.code === "string" &&
    typeof violation.path === "string" &&
    typeof violation.message === "string"
  )
}

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
      const rawViolations = body.error.violations
      // 过滤后为空时归一为 undefined：消费方以 `!== undefined` 判断是否存在 violations
      const filtered = Array.isArray(rawViolations)
        ? rawViolations.filter(isOntologyViolation)
        : undefined
      const violations =
        filtered !== undefined && filtered.length > 0 ? filtered : undefined
      return new ApiError(
        body.error.code,
        response.status,
        body.error.message,
        violations
      )
    }
  } catch {
    // Invalid or missing error JSON intentionally receives the safe fallback.
  }

  return new ApiError("http_error", response.status, "request failed")
}

/** String-safe unsigned decimal sequence comparator. */
export function compareEventSeq(left: string, right: string): number {
  const a = BigInt(left)
  const b = BigInt(right)
  return a < b ? -1 : a > b ? 1 : 0
}

const EVENT_SEQ_PATTERN = /^(0|[1-9]\d*)$/

export function isEventSeq(value: unknown): value is string {
  return typeof value === "string" && EVENT_SEQ_PATTERN.test(value)
}

export function incrementEventSeq(value: string): string {
  return (BigInt(value) + BigInt(1)).toString()
}

type Parser<T> = (value: unknown) => T | undefined

export function createStratumApi(options: {
  baseUrl: string
  fetcher?: typeof fetch
}): StratumApi {
  const baseUrl = options.baseUrl.replace(/\/$/, "")
  const fetcher = options.fetcher ?? fetch

  const request = async <T>(
    path: string,
    parser: Parser<T>,
    init?: RequestInit,
    expectedStatuses: readonly number[] = [200]
  ): Promise<T> => {
    const response = await fetcher(`${baseUrl}${path}`, init)
    if (!response.ok) throw await apiErrorFromResponse(response)
    assertResponseStatus(response, expectedStatuses)
    return parseSuccessResponse(response, parser)
  }

  const emptyCommand = async (
    path: string,
    body: unknown,
    expectedStatuses: readonly number[]
  ): Promise<void> => {
    const response = await fetcher(`${baseUrl}${path}`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    })
    if (!response.ok) throw await apiErrorFromResponse(response)
    assertResponseStatus(response, expectedStatuses)
  }

  // Ontology 契约通过 ETag 头暴露强验证器；缺失即视为契约破坏。
  const readEtag = (response: Response): string => {
    const etag = response.headers.get("etag")
    if (etag === null || etag === "")
      throw new ApiError(
        "invalid_response",
        response.status,
        "response is missing the etag header"
      )
    return etag
  }

  const readOntologyResource = async (
    response: Response
  ): Promise<OntologyResource> => {
    if (!response.ok) throw await apiErrorFromResponse(response)
    const etag = readEtag(response)
    const document = (await response.json()) as OntologyDocument
    return { document, etag, location: response.headers.get("location") }
  }
  const readStudioResource = async <T>(response: Response): Promise<StudioResource<T>> => {
    if (!response.ok) throw await apiErrorFromResponse(response)
    return { data: (await response.json()) as T, etag: readEtag(response), location: response.headers.get("location") }
  }
  const studioPage = <T>(path: string) => request(path, (value) => value as PageEnvelope<T>)
  const studioWrite = async <T>(path: string, method: "POST" | "PUT", body: unknown, etag?: string) =>
    readStudioResource<T>(await fetcher(`${baseUrl}${path}`, { method, headers: { "content-type": "application/json", ...(etag === undefined ? {} : { "if-match": etag }) }, body: JSON.stringify(body) }))
  const studioDelete = async (path: string, etag: string) => {
    const response = await fetcher(`${baseUrl}${path}`, { method: "DELETE", headers: { "if-match": etag } })
    if (!response.ok) throw await apiErrorFromResponse(response)
  }

  return {
    createAgentRuntime: async (input) => {
      const response = await fetcher(`${baseUrl}/v1/agent-runtimes`, {
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
      assertResponseStatus(response, [201])
      return parseSuccessResponse(response, parseAgentRuntimeCreated)
    },
    getAgentTemplates: async () => {
      const response = await request(
        "/v1/agent-templates",
        parseAgentTemplatesResponse
      )
      return response.templates
    },
    getModels: async () => {
      const response = await request("/v1/models", parseModelsResponse)
      return response.models
    },
    getAgentRuntime: (agentRuntimeId, options) =>
      request(
        `/v1/agent-runtimes/${encodeURIComponent(agentRuntimeId)}`,
        parseAgentRuntimeView,
        { signal: options?.signal }
      ),
    getAgentRuntimeHistory: (agentRuntimeId, query, options) => {
      const search = new URLSearchParams({
        through_event_seq: query.throughSeq,
        limit: String(query.limit ?? 50),
      })
      if (query.beforeSeq !== undefined)
        search.set("before_event_seq", query.beforeSeq)
      return request(
        `/v1/agent-runtimes/${encodeURIComponent(agentRuntimeId)}/history?${search}`,
        parseAgentRuntimeHistoryPage,
        { signal: options?.signal }
      )
    },
    sendMessage: async (agentRuntimeId, input) => {
      const response = await fetcher(
        `${baseUrl}/v1/agent-runtimes/${encodeURIComponent(agentRuntimeId)}/messages`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({
            text: input.text,
            expected_current_turn_id: input.expectedCurrentTurnId,
            ...(input.sessionId === undefined
              ? {}
              : { session_id: input.sessionId }),
            ...(input.modelConfig === undefined
              ? {}
              : { model_config: input.modelConfig }),
          }),
        }
      )
      if (!response.ok) throw await apiErrorFromResponse(response)
      assertResponseStatus(response, [202])
      const accepted = await parseSuccessResponse(
        response,
        parseAgentRuntimeTurnAccepted
      )
      assertRuntimeIdentity(agentRuntimeId, accepted.agent_runtime_id)
      return accepted
    },
    resume: async (agentRuntimeId, turnId) => {
      const response = await fetcher(
        `${baseUrl}/v1/agent-runtimes/${encodeURIComponent(agentRuntimeId)}/resume`,
        {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ turn_id: turnId }),
        }
      )
      if (!response.ok) throw await apiErrorFromResponse(response)
      if (response.status === 204) return null
      assertResponseStatus(response, [202])
      const accepted = await parseSuccessResponse(
        response,
        parseAgentRuntimeTurnAccepted
      )
      assertRuntimeIdentity(agentRuntimeId, accepted.agent_runtime_id)
      if (accepted.turn_id !== turnId)
        throw new ApiError(
          "protocol_identity_error",
          0,
          "the response belongs to a different turn"
        )
      return accepted
    },
    cancel: (agentRuntimeId, turnId) =>
      emptyCommand(
        `/v1/agent-runtimes/${encodeURIComponent(agentRuntimeId)}/cancel`,
        {
          turn_id: turnId,
        },
        [202, 204]
      ),
    resolveApproval: (agentRuntimeId, approvalId, input) =>
      emptyCommand(
        `/v1/agent-runtimes/${encodeURIComponent(agentRuntimeId)}/approvals/${encodeURIComponent(approvalId)}`,
        { turn_id: input.turnId, decision: input.decision },
        [204]
      ),
    listOntologies: (query) => {
      const search = new URLSearchParams()
      if (query?.page !== undefined) search.set("page", String(query.page))
      if (query?.perPage !== undefined)
        search.set("per_page", String(query.perPage))
      if (query?.sort !== undefined) search.set("sort", query.sort)
      const suffix = search.size === 0 ? "" : `?${search}`
      return request(
        `/v1/ontologies${suffix}`,
        (value) => value as OntologyListPage
      )
    },
    createOntology: async (input) => {
      const response = await fetcher(`${baseUrl}/v1/ontologies`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          name: input.name,
          display_name: input.displayName,
          ...(input.description === undefined
            ? {}
            : { description: input.description }),
        }),
      })
      return readOntologyResource(response)
    },
    getOntology: async (ontologyId) => {
      const response = await fetcher(`${baseUrl}/v1/ontologies/${ontologyId}`)
      return readOntologyResource(response)
    },
    replaceOntology: async (ontologyId, document, etag) => {
      const response = await fetcher(`${baseUrl}/v1/ontologies/${ontologyId}`, {
        method: "PUT",
        headers: { "content-type": "application/json", "if-match": etag },
        body: JSON.stringify(document),
      })
      if (!response.ok) throw await apiErrorFromResponse(response)
      return { etag: readEtag(response) }
    },
    deleteOntology: async (ontologyId, etag) => {
      const response = await fetcher(`${baseUrl}/v1/ontologies/${ontologyId}`, {
        method: "DELETE",
        headers: { "if-match": etag },
      })
      if (!response.ok) throw await apiErrorFromResponse(response)
    },
    getObjectTypeNeighborhood: (ontologyId, objectTypeId, depth) => {
      const suffix =
        depth === undefined ? "" : `?${new URLSearchParams({ depth: String(depth) })}`
      return request(
        `/v1/ontologies/${ontologyId}/object-types/${objectTypeId}/neighborhood${suffix}`,
        (value) => value as OntologyNeighborhood
      )
    },
    listAgentDefinitions: async (query) => {
      const params = new URLSearchParams()
      if (query?.page !== undefined) params.set("page", String(query.page))
      if (query?.perPage !== undefined) params.set("per_page", String(query.perPage))
      if (query?.search !== undefined) params.set("search", query.search)
      const page = await studioPage<AgentDefinitionView>(`/v1/agent-definitions${params.size === 0 ? "" : `?${params}`}`)
      return { ...page, pagination: { ...page.pagination, total_pages: Math.max(1, Math.ceil(page.pagination.total / page.pagination.per_page)) } }
    },
    createAgentDefinition: (input) => studioWrite<AgentDefinitionView>("/v1/agent-definitions", "POST", input),
    getAgentDefinition: async (name) => readStudioResource<AgentDefinitionView>(await fetcher(`${baseUrl}/v1/agent-definitions/${encodeURIComponent(name)}`)),
    updateAgentDefinition: (name, input, etag) =>
      studioWrite<AgentDefinitionView>(
        `/v1/agent-definitions/${encodeURIComponent(name)}`,
        "PUT",
        input,
        etag
      ),
    deleteAgentDefinition: (name, etag) => studioDelete(`/v1/agent-definitions/${encodeURIComponent(name)}`, etag),
    listProviders: async (query) => {
      const params = new URLSearchParams()
      if (query?.page !== undefined) params.set("page", String(query.page))
      if (query?.perPage !== undefined) params.set("per_page", String(query.perPage))
      const page = await studioPage<ProviderView>(`/v1/providers${params.size === 0 ? "" : `?${params}`}`)
      return { ...page, pagination: { ...page.pagination, total_pages: Math.max(1, Math.ceil(page.pagination.total / page.pagination.per_page)) } }
    },
    createProvider: (input) => studioWrite<ProviderView>("/v1/providers", "POST", input),
    getProvider: async (provider) => readStudioResource<ProviderView>(await fetcher(`${baseUrl}/v1/providers/${provider}`)),
    updateProvider: (provider, input, etag) => studioWrite<ProviderView>(`/v1/providers/${provider}`, "PUT", input, etag),
    deleteProvider: (provider, etag) => studioDelete(`/v1/providers/${provider}`, etag),
    listProviderModels: async (provider, query) => {
      const params = new URLSearchParams()
      if (query?.page !== undefined) params.set("page", String(query.page))
      if (query?.perPage !== undefined) params.set("per_page", String(query.perPage))
      const page = await studioPage<ManagedModelView>(`/v1/providers/${provider}/models${params.size === 0 ? "" : `?${params}`}`)
      return { ...page, data: page.data.map((model) => ({ ...model, is_default: false })), pagination: { ...page.pagination, total_pages: Math.max(1, Math.ceil(page.pagination.total / page.pagination.per_page)) } }
    },
    createProviderModel: async (provider, input) => {
      const resource = await studioWrite<ManagedModelView>(`/v1/providers/${provider}/models`, "POST", input)
      return { ...resource, data: { ...resource.data, is_default: false } }
    },
    getProviderModel: async (provider, name) => {
      const resource = await readStudioResource<ManagedModelView>(await fetcher(`${baseUrl}/v1/providers/${provider}/models/${encodeURIComponent(name)}`))
      return { ...resource, data: { ...resource.data, is_default: false } }
    },
    deleteProviderModel: (provider, name, etag) => studioDelete(`/v1/providers/${provider}/models/${encodeURIComponent(name)}`, etag),
    listManagedModels: async (query) => {
      const providers = await studioPage<ProviderView>("/v1/providers?per_page=100")
      const pages = await Promise.all(providers.data.map((provider) => studioPage<ManagedModelView>(`/v1/providers/${provider.provider}/models?per_page=100`)))
      const all = pages.flatMap((page) => page.data).map((model) => ({ ...model, is_default: false })).filter((model) => query?.search === undefined || model.model_id.toLowerCase().includes(query.search.toLowerCase()))
      const perPage = query?.perPage ?? 20
      const page = query?.page ?? 1
      return { data: all.slice((page - 1) * perPage, page * perPage), pagination: { page, per_page: perPage, total: all.length, total_pages: Math.max(1, Math.ceil(all.length / perPage)) } }
    },
    getManagedModel: (provider, name) => createStratumApi({ baseUrl, fetcher }).getProviderModel(provider, name),
    createManagedModel: (input) => createStratumApi({ baseUrl, fetcher }).createProviderModel(input.provider, { name: input.name }),
    deleteManagedModel: (provider, name, etag) => studioDelete(`/v1/providers/${provider}/models/${encodeURIComponent(name)}`, etag),
    testProvider: async () => {
      throw new ApiError("unsupported", 501, "provider connection testing is unavailable")
    },
  }
}

async function parseSuccessResponse<T>(
  response: Response,
  parser: Parser<T>
): Promise<T> {
  let value: unknown
  try {
    value = await response.json()
  } catch {
    throw new ApiError(
      "invalid_response",
      response.status,
      "the server returned an invalid response"
    )
  }
  const parsed = parser(value)
  if (parsed === undefined)
    throw new ApiError(
      "invalid_response",
      response.status,
      "the server returned an unsupported response"
    )
  return parsed
}

function assertResponseStatus(
  response: Response,
  expectedStatuses: readonly number[]
): void {
  if (!expectedStatuses.includes(response.status))
    throw new ApiError(
      "invalid_response",
      response.status,
      "the server returned an unexpected success status"
    )
}

function assertRuntimeIdentity(expected: string, actual: string): void {
  if (expected !== actual)
    throw new ApiError(
      "protocol_identity_error",
      0,
      "the response belongs to a different agent runtime"
    )
}
