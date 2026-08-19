"use client"

import { useRouter, useSearchParams } from "next/navigation"
import { useEffect, useReducer, useState } from "react"
import type { FormEvent } from "react"

import { studioApi } from "@/features/studio-management/client"
import {
  dispatchApiError,
  formReducer,
  initialFormState,
} from "@/features/studio-management/form-state"
import {
  safeStudioReturn,
  withStudioReturn,
} from "@/features/studio-management/navigation"
import {
  modelViewToDraft,
  splitManagedModelId,
} from "@/features/studio-management/transforms"
import type { ModelDraft } from "@/features/studio-management/types"
import { useDirtyGuard } from "@/features/studio-management/use-dirty-guard"
import {
  invalidatePageCache,
  readPageCache,
  writePageCache,
} from "@/lib/page-cache"
import { ApiError } from "@/lib/stratum/api"
import type {
  ManagedModelView,
  ProviderView,
  ResourceRevision,
} from "@/lib/stratum/api"

const EMPTY_DRAFT: ModelDraft = { provider: "openai", modelName: "" }

/** Owns Model editor loading, persistence, cache, and route side effects. */
export function useStudioModelEditor(modelId?: string) {
  const isNew = modelId === undefined
  const router = useRouter()
  const searchParams = useSearchParams()
  const returnTo = safeStudioReturn(searchParams.get("returnTo"))
  const modelsHref = withStudioReturn("/studio/settings/models", returnTo)
  const parsedId = modelId ? splitManagedModelId(modelId) : null
  const cacheKey = modelId ? `studio:model:${modelId}` : null
  const cached = cacheKey
    ? readPageCache<ResourceRevision<ManagedModelView>>(cacheKey)
    : null
  const cachedProviders = readPageCache<readonly ProviderView[]>(
    "studio:catalog:providers"
  )
  const initialDraft = cached
    ? modelViewToDraft(cached.data)
    : isNew && cachedProviders?.[0]
      ? { ...EMPTY_DRAFT, provider: cachedProviders[0].provider }
      : EMPTY_DRAFT
  const [state, dispatch] = useReducer(
    formReducer<ModelDraft>,
    initialDraft,
    (value) => initialFormState(value, cached?.etag ?? "")
  )
  const [resource, setResource] = useState<ManagedModelView | null>(
    () => cached?.data ?? null
  )
  const [providers, setProviders] = useState<readonly ProviderView[]>(
    () => cachedProviders ?? []
  )
  const [loading, setLoading] = useState(isNew || cached === null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [notFound, setNotFound] = useState(
    modelId !== undefined && parsedId === null
  )
  const [deleting, setDeleting] = useState(false)
  const { leave } = useDirtyGuard(state.dirty)

  const [appliedCacheKey, setAppliedCacheKey] = useState(cacheKey)
  if (cached && appliedCacheKey !== cacheKey) {
    setAppliedCacheKey(cacheKey)
    dispatch({
      type: "reload",
      value: modelViewToDraft(cached.data),
      etag: cached.etag,
    })
    setResource(cached.data)
  }

  const load = async (force = false) => {
    setLoadError(null)
    try {
      const [providerList, response] = await Promise.all([
        studioApi.listProviders({ page: 1, perPage: 50 }),
        isNew || !parsedId
          ? Promise.resolve(null)
          : studioApi.getManagedModel(parsedId.provider, parsedId.modelName),
      ])
      writePageCache("studio:catalog:providers", providerList.data)
      setProviders(providerList.data)
      if (isNew) {
        if (providerList.data[0])
          dispatch({
            type: "refresh",
            value: { ...EMPTY_DRAFT, provider: providerList.data[0].provider },
            etag: "",
          })
        return
      }
      if (!response) return
      if (cacheKey) writePageCache(cacheKey, response)
      setResource(response.data)
      dispatch({
        type: force ? "reload" : "refresh",
        value: modelViewToDraft(response.data),
        etag: response.etag,
      })
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 404) setNotFound(true)
      else
        setLoadError(
          caught instanceof Error ? caught.message : "无法加载 Model"
        )
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0)
    return () => window.clearTimeout(timer)
    // modelId is the route identity; refresh keeps dirty drafts intact.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modelId])

  const edit = (draft: ModelDraft) => dispatch({ type: "edit", draft })

  const save = async (event: FormEvent) => {
    event.preventDefault()
    if (!isNew) return
    dispatch({ type: "save" })
    try {
      const response = await studioApi.createManagedModel({
        provider: state.draft.provider,
        name: state.draft.modelName.trim(),
      })
      setResource(response.data)
      invalidatePageCache("studio-settings:")
      invalidatePageCache("studio:catalog:models")
      writePageCache(`studio:model:${response.data.model_id}`, {
        data: response.data,
        etag: response.etag,
      })
      leave(() => {
        dispatch({
          type: "acknowledge",
          value: modelViewToDraft(response.data),
          etag: response.etag,
          message:
            "已创建。Model 会从下一次 LLM work / Turn 起可用；当前进行中的 Turn 不变。",
        })
        router.replace(
          withStudioReturn(
            `/studio/settings/models/${encodeURIComponent(response.data.model_id)}`,
            returnTo
          )
        )
      }, false)
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "Model 已在别处变更，本地输入仍保留。",
        fallback: "保存失败",
      })
    }
  }

  const remove = async () => {
    if (isNew || !resource) return
    setDeleting(true)
    try {
      await studioApi.deleteManagedModel(
        resource.provider,
        resource.name,
        state.etag
      )
      invalidatePageCache()
      leave(() => router.replace(modelsHref), false)
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "Model 已变更，请重新加载后再删除。",
        fallback: "删除失败",
      })
      setDeleting(false)
    }
  }

  return {
    cancel: () => {
      leave(() => router.push(modelsHref))
    },
    deleting,
    edit,
    hasLoadedContent: isNew ? providers.length > 0 : resource !== null,
    isNew,
    loadError,
    loading,
    modelsHref,
    newModelHref: withStudioReturn("/studio/settings/models/new", returnTo),
    notFound,
    providers,
    reload: () => {
      setLoading(true)
      void load(true)
    },
    remove,
    resource,
    retry: () => {
      setLoading(isNew ? providers.length === 0 : resource === null)
      void load()
    },
    save,
    state,
  }
}
