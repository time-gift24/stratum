"use client"

import { useRouter, useSearchParams } from "next/navigation"
import { useEffect, useReducer, useState } from "react"
import { LoaderCircle, PlugZap } from "lucide-react"

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
  encodeProviderRaw,
  providerViewToDraft,
} from "@/features/studio-management/transforms"
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

const EMPTY: ProviderDraft = { provider: "openai", apiKey: "" }

export function ProviderEditor({ provider }: { provider?: string }) {
  const isNew = provider === undefined
  const router = useRouter()
  const searchParams = useSearchParams()
  const returnTo = safeStudioReturn(searchParams.get("returnTo"))
  const providersHref = withStudioReturn("/studio/settings/providers", returnTo)
  const [state, dispatch] = useReducer(
    formReducer<ProviderDraft>,
    EMPTY,
    initialFormState
  )
  const [view, setView] = useState<"structured" | "raw">("structured")
  const providerCacheKey = isNew ? null : `studio:provider:${provider}`
  const cachedProvider = providerCacheKey
    ? readPageCache<ResourceRevision<ProviderView>>(providerCacheKey)
    : null
  const [resource, setResource] = useState<ProviderView | null>(
    () => cachedProvider?.data ?? null
  )
  const [loading, setLoading] = useState(!isNew && cachedProvider === null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [notFound, setNotFound] = useState(false)
  const [deleting, setDeleting] = useState(false)
  const [testResult, setTestResult] = useState<{
    tone: "success" | "error"
    message: string
  } | null>(null)
  const dirty = isDirtyPhase(state.phase)
  const credentialDirty = state.draft.apiKey.trim() !== ""
  const confirmNavigation = useDirtyGuard(dirty)

  // 重访先用缓存填充表单，权威版本由 load 刷新
  const [appliedCacheKey, setAppliedCacheKey] = useState<string | null>(null)
  if (cachedProvider && appliedCacheKey !== providerCacheKey) {
    setAppliedCacheKey(providerCacheKey)
    dispatch({
      type: "reload",
      value: providerViewToDraft(cachedProvider.data),
      etag: cachedProvider.etag,
    })
  }

  const load = async () => {
    if (isNew) return
    setLoadError(null)
    try {
      const response = await studioApi.getProvider(provider as ProviderKind)
      if (providerCacheKey) writePageCache(providerCacheKey, response)
      const draft = providerViewToDraft(response.data)
      setResource(response.data)
      dispatch({ type: "reload", value: draft, etag: response.etag })
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 404) setNotFound(true)
      else if (cachedProvider === null)
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
    // provider is the resource identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [provider])

  const edit = (draft: ProviderDraft) => dispatch({ type: "edit", draft })

  const save = async (event: React.FormEvent) => {
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
      dispatch({
        type: "acknowledge",
        value: providerViewToDraft(response.data),
        etag: response.etag,
      })
      invalidatePageCache("studio-settings:")
      writePageCache(`studio:provider:${response.data.provider}`, {
        data: response.data,
        etag: response.etag,
      })
      if (isNew)
        router.replace(
          withStudioReturn(
            `/studio/settings/providers/${response.data.provider}`,
            returnTo
          )
        )
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
      router.replace(providersHref)
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "Provider 已变更，请重新加载后再删除。",
        fallback: "删除失败",
      })
      setDeleting(false)
    }
  }

  if (loading)
    return (
      <PageShell>
        <LoadingState label="正在加载 Provider" />
      </PageShell>
    )
  if (notFound)
    return (
      <PageShell>
        <PageHeader title="Provider 不存在" backHref={providersHref} />
        <NotFoundState
          message="该 Provider 不存在或已被删除。可以返回列表，或直接新建一个。"
          createHref={withStudioReturn("/studio/settings/providers/new", returnTo)}
          createLabel="新建 Provider"
        />
      </PageShell>
    )
  if (loadError)
    return (
      <PageShell>
        <PageHeader title="无法打开 Provider" backHref={providersHref} />
        <ErrorState
          title="Provider 加载失败"
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
        title={isNew ? "新建 Provider" : state.draft.provider}
        backHref={providersHref}
        backLabel="返回 Provider"
      />
      <SettingsShell current="providers" returnTo={returnTo}>
        <form onSubmit={save} className="grid gap-8">
          <div
            className="flex w-fit rounded-lg bg-muted p-1"
            role="tablist"
            aria-label="Provider 编辑视图"
          >
            {(["structured", "raw"] as const).map((item) => (
              <button
                key={item}
                type="button"
                role="tab"
                aria-selected={view === item}
                onClick={() => setView(item)}
                className="h-8 rounded-md px-3 text-sm font-medium focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-selected:bg-card aria-selected:shadow-sm"
              >
                {item === "structured" ? "结构化" : "Raw config"}
              </button>
            ))}
          </div>
          {view === "structured" ? (
            <FormSection
              title="连接"
              description="凭据只写不读：已存 secret 永不回显，留空表示保留。"
            >
              <div className="grid gap-6">
                <Field label="Provider kind" error={state.violations.provider}>
                  <StudioSelect
                    ariaLabel="Provider kind"
                    disabled={!isNew}
                    value={state.draft.provider}
                    options={[
                      { value: "openai", label: "OpenAI" },
                      { value: "deepseek", label: "DeepSeek" },
                    ]}
                    onChange={(next) =>
                      edit({
                        ...state.draft,
                        provider: next as ProviderKind,
                      })
                    }
                  />
                </Field>
                {!isNew && resource ? (
                  <p className="text-sm text-muted-foreground">
                    {resource.credential_configured
                      ? "凭据已配置。留空会保留现有凭据。"
                      : "尚未配置凭据。"}
                  </p>
                ) : null}
                <Field
                  label={isNew ? "API key" : "替换 API key"}
                  error={state.violations.api_key}
                  hint="已存 secret 永不回显；这里留空不会清除已有 secret。"
                >
                  <StudioInput
                    type="password"
                    autoComplete="new-password"
                    value={state.draft.apiKey}
                    onChange={(event) =>
                      edit({ ...state.draft, apiKey: event.target.value })
                    }
                  />
                </Field>
                {!isNew ? (
                  <div className="flex flex-wrap items-center gap-3">
                    <Button
                      type="button"
                      variant="outline"
                      size="lg"
                      disabled={
                        credentialDirty ||
                        state.phase === "testing" ||
                        state.phase === "saving"
                      }
                      onClick={() => void test()}
                    >
                      {state.phase === "testing" ? (
                        <>
                          <LoaderCircle
                            aria-hidden
                            className="animate-spin motion-reduce:animate-none"
                          />
                          测试中
                        </>
                      ) : (
                        <>
                          <PlugZap aria-hidden />
                          测试连接
                        </>
                      )}
                    </Button>
                    {testResult ? (
                      <FormStatus
                        message={testResult.message}
                        tone={testResult.tone}
                      />
                    ) : null}
                    {credentialDirty ? (
                      <p className="text-sm text-muted-foreground">
                        请先保存新凭据，再测试连接。
                      </p>
                    ) : null}
                  </div>
                ) : null}
              </div>
            </FormSection>
          ) : (
            <Field
              label="脱敏 Provider config"
              hint="此视图不会包含 secret、掩码、长度或指纹。"
            >
              <StudioTextarea
                readOnly
                rows={7}
                spellCheck={false}
                className="font-mono text-sm leading-6"
                value={
                  resource
                    ? encodeProviderRaw(resource)
                    : `provider = ${JSON.stringify(state.draft.provider)}\ncredential_configured = false\n`
                }
              />
            </Field>
          )}
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
          {state.phase === "conflict" ? (
            <Button
              type="button"
              variant="outline"
              size="lg"
              className="w-fit"
              onClick={() => {
                if (confirmNavigation()) void load()
              }}
            >
              重新加载
            </Button>
          ) : null}
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              size="lg"
              onClick={() => {
                if (confirmNavigation()) router.push(providersHref)
              }}
            >
              取消
            </Button>
            <SaveButton saving={state.phase === "saving"} />
          </div>
        </form>
        {!isNew ? (
          <div className="mt-12">
            <InlineDelete
              resourceLabel="Provider"
              explanation="删除 Provider 会同时移除它的未引用 Models。若它是默认 Model 的来源或被 Agent definition 引用，系统会列出 blocker 并拒绝删除。"
              pending={deleting}
              onDelete={() => void remove()}
            />
          </div>
        ) : null}
      </SettingsShell>
    </PageShell>
  )
}
