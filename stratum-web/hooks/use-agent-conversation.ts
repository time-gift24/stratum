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
import {
  resolveMessageIntent,
  sameModelConfig,
  type PendingMessageIntent,
} from "@/features/agent-conversation/message-intent"
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
  // message 没有独立幂等键：响应不确定时，同一 intent 必须复用原 exact CAS。
  const pendingMessageRef = useRef<PendingMessageIntent | null>(null)
  // reconcile 串行执行；timer / focus / command 只合并一次补跑，
  // 避免大分页请求被下一次轮询反复取消。
  const reconcileAbortRef = useRef<AbortController | null>(null)
  const reconcileRerunRef = useRef(false)
  const historyAbortRef = useRef<AbortController | null>(null)
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
        if (templates.status === "fulfilled") setAgentTemplates(templates.value)
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
    reconcileRerunRef.current = false
    reconcileAbortRef.current?.abort()
    reconcileAbortRef.current = null
    historyAbortRef.current?.abort()
    historyAbortRef.current = null
    selectionGeneration.current += 1
    selectedAgentRef.current = agentId
    // 切换会话 = state 全量重置：page cursor 一并作废，重新 cold bootstrap
    cursorsRef.current.clear()
    if (pendingMessageRef.current?.agentId !== agentId)
      pendingMessageRef.current = null
    if (agentId === null) setSelectedTemplate(null)
    setSelectedAgentId(agentId)
    dispatch({ type: "agent_selected", agentId })
  }, [])

  useEffect(
    () => () => {
      reconcileRerunRef.current = false
      reconcileAbortRef.current?.abort()
      reconcileAbortRef.current = null
      historyAbortRef.current?.abort()
    },
    []
  )

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
    reconcileRerunRef.current = false
    reconcileAbortRef.current?.abort()
    reconcileAbortRef.current = null
    historyAbortRef.current?.abort()
    historyAbortRef.current = null
    // 手动重连 = hard reset：清 page cursor，从无 cursor cold bootstrap 重来
    cursorsRef.current.delete(selectedAgentRef.current)
    selectionGeneration.current += 1
    setReconnectVersion((version) => version + 1)
  }, [])

  /** 命令返回后 / 窗口聚焦 / 低频轮询共用的增量 reconcile */
  const reconcileNow = useCallback(() => {
    const agentId = selectedAgentRef.current
    if (agentId === null) return

    // 活跃请求不可被 timer / focus / command 取消；多个触发只需
    // 在它完成后补跑一次，以最新 reducer barrier 收敛。
    if (reconcileAbortRef.current !== null) {
      reconcileRerunRef.current = true
      return
    }

    const generation = selectionGeneration.current

    const run = () => {
      if (
        selectedAgentRef.current !== agentId ||
        generation !== selectionGeneration.current
      )
        return

      const controller = new AbortController()
      reconcileAbortRef.current = controller
      const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
      const isCurrent = () =>
        !controller.signal.aborted &&
        reconcileAbortRef.current === controller &&
        selectedAgentRef.current === agentId &&
        generation === selectionGeneration.current

      void reconcileConversation(
        {
          api,
          getBarrier: () => {
            const current = stateRef.current
            return current.agentId === agentId && current.phase === "ready"
              ? current.barrier
              : null
          },
          isCurrent,
          dispatch: (action) => {
            if (isCurrent()) dispatch(action)
          },
        },
        { agentId, signal: controller.signal }
      ).finally(() => {
        // Agent switch / hard reset / unmount 会先换掉这个 handle；旧请求
        // 不得清理新 Agent 的状态，也不得触发补跑。
        if (reconcileAbortRef.current !== controller) return
        reconcileAbortRef.current = null
        if (!reconcileRerunRef.current) return
        reconcileRerunRef.current = false
        run()
      })
    }

    run()
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
      state.acceptedTurnId !== null ||
      state.cancelRequested ||
      state.realtimeDegraded ||
      Object.keys(state.approvals).length > 0)

  // 进入需要 PG 收敛的状态时立即跑一次，之后才低频轮询。首创 Agent 的
  // message 202 可能早于 cold bootstrap ready；acceptedTurnId 会把这次
  // immediate reconcile 延迟到 barrier 可读时，而不是等第一个 interval。
  useEffect(() => {
    if (!polling) return
    reconcileNow()
    const timer = setInterval(reconcileNow, RECONCILE_POLL_INTERVAL_MS)
    return () => clearInterval(timer)
  }, [polling, reconcileNow])

  const loadOlderHistory = useCallback(() => {
    const agentId = selectedAgentRef.current
    if (agentId === null) return
    // The reducer flag is updated on the next render; this ref also closes the
    // same-tick double-click window and owns cancellation on Agent switches.
    if (historyAbortRef.current !== null) return
    const controller = new AbortController()
    historyAbortRef.current = controller
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
      { agentId, signal: controller.signal }
    ).finally(() => {
      if (historyAbortRef.current === controller) historyAbortRef.current = null
    })
  }, [])

  const createConversation = useCallback(
    async (text: string) => {
      // 合同：trim 只用于空判定，发送与持久化一律用原文
      if (text.trim() === "") {
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
      // selectAgent 会推进 generation；记录推进后的值，既不把自身切换误判为
      // stale，也不允许随后用户切换 Agent 时由旧请求污染新会话。
      let selectedGeneration: number | null = null
      let createdAgentId: string | null = null
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
        if (pendingCreateRef.current === pending)
          pendingCreateRef.current = null
        if (generation !== selectionGeneration.current) return false

        rememberRecentAgent({
          agentId: created.agent_id,
          agentName: created.agent_name,
          title: text.trim(),
          lastOpenedAt: new Date().toISOString(),
        })
        selectAgent(created.agent_id)
        createdAgentId = created.agent_id
        selectedGeneration = selectionGeneration.current

        const messageIntent: PendingMessageIntent = {
          agentId: created.agent_id,
          text,
          expectedCurrentTurnId: null,
          modelConfig,
        }
        pendingMessageRef.current = messageIntent

        // 首个 Turn：idle Agent 的 CAS 是 expected_current_turn_id = null；
        // 只对同一 Agent 发送首条消息，不再 create
        const accepted = await api.sendMessage(created.agent_id, {
          text: messageIntent.text,
          expectedCurrentTurnId: messageIntent.expectedCurrentTurnId,
          modelConfig: messageIntent.modelConfig,
        })
        if (
          selectedGeneration !== selectionGeneration.current ||
          selectedAgentRef.current !== created.agent_id
        )
          return false
        dispatch({
          type: "turn_accepted",
          agentId: created.agent_id,
          turnId: accepted.turn_id,
        })
        if (pendingMessageRef.current === messageIntent)
          pendingMessageRef.current = null
        if (
          messageIntent.modelConfig !== undefined &&
          selectedAgentRef.current === created.agent_id
        ) {
          setAcceptedModelConfig(messageIntent.modelConfig)
          setRequestedModelConfig((pendingConfig) =>
            sameModelConfig(
              pendingConfig ?? undefined,
              messageIntent.modelConfig
            )
              ? null
              : pendingConfig
          )
        }
        reconcileNow()
        return true
      } catch (error) {
        if (selectedGeneration === null || createdAgentId === null) {
          if (generation !== selectionGeneration.current) return false
          // key 命中但请求不同 = 新 intent：清掉 pending key，下次生成新 key
          if (isApiErrorCode(error, "idempotency_key_conflict"))
            pendingCreateRef.current = null
          reportError(error)
          return false
        }
        if (
          selectedGeneration !== selectionGeneration.current ||
          selectedAgentRef.current !== createdAgentId
        )
          return false
        // 首条消息失败（selectAgent 已推进 generation）：必须显式 surface，
        // 不能静默返回。新 agent 的 recovery session 会 cold bootstrap 收敛
        // view；stale_turn 表示消息可能已提交但响应丢失——只 reconcile，
        // 绝不重发创建第二个 Turn
        if (isApiErrorCode(error, "stale_turn")) {
          reconcileNow()
          return false
        }
        reportError(error)
        reconcileNow()
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
      // 合同：trim 只用于空判定，发送与持久化一律用原文
      if (text.trim() === "") {
        reportError(new ApiError("invalid_input", 400, "message is required"))
        return false
      }

      const view = state.view
      if (state.phase === "recovering" || view === null) return false

      const client = selectedClient()
      if (!client) return false

      const selectedConfig = requestedModelConfig
      const existingIntent = pendingMessageRef.current
      // create 后 hook 会按 Agent 切换规则重置 requestedModelConfig；这不是用户
      // 新 intent。同 Agent + 同原文且没有新的显式 override 时继续沿用 pending
      // intent 的完整模型配置与 CAS。
      const submittedModelConfig =
        existingIntent?.agentId === client.agentId &&
        existingIntent.text === text &&
        selectedConfig === null
          ? existingIntent.modelConfig
          : (selectedConfig ?? undefined)
      const messageIntent = resolveMessageIntent(existingIntent, {
        agentId: client.agentId,
        text,
        expectedCurrentTurnId: view.current_turn_id,
        modelConfig: submittedModelConfig,
      })
      const retryingPendingIntent = messageIntent === existingIntent
      if (view.status === "running" && !retryingPendingIntent) return false
      pendingMessageRef.current = messageIntent
      try {
        const accepted = await client.api.sendMessage(client.agentId, {
          text: messageIntent.text,
          // exact current-Turn CAS：terminal 后携带最近 TurnId 才能开新 Turn
          expectedCurrentTurnId: messageIntent.expectedCurrentTurnId,
          modelConfig: messageIntent.modelConfig,
        })
        if (
          client.generation !== selectionGeneration.current ||
          selectedAgentRef.current !== client.agentId
        )
          return false
        dispatch({
          type: "turn_accepted",
          agentId: client.agentId,
          turnId: accepted.turn_id,
        })
        if (pendingMessageRef.current === messageIntent)
          pendingMessageRef.current = null
        if (
          messageIntent.modelConfig !== undefined &&
          client.generation === selectionGeneration.current
        ) {
          setAcceptedModelConfig(messageIntent.modelConfig)
          setRequestedModelConfig((pendingConfig) =>
            sameModelConfig(
              pendingConfig ?? undefined,
              messageIntent.modelConfig
            )
              ? null
              : pendingConfig
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
          ? {
              ...config,
              parameters: withThinkingLevel(config.parameters, carry),
            }
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

function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error
  return new ApiError(
    "command_failed",
    0,
    error instanceof Error ? error.message : "command failed"
  )
}
