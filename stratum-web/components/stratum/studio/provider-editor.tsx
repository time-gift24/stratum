"use client"

import {
  BlockerList,
  DeleteAction,
  ErrorState,
  Field,
  FormSection,
  FormStatus,
  LoadingState,
  NotFoundState,
  PageHeader,
  SaveButton,
  StudioInput,
  StudioSelect,
} from "@/components/stratum/studio/primitives"
import { Button } from "@/components/ui/button"
import { ProviderModelsSection } from "@/components/stratum/studio/provider-models"
import { useStudioProviderEditor } from "@/hooks/use-studio-provider-editor"
import type { ProviderKind } from "@/lib/stratum/api"

export function ProviderEditor({ provider }: { provider?: string }) {
  const {
    cancel,
    deleting,
    edit,
    hasLoadedContent,
    isNew,
    loadError,
    loading,
    newProviderHref,
    notFound,
    providersHref,
    reload,
    remove,
    resource,
    retry,
    save,
    state,
  } = useStudioProviderEditor(provider)

  if (loading)
    return (
      <>
        <LoadingState label="正在加载 Provider" />
      </>
    )
  if (notFound)
    return (
      <>
        <PageHeader title="Provider 不存在" backHref={providersHref} />
        <NotFoundState
          message="该 Provider 不存在或已被删除。可以返回列表，或直接新建一个。"
          createHref={newProviderHref}
          createLabel="新建 Provider"
        />
      </>
    )
  if (loadError && !hasLoadedContent)
    return (
      <>
        <PageHeader title="无法打开 Provider" backHref={providersHref} />
        <ErrorState
          title="Provider 加载失败"
          message={loadError}
          onRetry={retry}
        />
      </>
    )

  return (
    <>
      <PageHeader
        title={isNew ? "新建 Provider" : state.draft.provider}
        backHref={providersHref}
        backLabel="返回 Provider"
      >
        {!isNew ? (
          <DeleteAction
            resourceLabel="Provider"
            explanation="若没有 Agent definition 引用它的 Model，删除 Provider 会同时删除其 Models 与已存凭据；否则系统会列出 blocker 并保持资源不变。"
            pending={deleting}
            disabled={state.phase === "saving"}
            onDelete={() => void remove()}
          />
        ) : null}
      </PageHeader>
      {loadError ? (
        <div className="mb-6">
          <ErrorState
            title="Provider 刷新失败"
            message={loadError}
            onRetry={retry}
          />
        </div>
      ) : null}
      <form onSubmit={save} className="grid gap-8">
        <fieldset
          disabled={state.phase === "saving"}
          className="contents"
        >
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
            </div>
          </FormSection>
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
              className="min-h-11 w-fit"
              onClick={reload}
            >
              重新加载
            </Button>
          ) : null}
          <div className="flex justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              size="lg"
              className="min-h-11"
              onClick={cancel}
            >
              取消
            </Button>
            <SaveButton saving={state.phase === "saving"} />
          </div>
        </fieldset>
      </form>
      {!isNew ? (
        <div className="mt-8">
          <ProviderModelsSection provider={state.draft.provider} />
        </div>
      ) : null}
    </>
  )
}
