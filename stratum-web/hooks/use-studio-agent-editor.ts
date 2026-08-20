"use client"

import { useRouter } from "next/navigation"
import { useEffect, useReducer, useRef, useState } from "react"
import type { FormEvent } from "react"

import { studioApi } from "@/features/studio-management/client"
import {
  dispatchApiError,
  formReducer,
  initialFormState,
} from "@/features/studio-management/form-state"
import {
  agentDraftToInput,
  agentVersionValidationMessage,
  agentViewToDraft,
  encodeAgentToml,
  parseAgentToml,
} from "@/features/studio-management/transforms"
import type { AgentDraft } from "@/features/studio-management/types"
import { useDirtyGuard } from "@/features/studio-management/use-dirty-guard"
import { ApiError } from "@/lib/stratum/api"
import type {
  AgentDefinitionView,
  ResourceRevision,
  ToolView,
} from "@/lib/stratum/api"
import {
  invalidatePageCache,
  readPageCache,
  writePageCache,
} from "@/lib/page-cache"
import type { ModelDescriptor } from "@/lib/stratum/model-config"

const EMPTY_DRAFT: AgentDraft = {
  agentName: "",
  agentVersion: "v1",
  model: "",
  parameters: {},
  tools: [],
  prompt: "",
}

/** Owns Agent editor loading, persistence, cache, and route side effects. */
export function useStudioAgentEditor(agentName?: string) {
  const isNew = agentName === undefined
  const router = useRouter()
  const cacheKey = isNew ? null : `studio:agent-def:${agentName}`
  const cached = cacheKey
    ? readPageCache<ResourceRevision<AgentDefinitionView>>(cacheKey)
    : null
  const cachedModels = readPageCache<readonly ModelDescriptor[]>(
    "studio:catalog:models"
  )
  const initialDraft = cached
    ? agentViewToDraft(cached.data)
    : isNew && cachedModels?.[0]
      ? { ...EMPTY_DRAFT, model: cachedModels[0].model }
      : EMPTY_DRAFT
  const [state, dispatch] = useReducer(
    formReducer<AgentDraft>,
    initialDraft,
    (value) => initialFormState(value, cached?.etag ?? "")
  )
  const [models, setModels] = useState<readonly ModelDescriptor[]>(
    () => cachedModels ?? []
  )
  const [tools, setTools] = useState<readonly ToolView[]>(
    () => readPageCache("studio:catalog:tools") ?? []
  )
  const [toolsError, setToolsError] = useState(false)
  const [loading, setLoading] = useState(isNew || cached === null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [notFound, setNotFound] = useState(false)
  const [raw, setRaw] = useState(() => encodeAgentToml(initialDraft))
  const [rawError, setRawError] = useState<string | null>(null)
  const [rawDirty, setRawDirty] = useState(false)
  const [parametersValid, setParametersValid] = useState(true)
  const [deleting, setDeleting] = useState(false)
  const dirty = state.dirty || rawDirty
  const interactionRevisionRef = useRef(0)
  const { confirmNavigation, leave } = useDirtyGuard(dirty)

  // Route identity can change without remounting. Apply a cached resource once;
  // the authoritative background refresh still respects a dirty local draft.
  const [appliedCacheKey, setAppliedCacheKey] = useState(cacheKey)
  if (cached && appliedCacheKey !== cacheKey) {
    setAppliedCacheKey(cacheKey)
    const draft = agentViewToDraft(cached.data)
    dispatch({ type: "reload", value: draft, etag: cached.etag })
    setRaw(encodeAgentToml(draft))
    setRawDirty(false)
  }

  const loadTools = async () => {
    setToolsError(false)
    try {
      const catalog = await studioApi.listTools()
      writePageCache("studio:catalog:tools", catalog)
      setTools(catalog)
    } catch {
      setToolsError(true)
    }
  }

  const load = async (force = false) => {
    const interactionRevision = interactionRevisionRef.current
    const dirtyAtStart = dirty
    setLoadError(null)
    try {
      const [modelList, resource] = await Promise.all([
        studioApi.getModels(),
        isNew ? Promise.resolve(null) : studioApi.getAgentDefinition(agentName),
      ])
      writePageCache("studio:catalog:models", modelList)
      setModels(modelList)
      if (resource) {
        if (cacheKey) writePageCache(cacheKey, resource)
        const draft = agentViewToDraft(resource.data)
        const unchanged = interactionRevisionRef.current === interactionRevision
        const applyEditorState = unchanged && (force || !dirtyAtStart)
        dispatch({
          type: force && unchanged ? "reload" : "refresh",
          value: draft,
          etag: resource.etag,
        })
        if (applyEditorState) {
          setRaw(encodeAgentToml(draft))
          setRawError(null)
          setRawDirty(false)
          setParametersValid(true)
        }
      } else if (modelList[0]) {
        const draft = { ...EMPTY_DRAFT, model: modelList[0].model }
        dispatch({ type: "refresh", value: draft, etag: "" })
        if (
          interactionRevisionRef.current === interactionRevision &&
          !dirtyAtStart
        ) {
          setRaw(encodeAgentToml(draft))
          setRawError(null)
          setRawDirty(false)
          setParametersValid(true)
        }
      }
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 404) setNotFound(true)
      else
        setLoadError(
          caught instanceof Error ? caught.message : "无法加载 Agent definition"
        )
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0)
    return () => window.clearTimeout(timer)
    // agentName is the route identity; background refresh must not overwrite dirty state.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentName])

  useEffect(() => {
    const timer = window.setTimeout(() => void loadTools(), 0)
    return () => window.clearTimeout(timer)
  }, [])

  const edit = (draft: AgentDraft) => {
    interactionRevisionRef.current += 1
    if (rawDirty) {
      setRaw(encodeAgentToml(draft))
      setRawError(null)
      setRawDirty(false)
    }
    dispatch({ type: "edit", draft })
  }

  const selectModel = (model: string) => {
    setParametersValid(true)
    edit({ ...state.draft, model, parameters: {} })
  }

  const updateRaw = (source: string) => {
    interactionRevisionRef.current += 1
    setRaw(source)
    setRawDirty(true)
    const parsed = parseAgentToml(source)
    if (parsed.ok) {
      setRawError(null)
      setRawDirty(false)
      setParametersValid(true)
      dispatch({
        type: "edit",
        draft: { ...parsed.draft, agentName: state.draft.agentName },
      })
    } else {
      setRawError(`第 ${parsed.line} 行：${parsed.message}`)
    }
  }

  const save = async (event: FormEvent) => {
    event.preventDefault()
    const nextVersion = state.draft.agentVersion
    const versionError = agentVersionValidationMessage(nextVersion)
    if (
      versionError !== null ||
      (!isNew && nextVersion === state.acknowledged.agentVersion)
    ) {
      dispatch({
        type: "invalid",
        message: isNew
          ? (versionError ?? "请填写版本标签。")
          : "每次保存 Agent definition 都必须填写一个不同的新版本标签。",
        violations: [
          {
            field: "agent_version",
            code: "invalid_agent_version",
            message:
              versionError ??
              (isNew ? "版本标签不能为空" : "版本标签必须与当前版本不同"),
          },
        ],
      })
      return
    }
    dispatch({ type: "save" })
    try {
      const input = agentDraftToInput(state.draft)
      const response = isNew
        ? await studioApi.createAgentDefinition(input)
        : await studioApi.updateAgentDefinition(agentName, input, state.etag)
      const value = agentViewToDraft(response.data)
      invalidatePageCache("studio-agents:")
      writePageCache(`studio:agent-def:${response.data.agent_name}`, {
        data: response.data,
        etag: response.etag,
      })
      const acknowledge = () => {
        dispatch({ type: "acknowledge", value, etag: response.etag })
        setRaw(encodeAgentToml(value))
        setRawDirty(false)
      }
      if (isNew) {
        leave(() => {
          acknowledge()
          router.replace(
            `/studio/agents/${encodeURIComponent(response.data.agent_name)}`
          )
        }, false)
      } else {
        acknowledge()
      }
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "资源已在别处变更。本地内容仍保留，请重新加载后再决定。",
        fallback: "保存失败",
      })
    }
  }

  const remove = async () => {
    if (isNew || !agentName) return
    setDeleting(true)
    try {
      await studioApi.deleteAgentDefinition(agentName, state.etag)
      invalidatePageCache()
      leave(() => router.replace("/studio"), false)
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "资源已变更，请重新加载后再删除。",
        fallback: "删除失败",
      })
      setDeleting(false)
    }
  }

  const reload = () => {
    if (confirmNavigation()) {
      setLoading(true)
      void load(true)
    }
  }

  const retry = () => {
    setLoading(!hasLoadedContent)
    void load()
  }

  const cancel = () => {
    leave(() => router.push("/studio"))
  }

  const localVersionError = agentVersionValidationMessage(
    state.draft.agentVersion
  )
  const versionError =
    localVersionError ??
    (!isNew &&
    dirty &&
    state.draft.agentVersion === state.acknowledged.agentVersion
      ? "每次保存都必须填写一个不同的新版本标签"
      : null)
  const versionIsReady =
    versionError === null &&
    (isNew || state.draft.agentVersion !== state.acknowledged.agentVersion)
  const hasLoadedContent = isNew ? models.length > 0 : state.etag !== ""

  return {
    cancel,
    deleting,
    dirty,
    dispatch,
    edit,
    isNew,
    loadError,
    loading,
    loadTools,
    models,
    notFound,
    parametersValid,
    raw,
    rawError,
    reload,
    remove,
    retry,
    save,
    selectModel,
    setParametersValid,
    state,
    hasLoadedContent,
    syncRaw: () => {
      if (!rawDirty) setRaw(encodeAgentToml(state.draft))
    },
    tools,
    toolsError,
    updateRaw,
    versionError,
    versionIsReady,
  }
}
