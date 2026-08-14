"use client"

import { useRouter, useSearchParams } from "next/navigation"
import { useEffect, useReducer, useState } from "react"

import {
  BlockerList,
  ErrorState,
  Field,
  FormSection,
  FormStatus,
  InlineDelete,
  LoadingState,
  NotFoundState,
  PageHeader,
  PageShell,
  SaveButton,
  SettingsShell,
  StudioInput,
  StudioSelect,
  StudioTextarea,
} from "@/components/stratum/studio/primitives"
import { Button } from "@/components/ui/button"
import { studioApi } from "@/features/studio-management/client"
import {
  dispatchApiError,
  formReducer,
  initialFormState,
  isDirtyPhase,
} from "@/features/studio-management/form-state"
import {
  safeStudioReturn,
  withStudioReturn,
} from "@/features/studio-management/navigation"
import {
  encodeModelSchema,
  modelViewToDraft,
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
  ProviderKind,
  ProviderView,
  ResourceRevision,
} from "@/lib/stratum/api"

const EMPTY: ModelDraft = { provider: "openai", modelName: "" }

function splitModelId(
  modelId: string
): { provider: ProviderKind; modelName: string } | null {
  const separator = modelId.indexOf(":")
  if (separator <= 0) return null
  const provider = modelId.slice(0, separator)
  if (provider !== "openai" && provider !== "deepseek") return null
  return { provider, modelName: modelId.slice(separator + 1) }
}

export function ModelEditor({ modelId }: { modelId?: string }) {
  const isNew = modelId === undefined
  const router = useRouter()
  const searchParams = useSearchParams()
  const returnTo = safeStudioReturn(searchParams.get("returnTo"))
  const modelsHref = withStudioReturn("/studio/settings/models", returnTo)
  const parsedId = modelId ? splitModelId(modelId) : null
  const [state, dispatch] = useReducer(
    formReducer<ModelDraft>,
    EMPTY,
    initialFormState
  )
  const [resource, setResource] = useState<ManagedModelView | null>(() => {
    if (!modelId) return null
    return (
      readPageCache<ResourceRevision<ManagedModelView>>(`studio:model:${modelId}`)
        ?.data ?? null
    )
  })
  const [providers, setProviders] = useState<readonly ProviderView[]>(
    () => readPageCache("studio:catalog:providers") ?? []
  )
  const [loading, setLoading] = useState(
    !isNew &&
      readPageCache<ResourceRevision<ManagedModelView>>(
        `studio:model:${modelId}`
      ) === null
  )
  const [loadError, setLoadError] = useState<string | null>(null)
  const [notFound, setNotFound] = useState(
    modelId !== undefined && parsedId === null
  )
  const [deleting, setDeleting] = useState(false)
  const confirmNavigation = useDirtyGuard(isDirtyPhase(state.phase))

  // 重访先用缓存填充，权威版本由 load 刷新
  const modelCacheKey = modelId ? `studio:model:${modelId}` : null
  const [appliedCacheKey, setAppliedCacheKey] = useState<string | null>(null)
  const cachedModel = modelCacheKey
    ? readPageCache<ResourceRevision<ManagedModelView>>(modelCacheKey)
    : null
  if (cachedModel && appliedCacheKey !== modelCacheKey) {
    setAppliedCacheKey(modelCacheKey)
    dispatch({
      type: "reload",
      value: modelViewToDraft(cachedModel.data),
      etag: cachedModel.etag,
    })
  }

  const load = async () => {
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
            type: "reload",
            value: { ...EMPTY, provider: providerList.data[0].provider },
            etag: "",
          })
        return
      }
      if (!response) return
      if (modelCacheKey) writePageCache(modelCacheKey, response)
      setResource(response.data)
      dispatch({
        type: "reload",
        value: modelViewToDraft(response.data),
        etag: response.etag,
      })
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 404) setNotFound(true)
      else if (cachedModel === null)
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
    // modelId is the resource identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modelId])

  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!isNew) return
    dispatch({ type: "save" })
    try {
      const response = await studioApi.createManagedModel({
        provider: state.draft.provider,
        name: state.draft.modelName.trim(),
      })
      setResource(response.data)
      dispatch({
        type: "acknowledge",
        value: modelViewToDraft(response.data),
        etag: response.etag,
      })
      invalidatePageCache("studio-settings:")
      writePageCache(`studio:model:${response.data.model_id}`, {
        data: response.data,
        etag: response.etag,
      })
      router.replace(
        withStudioReturn(
          `/studio/settings/models/${encodeURIComponent(response.data.model_id)}`,
          returnTo
        )
      )
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
      router.replace(modelsHref)
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "Model 已变更，请重新加载后再删除。",
        fallback: "删除失败",
      })
      setDeleting(false)
    }
  }

  if (loading)
    return (
      <PageShell>
        <LoadingState label="正在加载 Model" />
      </PageShell>
    )
  if (notFound)
    return (
      <PageShell>
        <PageHeader title="Model 不存在" backHref={modelsHref} />
        <NotFoundState
          message="该 Model 不存在或已被删除。可以返回列表，或直接新建一个。"
          createHref={withStudioReturn("/studio/settings/models/new", returnTo)}
          createLabel="新建 Model"
        />
      </PageShell>
    )
  if (loadError)
    return (
      <PageShell>
        <PageHeader title="无法打开 Model" backHref={modelsHref} />
        <ErrorState
          title="Model 加载失败"
          message={loadError}
          onRetry={() => {
            setLoading(true)
            void load()
          }}
        />
      </PageShell>
    )

  return (
    <PageShell>
      <PageHeader
        title={isNew ? "新建 Model" : (resource?.name ?? state.draft.modelName)}
        backHref={modelsHref}
        backLabel="返回 Model"
      />
      <SettingsShell current="models" returnTo={returnTo}>
        <form onSubmit={save} className="grid gap-8">
          <FormSection
            title="模型"
            description="Model 挂在 Provider 下，名称与 Provider 创建后不可修改。"
          >
            <div className="grid gap-6">
              <Field label="Provider" error={state.violations.provider}>
                <StudioSelect
                  ariaLabel="Provider"
                  disabled={!isNew}
                  value={state.draft.provider}
                  options={providers.map((provider) => ({
                    value: provider.provider,
                    label: provider.provider,
                  }))}
                  onChange={(next) =>
                    dispatch({
                      type: "edit",
                      draft: {
                        ...state.draft,
                        provider: next as ProviderKind,
                      },
                    })
                  }
                />
              </Field>
              <Field label="Model name" error={state.violations.name}>
                <StudioInput
                  disabled={!isNew}
                  autoFocus={isNew}
                  className="font-mono"
                  value={state.draft.modelName}
                  onChange={(event) =>
                    dispatch({
                      type: "edit",
                      draft: { ...state.draft, modelName: event.target.value },
                    })
                  }
                />
              </Field>
            </div>
          </FormSection>
          {resource ? (
            <FormSection
              title="Parameter schema"
              description="由 Provider adapter 声明，只读。"
            >
              <StudioTextarea
                readOnly
                rows={18}
                spellCheck={false}
                className="font-mono text-sm leading-6"
                value={encodeModelSchema(resource)}
              />
            </FormSection>
          ) : null}
          <FormStatus
            message={state.message}
            tone={
              state.phase === "invalid" || state.phase === "conflict"
                ? "error"
                : state.message
                  ? "success"
                  : "neutral"
            }
          />
          <BlockerList blockers={state.blockers} />
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              size="lg"
              onClick={() => {
                if (confirmNavigation()) router.push(modelsHref)
              }}
            >
              返回列表
            </Button>
            {isNew ? <SaveButton saving={state.phase === "saving"} /> : null}
          </div>
        </form>
        {!isNew ? (
          <div className="mt-12">
            <InlineDelete
              resourceLabel="Model"
              explanation="若此 Model 是默认 Model 或被 Agent definition 引用，系统会列出 blocker 并保持资源不变。"
              pending={deleting}
              onDelete={() => void remove()}
            />
          </div>
        ) : null}
      </SettingsShell>
    </PageShell>
  )
}
