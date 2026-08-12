"use client"

import { useRouter, useSearchParams } from "next/navigation"
import { useEffect, useReducer, useState } from "react"

import {
  BlockerList,
  Field,
  FormStatus,
  InlineDelete,
  SaveButton,
  SettingsNav,
  StudioHeader,
  StudioInput,
  StudioPage,
  StudioTextarea,
  controlClass,
} from "@/components/stratum/studio/primitives"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { studioApi } from "@/features/studio-management/client"
import {
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
import { ApiError } from "@/lib/stratum/api"
import type {
  ManagedModelView,
  ProviderKind,
  ProviderView,
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
  const [resource, setResource] = useState<ManagedModelView | null>(null)
  const [providers, setProviders] = useState<readonly ProviderView[]>([])
  const [loading, setLoading] = useState(!isNew)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [notFound, setNotFound] = useState(
    modelId !== undefined && parsedId === null
  )
  const [deleting, setDeleting] = useState(false)
  const confirmNavigation = useDirtyGuard(isDirtyPhase(state.phase))

  const load = async () => {
    setLoadError(null)
    try {
      const providerList = await studioApi.listProviders({
        page: 1,
        perPage: 50,
      })
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
      if (!parsedId) return
      const response = await studioApi.getManagedModel(
        parsedId.provider,
        parsedId.modelName
      )
      setResource(response.data)
      dispatch({
        type: "reload",
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
      router.replace(
        withStudioReturn(
          `/studio/settings/models/${encodeURIComponent(response.data.model_id)}`,
          returnTo
        )
      )
    } catch (caught) {
      if (
        caught instanceof ApiError &&
        (caught.status === 400 || caught.status === 422)
      )
        dispatch({
          type: "invalid",
          message: caught.message,
          violations: caught.details.violations,
        })
      else
        dispatch({
          type: "invalid",
          message: caught instanceof Error ? caught.message : "保存失败",
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
      router.replace(modelsHref)
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 409)
        dispatch({
          type: "blocked",
          message: caught.message,
          blockers: caught.details.blockers ?? [],
        })
      else if (caught instanceof ApiError && caught.status === 412)
        dispatch({
          type: "conflict",
          message: "Model 已变更，请重新加载后再删除。",
        })
      else
        dispatch({
          type: "invalid",
          message: caught instanceof Error ? caught.message : "删除失败",
        })
      setDeleting(false)
    }
  }

  if (loading)
    return (
      <StudioPage>
        <Skeleton className="h-8 w-48" />
        <Skeleton className="mt-10 h-96 rounded-2xl" />
      </StudioPage>
    )
  if (notFound)
    return (
      <StudioPage>
        <StudioHeader title="Model 不存在" backHref={modelsHref} />
      </StudioPage>
    )
  if (loadError)
    return (
      <StudioPage>
        <StudioHeader title="无法打开 Model" backHref={modelsHref} />
        <FormStatus message={loadError} tone="error" />
      </StudioPage>
    )

  return (
    <StudioPage>
      <StudioHeader
        title={isNew ? "新建 Model" : (resource?.name ?? state.draft.modelName)}
        backHref={modelsHref}
        backLabel="返回 Model"
      />
      <SettingsNav current="models" returnTo={returnTo} />
      <form onSubmit={save} className="grid gap-7">
        <section className="grid gap-6 rounded-2xl border border-border bg-card p-5 sm:p-7">
          <Field label="Provider" error={state.violations.provider}>
            <select
              className={`${controlClass} h-9 rounded-md border px-3 text-sm outline-none focus-visible:ring-2`}
              disabled={!isNew}
              value={state.draft.provider}
              onChange={(event) =>
                dispatch({
                  type: "edit",
                  draft: {
                    ...state.draft,
                    provider: event.target.value as ProviderKind,
                  },
                })
              }
            >
              {providers.map((provider) => (
                <option key={provider.provider} value={provider.provider}>
                  {provider.provider}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Model name" error={state.violations.name}>
            <StudioInput
              disabled={!isNew}
              autoFocus={isNew}
              value={state.draft.modelName}
              onChange={(event) =>
                dispatch({
                  type: "edit",
                  draft: { ...state.draft, modelName: event.target.value },
                })
              }
            />
          </Field>
          {resource ? (
            <Field
              label="Parameter schema"
              hint="由 Provider adapter 声明，只读。"
            >
              <StudioTextarea
                readOnly
                rows={18}
                spellCheck={false}
                className="font-mono text-sm"
                value={encodeModelSchema(resource)}
              />
            </Field>
          ) : null}
        </section>
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
        <div className="flex justify-end gap-3">
          <Button
            type="button"
            variant="ghost"
            className="min-h-11 rounded-xl"
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
    </StudioPage>
  )
}
