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
  StudioTextarea,
} from "@/components/stratum/studio/primitives"
import { Button } from "@/components/ui/button"
import { encodeModelSchema } from "@/features/studio-management/transforms"
import { useStudioModelEditor } from "@/hooks/use-studio-model-editor"
import type { ProviderKind } from "@/lib/stratum/api"

export function ModelEditor({ modelId }: { modelId?: string }) {
  const {
    cancel,
    deleting,
    edit,
    hasLoadedContent,
    isNew,
    loadError,
    loading,
    modelsHref,
    newModelHref,
    notFound,
    providers,
    reload,
    remove,
    resource,
    retry,
    save,
    state,
  } = useStudioModelEditor(modelId)

  if (loading)
    return (
      <>
        <LoadingState label="正在加载 Model" />
      </>
    )
  if (notFound)
    return (
      <>
        <PageHeader title="Model 不存在" backHref={modelsHref} />
        <NotFoundState
          message="该 Model 不存在或已被删除。可以返回列表，或直接新建一个。"
          createHref={newModelHref}
          createLabel="新建 Model"
        />
      </>
    )
  if (loadError && !hasLoadedContent)
    return (
      <>
        <PageHeader title="无法打开 Model" backHref={modelsHref} />
        <ErrorState
          title="Model 加载失败"
          message={loadError}
          onRetry={retry}
        />
      </>
    )

  return (
    <>
      <PageHeader
        title={isNew ? "新建 Model" : (resource?.name ?? state.draft.modelName)}
        backHref={modelsHref}
        backLabel="返回 Model"
      >
        {!isNew ? (
          <DeleteAction
            resourceLabel="Model"
            explanation="若此 Model 被 Agent definition 引用，系统会列出 blocker 并保持资源不变。"
            pending={deleting}
            disabled={state.phase === "saving"}
            onDelete={() => void remove()}
          />
        ) : null}
      </PageHeader>
      {loadError ? (
        <div className="mb-6">
          <ErrorState
            title="Model 刷新失败"
            message={loadError}
            onRetry={retry}
          />
        </div>
      ) : null}
      <form onSubmit={save} className="grid gap-8">
        <fieldset disabled={state.phase === "saving"} className="contents">
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
                    edit({
                      ...state.draft,
                      provider: next as ProviderKind,
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
                    edit({ ...state.draft, modelName: event.target.value })
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
              返回列表
            </Button>
            {isNew ? (
              <SaveButton
                saving={state.phase === "saving"}
                disabled={providers.length === 0}
              />
            ) : null}
          </div>
        </fieldset>
      </form>
    </>
  )
}
