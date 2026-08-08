"use client"

import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react"

import {
  initialConversationState,
  conversationReducer,
} from "@/features/agent-conversation/reducer"
import {
  loadOlderHistoryPage,
  reconcileConversation,
  runConversationSession,
} from "@/features/agent-conversation/recovery"
import type { ConversationState } from "@/features/agent-conversation/types"
import {
  loadRecentAgents,
  rememberRecentAgent as rememberStoredRecentAgent,
  removeRecentAgent as removeStoredRecentAgent,
  type RecentAgent,
  type StorageLike,
} from "@/lib/stratum/recent-agents"
import {
  configForModel,
  configForTemplate,
  currentThinkingLevel,
  thinkingLevels,
  withThinkingLevel,
  type AgentTemplateView,
  type ModelConfig,
  type ModelDescriptor,
} from "@/lib/stratum/model-config"
import {
  createStratumApi,
  ApiError,
  STRATUM_API_BASE_URL,
} from "@/lib/stratum/api"
import { subscribeToAgentEvents } from "@/lib/stratum/event-stream"

/** running / 待决审批 / 取消确认中 / realtime 降级时的低频 reconcile 间隔 */
const RECONCILE_POLL_INTERVAL_MS = 15_000

export type ComposerConfiguration = {
  agentTemplates: readonly AgentTemplateView[]
  models: readonly ModelDescriptor[]
  metadataLoading: boolean
  metadataError: ApiError | null
  selectedTemplate: AgentTemplateView | null
  agentName: string | null
  persistedModelConfig: ModelConfig | null
  currentModelConfig: ModelConfig | null
  selectedModelConfig: ModelConfig | null
  existingAgent: boolean
  turnRunning: boolean
  selectTemplate(template: AgentTemplateView): void
  selectModel(descriptor: ModelDescriptor): void
  setThinkingLevel(level: string): void
}

export type AgentConversation = {
  state: ConversationState
  recentAgents: readonly RecentAgent[]
  composerConfiguration: ComposerConfiguration
  selectAgent(agentId: string | null): void
  createConversation(text: string): Promise<boolean>
  sendMessage(text: string): Promise<boolean>
  resume(): Promise<void>
  cancel(): Promise<void>
  resolveApproval(
    approvalId: string,
    decision: "approve" | "reject"
  ): Promise<boolean>
  reconnect(): void
  loadOlderHistory(): void
  removeRecentAgent(agentId: string): void
}

type PendingCreate = {
  key: string
  agentName: string
  modelConfig?: ModelConfig
}

export function useAgentConversation(): AgentConversation {
  const [state, dispatch] = useReducer(
    conversationReducer,
    initialConversationState
  )
  const [agentTemplates, setAgentTemplates] = useState<
    readonly AgentTemplateView[]
  >([])
  const [models, setModels] = useState<readonly ModelDescriptor[]>([])
  const [metadataLoading, setMetadataLoading] = useState(true)
  const [metadataError, setMetadataError] = useState<ApiError | null>(null)
  const [recentAgents, setRecentAgents] = useState<readonly RecentAgent[]>([])
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null)
  const [reconnectVersion, setReconnectVersion] = useState(0)
  const [selectedTemplate, setSelectedTemplate] =
    useState<AgentTemplateView | null>(null)
  const [requestedModelConfig, setRequestedModelConfig] =
    useState<ModelConfig | null>(null)
  const [acceptedModelConfig, setAcceptedModelConfig] =
    useState<ModelConfig | null>(null)
  // 用户显式选过的 thinking 等级；跨模型切换时尽量保留（sticky）
  const [preferredThinkingLevel, setPreferredThinkingLevel] = useState<
    string | null
  >(null)
  const selectedAgentRef = useRef<string | null>(null)
  const selectionGeneration = useRef(0)
  // SSE cursor 只存当前页面内存（协议禁止跨刷新持久化）
  const cursorsRef = useRef(new Map<string, string>())
  // 结果未确定的 create 保留同一 Idempotency-Key；新 intent 才生成新 key
  const pendingCreateRef = useRef<PendingCreate | null>(null)
  // reconcile/pagination 读取最新 reducer state（命令回调里的闭包读不到）
  const stateRef = useRef(state)
  useEffect(() => {
    stateRef.current = state
  }, [state])

  useEffect(() => {
    let cancelled = false
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })

    void Promise.allSettled([api.getAgentTemplates(), api.getModels()]).then(
      ([templates, modelDescriptors]) => {
        if (cancelled) return
        if (templates.status === "fulfilled")
          setAgentTemplates(templates.value)
        if (modelDescriptors.status === "fulfilled")
          setModels(modelDescriptors.value)
        const failure =
          templates.status === "rejected"
            ? templates.reason
            : modelDescriptors.status === "rejected"
              ? modelDescriptors.reason
              : undefined
        setMetadataError(failure === undefined ? null : toApiError(failure))
        setMetadataLoading(false)
      }
    )

    const storage = browserStorage()
    if (storage) {
      // 异步调度：避免在 effect 体内同步 setState 触发级联渲染
      void Promise.resolve().then(() => {
        if (!cancelled) setRecentAgents(loadRecentAgents(storage))
      })
    }

    return () => {
      cancelled = true
    }
  }, [])

  const rememberRecentAgent = useCallback((agent: RecentAgent) => {
    const storage = browserStorage()
    if (storage) {
      rememberStoredRecentAgent(storage, agent)
      setRecentAgents(loadRecentAgents(storage))
      return
    }
    setRecentAgents((agents) => [
      agent,
      ...agents.filter((recentAgent) => recentAgent.agentId !== agent.agentId),
    ])
  }, [])

  const removeRecentAgent = useCallback((agentId: string) => {
    const storage = browserStorage()
    if (storage) {
      removeStoredRecentAgent(storage, agentId)
      setRecentAgents(loadRecentAgents(storage))
    } else {
      setRecentAgents((agents) =>
        agents.filter((agent) => agent.agentId !== agentId)
      )
    }
  }, [])

  const selectAgent = useCallback((agentId: string | null) => {
    selectionGeneration.current += 1
    selectedAgentRef.current = agentId
    // 切换会话 = state 全量重置：page cursor 一并作废，重新 cold bootstrap
    cursorsRef.current.clear()
    if (agentId === null) setSelectedTemplate(null)
    setSelectedAgentId(agentId)
    dispatch({ type: "agent_selected", agentId })
  }, [])

  // 默认 template 派生而非 effect：未选中 agent 且未显式选择时取第一个
  const effectiveTemplate =
    state.agentId === null
      ? (selectedTemplate ?? agentTemplates[0] ?? null)
      : null

  // agent 切换时在渲染期重置 pending 的模型配置（derive-state-during-render 模式）
  const [prevConfigAgentId, setPrevConfigAgentId] = useState(state.agentId)
  if (prevConfigAgentId !== state.agentId) {
    setPrevConfigAgentId(state.agentId)
    setRequestedModelConfig(null)
    setAcceptedModelConfig(null)
    setPreferredThinkingLevel(null)
  }

  useEffect(() => {
    if (selectedAgentId === null) return

    const controller = new AbortController()
    const generation = selectionGeneration.current
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
    const cursors = cursorsRef.current
    const dispatchIfCurrent = (action: Parameters<typeof dispatch>[0]) => {
      if (
        !controller.signal.aborted &&
        generation === selectionGeneration.current
      )
        dispatch(action)
    }

    void runConversationSession(
      {
        api,
        subscribe: (options) =>
          subscribeToAgentEvents({
            ...options,
            baseUrl: STRATUM_API_BASE_URL,
          }),
        loadCursor: (agentId) => cursors.get(agentId),
        saveCursor: (agentId, cursor) => {
          if (generation === selectionGeneration.current)
            cursors.set(agentId, cursor)
        },
        clearCursor: (agentId) => cursors.delete(agentId),
        dispatch: dispatchIfCurrent,
      },
      { agentId: selectedAgentId, signal: controller.signal }
    )

    return () => controller.abort()
  }, [reconnectVersion, selectedAgentId])

  const reportError = useCallback((error: unknown) => {
    const apiError = toApiError(error)
    dispatch(
      apiError.status === 404
        ? { type: "missing", error: apiError }
        : { type: "connection_error", error: apiError }
    )
  }, [])

  const reconnect = useCallback(() => {
    if (selectedAgentRef.current === null) return
    // 手动重连 = hard reset：清 page cursor，从无 cursor cold bootstrap 重来
    cursorsRef.current.delete(selectedAgentRef.current)
    selectionGeneration.current += 1
    setReconnectVersion((version) => version + 1)
  }, [])

  /** 命令返回后 / 窗口聚焦 / 低频轮询共用的增量 reconcile */
  const reconcileNow = useCallback(() => {
    const agentId = selectedAgentRef.current
    if (agentId === null) return
    const generation = selectionGeneration.current
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
    void reconcileConversation(
      {
        api,
        getBarrier: () => {
          const current = stateRef.current
          return current.agentId === agentId && current.phase === "ready"
            ? current.barrier
            : null
        },
        dispatch: (action) => {
          if (generation === selectionGeneration.current) dispatch(action)
        },
      },
      { agentId }
    )
  }, [])

  // 窗口重新获得焦点立即 reconcile
  useEffect(() => {
    const onFocus = () => reconcileNow()
    window.addEventListener("focus", onFocus)
    return () => window.removeEventListener("focus", onFocus)
  }, [reconcileNow])

  const polling =
    state.phase === "ready" &&
    state.agentId !== null &&
    (state.view?.status === "running" ||
      state.cancelRequested ||
      state.realtimeDegraded ||
      Object.keys(state.approvals).length > 0)

  // running / 待决审批 / realtime 降级期间的低频 PG reconcile
  useEffect(() => {
    if (!polling) return
    const timer = setInterval(reconcileNow, RECONCILE_POLL_INTERVAL_MS)
    return () => clearInterval(timer)
  }, [polling, reconcileNow])

  const loadOlderHistory = useCallback(() => {
    const agentId = selectedAgentRef.current
    if (agentId === null) return
    const generation = selectionGeneration.current
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
    void loadOlderHistoryPage(
      {
        api,
        getWindow: () => {
          const current = stateRef.current
          if (current.agentId !== agentId || current.historyThrough === null)
            return null
          return {
            through: current.historyThrough,
            before: current.historyBefore,
            hasMore: current.historyHasMore,
            loading: current.historyLoading,
          }
        },
        dispatch: (action) => {
          if (generation === selectionGeneration.current) dispatch(action)
        },
      },
      { agentId }
    )
  }, [])

  const createConversation = useCallback(
    async (text: string) => {
      const prompt = text.trim()
      if (prompt === "") {
        reportError(new ApiError("invalid_input", 400, "message is required"))
        return false
      }

      if (effectiveTemplate === null) {
        reportError(
          new ApiError(
            "agent_template_not_selected",
            400,
            "select an agent first"
          )
        )
        return false
      }

      const generation = selectionGeneration.current
      const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
      const modelConfig = requestedModelConfig ?? undefined
      try {
        // 同一 pending intent 复用 Idempotency-Key；只有新 intent 才生成新 key
        let pending = pendingCreateRef.current
        if (
          pending === null ||
          pending.agentName !== effectiveTemplate.agent_name ||
          !sameModelConfig(pending.modelConfig, modelConfig)
        ) {
          pending = {
            key: crypto.randomUUID(),
            agentName: effectiveTemplate.agent_name,
            modelConfig,
          }
          pendingCreateRef.current = pending
        }
        const created = await api.createAgent({
          agentName: pending.agentName,
          modelConfig: pending.modelConfig,
          idempotencyKey: pending.key,
        })
        pendingCreateRef.current = null
        if (generation !== selectionGeneration.current) return false

        rememberRecentAgent({
          agentId: created.agent_id,
          agentName: created.agent_name,
          title: prompt,
          lastOpenedAt: new Date().toISOString(),
        })
        selectAgent(created.agent_id)

        // 首个 Turn：idle Agent 的 CAS 是 expected_current_turn_id = null；
        // 只对同一 Agent 发送首条消息，不再 create
        await api.sendMessage(created.agent_id, {
          text: prompt,
          expectedCurrentTurnId: null,
          modelConfig,
        })
        if (
          modelConfig !== undefined &&
          generation === selectionGeneration.current
        ) {
          setAcceptedModelConfig(modelConfig)
          setRequestedModelConfig((pendingConfig) =>
            pendingConfig === modelConfig ? null : pendingConfig
          )
        }
        return true
      } catch (error) {
        if (generation !== selectionGeneration.current) return false
        // key 命中但请求不同 = 新 intent：清掉 pending key，下次生成新 key
        if (isApiErrorCode(error, "idempotency_key_conflict"))
          pendingCreateRef.current = null
        // stale_turn：首条消息可能已提交但响应丢失——只 reconcile 收敛，
        // 绝不重发创建第二个 Turn
        if (isApiErrorCode(error, "stale_turn")) {
          reconcileNow()
          return true
        }
        reportError(error)
        return false
      }
    },
    [
      effectiveTemplate,
      reconcileNow,
      rememberRecentAgent,
      reportError,
      requestedModelConfig,
      selectAgent,
    ]
  )

  const selectedClient = useCallback(() => {
    const agentId = selectedAgentRef.current
    if (agentId === null) {
      reportError(
        new ApiError("agent_not_selected", 400, "select an agent first")
      )
      return undefined
    }

    return {
      api: createStratumApi({ baseUrl: STRATUM_API_BASE_URL }),
      agentId,
      generation: selectionGeneration.current,
    }
  }, [reportError])

  const sendMessage = useCallback(
    async (text: string) => {
      const message = text.trim()
      if (message === "") {
        reportError(new ApiError("invalid_input", 400, "message is required"))
        return false
      }

      const view = state.view
      if (state.phase === "recovering" || view === null || view.status === "running")
        return false

      const client = selectedClient()
      if (!client) return false

      const selectedConfig = requestedModelConfig
      try {
        await client.api.sendMessage(client.agentId, {
          text: message,
          // exact current-Turn CAS：terminal 后携带最近 TurnId 才能开新 Turn
          expectedCurrentTurnId: view.current_turn_id,
          modelConfig: selectedConfig ?? undefined,
        })
        if (
          selectedConfig !== null &&
          client.generation === selectionGeneration.current
        ) {
          setAcceptedModelConfig(selectedConfig)
          setRequestedModelConfig((pendingConfig) =>
            pendingConfig === selectedConfig ? null : pendingConfig
          )
        }
        reconcileNow()
        return true
      } catch (error) {
        if (client.generation !== selectionGeneration.current) return false
        // stale_turn：view 过期，刷新后由用户重试，不静默创建第二个 Turn
        if (isApiErrorCode(error, "stale_turn")) {
          reconcileNow()
          reportError(error)
          return false
        }
        // resume_required：unhosted running Turn，reconcile 后显示 Resume
        if (isApiErrorCode(error, "resume_required")) {
          reconcileNow()
          return false
        }
        if (!isApiErrorCode(error, "agent_busy")) reportError(error)
        return false
      }
    },
    [
      reconcileNow,
      reportError,
      requestedModelConfig,
      selectedClient,
      state.phase,
      state.view,
    ]
  )

  const resume = useCallback(async () => {
    const turnId = stateRef.current.view?.current_turn_id
    const client = selectedClient()
    if (!client || turnId === undefined || turnId === null) return

    try {
      // 202 = 已托管；204 = already hosted/starting，幂等成功
      await client.api.resume(client.agentId, turnId)
      reconcileNow()
    } catch (error) {
      if (client.generation !== selectionGeneration.current) return
      // stale_turn / turn_not_running：view 已过期，reconcile 收敛
      if (
        isApiErrorCode(error, "stale_turn") ||
        isApiErrorCode(error, "turn_not_running")
      ) {
        reconcileNow()
        return
      }
      reportError(error)
    }
  }, [reconcileNow, reportError, selectedClient])

  const cancel = useCallback(async () => {
    const turnId = stateRef.current.view?.current_turn_id
    const client = selectedClient()
    if (!client || turnId === undefined || turnId === null) return

    try {
      // 202 = 信号已接受（只显示"取消请求已发送"）；204 = 已 cancelled
      await client.api.cancel(client.agentId, turnId)
      dispatch({ type: "cancel_requested" })
      reconcileNow()
    } catch (error) {
      if (client.generation !== selectionGeneration.current) return
      // turn_not_hosted / stale_turn：reconcile 后按真实 view 显示 Resume 等
      if (
        isApiErrorCode(error, "turn_not_hosted") ||
        isApiErrorCode(error, "stale_turn") ||
        isApiErrorCode(error, "turn_starting")
      ) {
        reconcileNow()
        return
      }
      reportError(error)
    }
  }, [reconcileNow, reportError, selectedClient])

  const resolveApproval = useCallback(
    async (approvalId: string, decision: "approve" | "reject") => {
      const turnId = stateRef.current.view?.current_turn_id
      const client = selectedClient()
      if (!client || turnId === undefined || turnId === null) return false

      try {
        // 204：持久化决定；unhosted Turn 由后续显式 Resume 接管（不自动 resume）
        await client.api.resolveApproval(client.agentId, approvalId, {
          turnId,
          decision,
        })
        if (client.generation !== selectionGeneration.current) return false
        dispatch({ type: "approval_resolved", approvalId })
        reconcileNow()
        return true
      } catch (error) {
        if (client.generation !== selectionGeneration.current) return false
        // approval_invalidated / already_resolved：reconcile 以 ledger 为准
        if (
          isApiErrorCode(error, "approval_invalidated") ||
          isApiErrorCode(error, "approval_already_resolved") ||
          isApiErrorCode(error, "stale_turn")
        ) {
          reconcileNow()
          return false
        }
        reportError(error)
        return false
      }
    },
    [reconcileNow, reportError, selectedClient]
  )

  const selectTemplate = useCallback(
    (template: AgentTemplateView) => {
      if (selectedAgentRef.current !== null) selectAgent(null)
      setRequestedModelConfig(null)
      setAcceptedModelConfig(null)
      setPreferredThinkingLevel(null)
      setSelectedTemplate(template)
    },
    [selectAgent]
  )

  // memo 派生：未选 agent 时 configForTemplate 每次渲染 structuredClone 会产生
  // 新引用，级联打穿下游 useCallback/memo（selectModel、ModelSelector props）
  const currentModelConfig = useMemo(
    () =>
      state.agentId === null
        ? effectiveTemplate === null
          ? null
          : configForTemplate(effectiveTemplate)
        : (acceptedModelConfig ?? state.view?.model_config ?? null),
    [
      state.agentId,
      effectiveTemplate,
      acceptedModelConfig,
      state.view?.model_config,
    ]
  )
  const persistedModelConfig = state.view?.model_config ?? null
  const selectedModelConfig = requestedModelConfig ?? currentModelConfig

  const selectModel = useCallback(
    (descriptor: ModelDescriptor) => {
      // Sticky 选择：尽量保留当前/偏好等级；新模型 schema 不含该等级时
      // 省略 thinking 参数（落回 schema default），切回支持的模型时恢复。
      const carry =
        (selectedModelConfig === null
          ? null
          : currentThinkingLevel(selectedModelConfig.parameters)) ??
        preferredThinkingLevel
      const config = configForModel(descriptor)
      const supported = thinkingLevels(descriptor.parameters_schema).some(
        (level) => level.id === carry
      )
      setRequestedModelConfig(
        carry !== null && supported
          ? { ...config, parameters: withThinkingLevel(config.parameters, carry) }
          : config
      )
    },
    [preferredThinkingLevel, selectedModelConfig]
  )

  const setThinkingLevel = useCallback(
    (level: string) => {
      if (selectedModelConfig === null) return
      setPreferredThinkingLevel(level)
      setRequestedModelConfig({
        ...selectedModelConfig,
        parameters: withThinkingLevel(selectedModelConfig.parameters, level),
      })
    },
    [selectedModelConfig]
  )

  const composerConfiguration: ComposerConfiguration = {
    agentTemplates,
    models,
    metadataLoading,
    metadataError,
    selectedTemplate: effectiveTemplate,
    agentName:
      state.agentId === null
        ? (effectiveTemplate?.agent_name ?? null)
        : (state.view?.agent_name ?? null),
    persistedModelConfig,
    currentModelConfig,
    selectedModelConfig,
    existingAgent: state.agentId !== null,
    turnRunning: state.view?.status === "running",
    selectTemplate,
    selectModel,
    setThinkingLevel,
  }

  return {
    state,
    recentAgents,
    composerConfiguration,
    selectAgent,
    createConversation,
    sendMessage,
    resume,
    cancel,
    resolveApproval,
    reconnect,
    loadOlderHistory,
    removeRecentAgent,
  }
}

function browserStorage(): StorageLike | undefined {
  if (typeof window === "undefined") return undefined

  try {
    return window.localStorage
  } catch {
    return undefined
  }
}

function isApiErrorCode(error: unknown, code: string): boolean {
  return error instanceof ApiError && error.code === code
}

function sameModelConfig(
  left: ModelConfig | undefined,
  right: ModelConfig | undefined
): boolean {
  if (left === undefined || right === undefined) return left === right
  return (
    left.model === right.model &&
    JSON.stringify(left.parameters) === JSON.stringify(right.parameters)
  )
}

function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error
  return new ApiError(
    "command_failed",
    0,
    error instanceof Error ? error.message : "command failed"
  )
}
