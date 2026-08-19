"use client"

import Link from "next/link"
import dynamic from "next/dynamic"
import { useState } from "react"

import { ParameterFields } from "@/components/stratum/studio/parameter-fields"
import { ToolsSelect } from "@/components/stratum/studio/tools-select"
import proseStyles from "@/components/stratum/styles/prose-medium.module.css"
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
  PageShell,
  SaveButton,
  StudioInput,
  StudioSelect,
  StudioTextarea,
} from "@/components/stratum/studio/primitives"
import { Button, buttonVariants } from "@/components/ui/button"
import { Textarea } from "@/components/ui/textarea"
import { useStudioAgentEditor } from "@/hooks/use-studio-agent-editor"
import { cn } from "@/lib/utils"

const Streamdown = dynamic(
  () => import("streamdown").then((module) => module.Streamdown),
  { ssr: false }
)

export function AgentEditor({ agentName }: { agentName?: string }) {
  const {
    cancel,
    deleting,
    dirty,
    dispatch,
    edit,
    hasLoadedContent,
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
    syncRaw,
    tools,
    toolsError,
    updateRaw,
    versionError,
    versionIsReady,
  } = useStudioAgentEditor(agentName)
  const [view, setView] = useState<"structured" | "prompt" | "raw">(
    "structured"
  )
  const [promptMode, setPromptMode] = useState<"edit" | "preview">("edit")

  const selectedModel = models.find(
    (model) => model.model === state.draft.model
  )
  const errorTone = state.phase === "invalid" || state.phase === "conflict"
  const promptLines = state.draft.prompt.split("\n").length
  const promptChars = state.draft.prompt.length

  if (loading)
    return (
      <PageShell>
        <LoadingState label="正在加载 Agent" />
      </PageShell>
    )

  if (notFound)
    return (
      <PageShell>
        <PageHeader title="Agent 不存在" backHref="/studio" />
        <NotFoundState
          message="该 definition 不存在或已被删除。可以返回仪表盘，或直接新建一个。"
          createHref="/studio/agents/new"
          createLabel="新建 Agent"
        />
      </PageShell>
    )

  if (loadError && !hasLoadedContent)
    return (
      <PageShell>
        <PageHeader title="无法打开 Agent" backHref="/studio" />
        <ErrorState
          title="Agent 加载失败"
          message={loadError}
          onRetry={retry}
        />
      </PageShell>
    )

  return (
    <PageShell>
      <PageHeader
        title={isNew ? "新建 Agent" : state.draft.agentName}
        backHref="/studio"
      >
        {!isNew ? (
          <DeleteAction
            resourceLabel="Agent definition"
            explanation="只删除这个 definition。已存在的 runtime Agent、Session 和历史记录不会被删除，但之后不能再用这个名称新建 Agent。"
            pending={deleting}
            disabled={state.phase === "saving"}
            onDelete={() => void remove()}
          />
        ) : null}
      </PageHeader>
      {loadError ? (
        <div className="mb-6">
          <ErrorState
            title="Agent 刷新失败"
            message={loadError}
            onRetry={retry}
          />
        </div>
      ) : null}
      <form onSubmit={save} className="grid gap-8">
        <fieldset disabled={state.phase === "saving"} className="contents">
          <div
            className="flex w-fit rounded-lg bg-muted p-1"
            role="tablist"
            aria-label="Agent 编辑视图"
          >
            {(["structured", "prompt", "raw"] as const).map((item) => (
              <Button
                key={item}
                type="button"
                variant="ghost"
                size="lg"
                role="tab"
                aria-selected={view === item}
                className="h-8 min-h-11 rounded-md px-3 text-sm aria-selected:bg-card aria-selected:shadow-sm sm:min-h-8"
                onClick={() => {
                  setView(item)
                  if (item === "raw") syncRaw()
                }}
              >
                {item === "structured"
                  ? "结构化"
                  : item === "prompt"
                    ? "System prompt"
                    : "Raw TOML"}
              </Button>
            ))}
          </div>

          {view === "structured" ? (
            <>
              <FormSection
                title="身份"
                description="名称是 Agent definition 的稳定标识，创建后不可修改。"
              >
                <Field
                  label="名称"
                  error={state.violations.agent_name}
                  hint={isNew ? "创建后不可修改。" : undefined}
                >
                  <StudioInput
                    autoFocus={isNew}
                    disabled={!isNew}
                    className="font-mono"
                    value={state.draft.agentName}
                    onChange={(event) =>
                      edit({ ...state.draft, agentName: event.target.value })
                    }
                  />
                </Field>
                <Field
                  label="版本标签"
                  error={
                    state.violations.agent_version ?? versionError ?? undefined
                  }
                  hint={
                    isNew
                      ? "作者定义的版本标识；默认 v1，可在创建前修改。"
                      : `当前版本为 ${state.acknowledged.agentVersion}。每次保存都必须填写一个不同的新标签。`
                  }
                >
                  <StudioInput
                    required
                    maxLength={128}
                    className="font-mono"
                    value={state.draft.agentVersion}
                    onChange={(event) =>
                      edit({
                        ...state.draft,
                        agentVersion: event.target.value,
                      })
                    }
                  />
                </Field>
              </FormSection>

              <FormSection
                title="模型"
                description="可配参数由所选模型的 schema 决定。"
              >
                {models.length === 0 ? (
                  <div className="grid justify-items-start gap-3">
                    <p className="text-sm leading-6 text-muted-foreground">
                      还没有可用的 Model。先在设置里配置 Provider 和
                      Model，再回来创建 Agent。
                    </p>
                    <Link
                      href="/studio/settings/providers"
                      className={buttonVariants({
                        size: "lg",
                        className: "min-h-11",
                      })}
                    >
                      去配置 Provider 和 Model
                    </Link>
                  </div>
                ) : (
                  <div className="grid gap-6">
                    <Field error={state.violations.model}>
                      <StudioSelect
                        ariaLabel="Model"
                        value={state.draft.model}
                        options={models.map((model) => ({
                          value: model.model,
                          label: model.model,
                        }))}
                        onChange={selectModel}
                      />
                    </Field>
                    {selectedModel ? (
                      <div className="grid gap-2">
                        <ParameterFields
                          key={`${state.draft.model}:${state.etag}`}
                          schema={selectedModel.parameters_schema}
                          parameters={state.draft.parameters}
                          onChange={(parameters) =>
                            edit({ ...state.draft, parameters })
                          }
                          onInvalidEdit={() =>
                            dispatch({
                              type: "edit",
                              draft: state.draft,
                              forceDirty: true,
                            })
                          }
                          onValidityChange={setParametersValid}
                        />
                        {state.violations.model_parameters ? (
                          <p className="text-sm text-destructive" role="alert">
                            {state.violations.model_parameters}
                          </p>
                        ) : null}
                      </div>
                    ) : null}
                  </div>
                )}
              </FormSection>

              <FormSection
                title="工具"
                description="目录来自 host 当前实际可注册的工具。"
              >
                <div className="grid gap-4">
                  <Field error={state.violations.tools}>
                    <ToolsSelect
                      catalog={tools}
                      value={state.draft.tools}
                      onChange={(next) => edit({ ...state.draft, tools: next })}
                    />
                  </Field>
                  {toolsError ? (
                    <div className="flex flex-wrap items-center gap-3">
                      <FormStatus
                        message="工具目录加载失败，已选工具仍保留。"
                        tone="error"
                      />
                      <Button
                        type="button"
                        variant="outline"
                        size="lg"
                        className="min-h-11"
                        onClick={() => void loadTools()}
                      >
                        重试
                      </Button>
                    </div>
                  ) : null}
                </div>
              </FormSection>
            </>
          ) : view === "prompt" ? (
            <div className="grid gap-4">
              <div className="flex items-center justify-between gap-3">
                <div
                  className="flex w-fit rounded-lg bg-muted p-1"
                  role="tablist"
                  aria-label="Prompt 编辑模式"
                >
                  {(["edit", "preview"] as const).map((mode) => (
                    <Button
                      key={mode}
                      type="button"
                      variant="ghost"
                      size="lg"
                      role="tab"
                      aria-selected={promptMode === mode}
                      className="h-8 min-h-11 rounded-md px-3 text-sm aria-selected:bg-card aria-selected:shadow-sm sm:min-h-8"
                      onClick={() => setPromptMode(mode)}
                    >
                      {mode === "edit" ? "修改" : "预览"}
                    </Button>
                  ))}
                </div>
                <span className="text-xs text-muted-foreground">
                  共 {promptLines} 行 · {promptChars} 字符
                </span>
              </div>
              {state.violations.prompt ? (
                <p className="text-sm text-destructive" role="alert">
                  {state.violations.prompt}
                </p>
              ) : null}
              <div className="mx-auto w-full max-w-[46rem] rounded-2xl bg-card px-5 py-6 transition-shadow focus-within:ring-2 focus-within:ring-ring/25 motion-reduce:transition-none sm:px-10 sm:py-9">
                {promptMode === "edit" ? (
                  <Textarea
                    aria-label="System prompt"
                    aria-invalid={state.violations.prompt ? true : undefined}
                    placeholder="用 Markdown 编写 System prompt，描述这个 Agent 的角色、边界与输出风格。"
                    className="min-h-[50vh] rounded-none border-0 bg-transparent p-0 font-[family-name:var(--font-reading)] text-[0.9375rem] leading-7 shadow-none focus-visible:border-transparent focus-visible:ring-0 md:text-[1.0625rem] md:leading-8 dark:bg-transparent"
                    value={state.draft.prompt}
                    onChange={(event) =>
                      edit({ ...state.draft, prompt: event.target.value })
                    }
                  />
                ) : state.draft.prompt.trim() === "" ? (
                  <p className="py-16 text-center text-sm text-muted-foreground">
                    还没有内容。切到修改模式编写 System prompt，支持 Markdown。
                  </p>
                ) : (
                  <div
                    className={cn(
                      proseStyles.proseMedium,
                      proseStyles.proseMediumChat,
                      "text-foreground"
                    )}
                  >
                    <Streamdown mode="static">{state.draft.prompt}</Streamdown>
                  </div>
                )}
              </div>
            </div>
          ) : (
            <Field label="Canonical Agent TOML" error={rawError ?? undefined}>
              <StudioTextarea
                rows={22}
                spellCheck={false}
                className="font-mono text-sm leading-6"
                value={raw}
                onChange={(event) => updateRaw(event.target.value)}
              />
            </Field>
          )}

          <FormStatus
            message={state.message}
            tone={errorTone ? "error" : state.message ? "success" : "neutral"}
          />
          <BlockerList blockers={state.blockers} />
          {!isNew && dirty && !versionIsReady ? (
            <p className="text-sm text-muted-foreground">
              保存前请在“结构化”视图中填写一个新的版本标签。
            </p>
          ) : null}
          {state.phase === "conflict" ? (
            <Button
              type="button"
              variant="outline"
              size="lg"
              className="min-h-11 w-fit"
              onClick={reload}
            >
              重新加载服务端版本
            </Button>
          ) : null}
          <div className="flex flex-wrap items-center justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              size="lg"
              className="min-h-11"
              onClick={cancel}
            >
              取消
            </Button>
            <SaveButton
              saving={state.phase === "saving"}
              disabled={
                selectedModel === undefined ||
                rawError !== null ||
                !parametersValid ||
                !versionIsReady
              }
            />
          </div>
        </fieldset>
      </form>
    </PageShell>
  )
}
