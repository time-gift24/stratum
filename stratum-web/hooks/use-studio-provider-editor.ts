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
import { providerViewToDraft } from "@/features/studio-management/transforms"
import type { ProviderDraft } from "@/features/studio-management/types"
import { useDirtyGuard } from "@/features/studio-management/use-dirty-guard"
import {
  invalidatePageCache,
  readPageCache,
  writePageCache,
} from "@/lib/page-cache"
import { ApiError } from "@/lib/stratum/api"
import type {
  ProviderKind,
  ProviderView,
  ResourceRevision,
} from "@/lib/stratum/api"

const EMPTY_DRAFT: ProviderDraft = { provider: "openai", apiKey: "" }

/** Owns Provider editor loading, persistence, testing, cache, and navigation. */
export function useStudioProviderEditor(provider?: string) {
  const isNew = provider === undefined
  const router = useRouter()
  const searchParams = useSearchParams()
  const returnTo = safeStudioReturn(searchParams.get("returnTo"))
  const providersHref = withStudioReturn("/studio/settings/providers", returnTo)
  const cacheKey = isNew ? null : `studio:provider:${provider}`
  const cached = cacheKey
    ? readPageCache<ResourceRevision<ProviderView>>(cacheKey)
    : null
  const initialDraft = cached ? providerViewToDraft(cached.data) : EMPTY_DRAFT
  const [state, dispatch] = useReducer(
    formReducer<ProviderDraft>,
    initialDraft,
    (value) => initialFormState(value, cached?.etag ?? "")
  )
  const [resource, setResource] = useState<ProviderView | null>(
    () => cached?.data ?? null
  )
  const [loading, setLoading] = useState(!isNew && cached === null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [notFound, setNotFound] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [testResult, setTestResult] = useState<{
    tone: "success" | "error"
    message: string
  } | null>(null)
  const dirty = state.dirty
  const credentialDirty = state.draft.apiKey.trim() !== ""
  const { confirmNavigation, leave } = useDirtyGuard(dirty)

  const [appliedCacheKey, setAppliedCacheKey] = useState(cacheKey)
  if (cached && appliedCacheKey !== cacheKey) {
    setAppliedCacheKey(cacheKey)
    dispatch({
      type: "reload",
      value: providerViewToDraft(cached.data),
      etag: cached.etag,
    })
    setResource(cached.data)
  }

  const load = async (force = false) => {
    if (isNew) return
    setLoadError(null)
    try {
      const response = await studioApi.getProvider(provider as ProviderKind)
      if (cacheKey) writePageCache(cacheKey, response)
      setResource(response.data)
      dispatch({
        type: force ? "reload" : "refresh",
        value: providerViewToDraft(response.data),
        etag: response.etag,
      })
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 404) setNotFound(true)
      else
        setLoadError(
          caught instanceof Error ? caught.message : "无法加载 Provider"
        )
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0)
    return () => window.clearTimeout(timer)
    // provider is the route identity; refresh keeps dirty drafts intact.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider])

  const edit = (draft: ProviderDraft) => dispatch({ type: "edit", draft })

  const save = async (event: FormEvent) => {
    event.preventDefault()
    dispatch({ type: "save" })
    try {
      const input = {
        provider: state.draft.provider,
        ...(state.draft.apiKey.trim() === ""
          ? {}
          : { api_key: state.draft.apiKey }),
      }
      const response = isNew
        ? await studioApi.createProvider(input)
        : await studioApi.updateProvider(
            state.draft.provider,
            input,
            state.etag
          )
      setResource(response.data)
      invalidatePageCache("studio-settings:")
      invalidatePageCache("studio:catalog:providers")
      writePageCache(`studio:provider:${response.data.provider}`, {
        data: response.data,
        etag: response.etag,
      })
      const acknowledge = () =>
        dispatch({
          type: "acknowledge",
          value: providerViewToDraft(response.data),
          etag: response.etag,
          message:
            "已保存。Provider 变更会从下一次 LLM work / Turn 起生效；当前进行中的 Turn 不变。",
        })
      if (isNew) {
        leave(() => {
          acknowledge()
          router.replace(
            withStudioReturn(
              `/studio/settings/providers/${response.data.provider}`,
              returnTo
            )
          )
        }, false)
      } else {
        acknowledge()
      }
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "Provider 已在别处变更，本地输入仍保留。",
        fallback: "保存失败",
      })
    }
  }

  const test = async () => {
    if (isNew || credentialDirty) return
    const restorePhase = dirty ? "dirty" : "loaded"
    setTestResult(null)
    dispatch({ type: "test" })
    try {
      const result = await studioApi.testProvider(state.draft.provider)
      setTestResult({
        tone: result.success ? "success" : "error",
        message: result.success
          ? `本次连接成功 · ${result.completed_at}`
          : (result.message ?? "连接失败"),
      })
      dispatch({ type: "settle", restorePhase })
    } catch (caught) {
      setTestResult({
        tone: "error",
        message: caught instanceof Error ? caught.message : "连接测试失败",
      })
      dispatch({ type: "settle", restorePhase })
    }
  }

  const remove = async () => {
    if (isNew) return
    setDeleting(true)
    try {
      await studioApi.deleteProvider(state.draft.provider, state.etag)
      invalidatePageCache()
      leave(() => router.replace(providersHref), false)
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "Provider 已变更，请重新加载后再删除。",
        fallback: "删除失败",
      })
      setDeleting(false)
    }
  }

  return {
    cancel: () => {
      leave(() => router.push(providersHref))
    },
    credentialDirty,
    deleting,
    edit,
    hasLoadedContent: isNew || resource !== null,
    isNew,
    loadError,
    loading,
    newProviderHref: withStudioReturn(
      "/studio/settings/providers/new",
      returnTo
    ),
    notFound,
    providersHref,
    reload: () => {
      if (confirmNavigation()) {
        setLoading(true)
        void load(true)
      }
    },
    remove,
    resource,
    retry: () => {
      setLoading(resource === null)
      void load()
    },
    returnTo,
    save,
    state,
    test,
    testResult,
  }
}
