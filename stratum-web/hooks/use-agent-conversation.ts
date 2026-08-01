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
import { recoverConversation } from "@/features/agent-conversation/recovery"
import type { ConversationState } from "@/features/agent-conversation/types"
import {
  clearCursor,
  loadCursor,
  loadRecentAgents,
  rememberRecentAgent as rememberStoredRecentAgent,
  removeRecentAgent as removeStoredRecentAgent,
  saveCursor,
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
  ): Promise<void>
  reconnect(): void
  removeRecentAgent(agentId: string): void
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
    const storage = browserStorage()
    const api = createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
    const dispatchIfCurrent = (action: Parameters<typeof dispatch>[0]) => {
      if (
        !controller.signal.aborted &&
        generation === selectionGeneration.current
      )
        dispatch(action)
    }

    void recoverConversation(
      {
        api,
        subscribe: (options) =>
          subscribeToAgentEvents({
            ...options,
            baseUrl: STRATUM_API_BASE_URL,
          }),
        loadCursor: (agentId) =>
          storage ? loadCursor(storage, agentId) : undefined,
        saveCursor: (agentId, cursor) => {
          if (
            storage &&
            !controller.signal.aborted &&
            generation === selectionGeneration.current
          )
            saveCursor(storage, agentId, cursor)
        },
        clearCursor: (agentId) => {
          if (storage && generation === selectionGeneration.current)
            clearCursor(storage, agentId)
        },
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
    selectionGeneration.current += 1
    setReconnectVersion((version) => version + 1)
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
      try {
        const created = await createStratumApi({
          baseUrl: STRATUM_API_BASE_URL,
        }).createAgent({
          agentName: effectiveTemplate.agent_name,
          text: prompt,
          modelConfig: requestedModelConfig ?? undefined,
        })
        if (generation !== selectionGeneration.current) return false

        const recentAgent: RecentAgent = {
          agentId: created.agent_id,
          agentName: created.agent_name,
          title: prompt,
          lastOpenedAt: new Date().toISOString(),
        }
        rememberRecentAgent(recentAgent)
        selectAgent(created.agent_id)
        return true
      } catch (error) {
        if (generation === selectionGeneration.current) reportError(error)
        return false
      }
    },
    [
      effectiveTemplate,
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

      if (state.phase === "recovering" || state.view?.status === "running")
        return false

      const client = selectedClient()
      if (!client) return false

      const selectedConfig = requestedModelConfig
      try {
        await client.api.sendMessage(
          client.agentId,
          message,
          selectedConfig ?? undefined
        )
        if (
          selectedConfig !== null &&
          client.generation === selectionGeneration.current
        ) {
          setAcceptedModelConfig(selectedConfig)
          setRequestedModelConfig((pendingConfig) =>
            pendingConfig === selectedConfig ? null : pendingConfig
          )
        }
        return true
      } catch (error) {
        if (
          client.generation === selectionGeneration.current &&
          !isApiErrorCode(error, "agent_busy")
        )
          reportError(error)
        return false
      }
    },
    [
      reportError,
      requestedModelConfig,
      selectedClient,
      state.phase,
      state.view?.status,
    ]
  )

  const resume = useCallback(async () => {
    const client = selectedClient()
    if (!client) return

    try {
      await client.api.resume(client.agentId)
    } catch (error) {
      if (client.generation !== selectionGeneration.current) return
      if (isApiErrorCode(error, "resume_not_running")) {
        reconnect()
        return
      }
      reportError(error)
    }
  }, [reconnect, reportError, selectedClient])

  const cancel = useCallback(async () => {
    const client = selectedClient()
    if (!client) return

    try {
      await client.api.cancel(client.agentId)
    } catch (error) {
      if (client.generation !== selectionGeneration.current) return
      if (isApiErrorCode(error, "resume_required")) {
        reconnect()
        return
      }
      reportError(error)
    }
  }, [reconnect, reportError, selectedClient])

  const resolveApproval = useCallback(
    async (approvalId: string, decision: "approve" | "reject") => {
      const client = selectedClient()
      if (!client) return

      try {
        await client.api.resolveApproval(client.agentId, approvalId, decision)
      } catch (error) {
        if (client.generation === selectionGeneration.current)
          reportError(error)
      }
    },
    [reportError, selectedClient]
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
