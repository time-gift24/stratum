"use client"

import Link from "next/link"
import { useRouter } from "next/navigation"
import { useEffect, useReducer, useState } from "react"
import { Streamdown } from "streamdown"

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
import { studioApi } from "@/features/studio-management/client"
import {
  dispatchApiError,
  formReducer,
  initialFormState,
  isDirtyPhase,
} from "@/features/studio-management/form-state"
import {
  agentDraftToInput,
  agentViewToDraft,
  encodeAgentToml,
  parseAgentToml,
} from "@/features/studio-management/transforms"
import type { AgentDraft } from "@/features/studio-management/types"
import { useDirtyGuard } from "@/features/studio-management/use-dirty-guard"
import { cn } from "@/lib/utils"
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
  model: "",
  parameters: {},
  tools: [],
  prompt: "",
}

export function AgentEditor({ agentName }: { agentName?: string }) {
  const isNew = agentName === undefined
  const router = useRouter()
  const [state, dispatch] = useReducer(
    formReducer<AgentDraft>,
    EMPTY_DRAFT,
    initialFormState
  )
  const defCacheKey = isNew ? null : `studio:agent-def:${agentName}`
  const cachedDef = defCacheKey
    ? readPageCache<ResourceRevision<AgentDefinitionView>>(defCacheKey)
    : null
  const [models, setModels] = useState<readonly ModelDescriptor[]>(
    () => readPageCache("studio:catalog:models") ?? []
  )
  const [tools, setTools] = useState<readonly ToolView[]>(
    () => readPageCache("studio:catalog:tools") ?? []
  )
  const [toolsError, setToolsError] = useState(false)
  const [loading, setLoading] = useState(!isNew && cachedDef === null)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [notFound, setNotFound] = useState(false)
  const [view, setView] = useState<"structured" | "prompt" | "raw">(
    "structured"
  )
  const [promptMode, setPromptMode] = useState<"edit" | "preview">("edit")
  const [raw, setRaw] = useState(encodeAgentToml(EMPTY_DRAFT))
  const [rawError, setRawError] = useState<string | null>(null)
  const [parametersValid, setParametersValid] = useState(true)
  const [deleting, setDeleting] = useState(false)
  const dirty = isDirtyPhase(state.phase)
  const confirmNavigation = useDirtyGuard(dirty)

  // 重访时先用缓存的 definition 填表单（首帧可渲染），权威版本由 load 刷新
  const [appliedCacheKey, setAppliedCacheKey] = useState<string | null>(null)
  if (cachedDef && appliedCacheKey !== defCacheKey) {
    setAppliedCacheKey(defCacheKey)
    const draft = agentViewToDraft(cachedDef.data)
    dispatch({ type: "reload", value: draft, etag: cachedDef.etag })
    setRaw(encodeAgentToml(draft))
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

  const load = async () => {
    setLoadError(null)
    try {
      const [modelList, resource] = await Promise.all([
        studioApi.getModels(),
        isNew ? Promise.resolve(null) : studioApi.getAgentDefinition(agentName),
      ])
      writePageCache("studio:catalog:models", modelList)
      setModels(modelList)
      if (resource) {
        if (defCacheKey) writePageCache(defCacheKey, resource)
        const draft = agentViewToDraft(resource.data)
        dispatch({ type: "reload", value: draft, etag: resource.etag })
        setRaw(encodeAgentToml(draft))
        setRawError(null)
        setParametersValid(true)
      } else if (modelList[0]) {
        const draft = {
          ...EMPTY_DRAFT,
          model: modelList[0].model,
        }
        dispatch({ type: "reload", value: draft, etag: "" })
        setRaw(encodeAgentToml(draft))
        setRawError(null)
        setParametersValid(true)
      }
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 404) setNotFound(true)
      else if (cachedDef === null)
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
    // agentName is a route identity and must reload the editor.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentName])

  useEffect(() => {
    const timer = window.setTimeout(() => void loadTools(), 0)
    return () => window.clearTimeout(timer)
  }, [])

  const edit = (draft: AgentDraft) => {
    // raw 视图在切换页签时从 draft 重新生成，这里不再逐键编码
    dispatch({ type: "edit", draft })
  }

  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    dispatch({ type: "save" })
    try {
      const input = agentDraftToInput(state.draft)
      const response = isNew
        ? await studioApi.createAgentDefinition(input)
        : await studioApi.updateAgentDefinition(agentName, input, state.etag)
      const value = agentViewToDraft(response.data)
      dispatch({ type: "acknowledge", value, etag: response.etag })
      setRaw(encodeAgentToml(value))
      invalidatePageCache("studio-agents:")
      writePageCache(`studio:agent-def:${response.data.agent_name}`, {
        data: response.data,
        etag: response.etag,
      })
      if (isNew)
        router.replace(
          `/studio/agents/${encodeURIComponent(response.data.agent_name)}`
        )
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
      router.replace("/studio")
    } catch (caught) {
      dispatchApiError(dispatch, caught, {
        conflict: "资源已变更，请重新加载后再删除。",
        fallback: "删除失败",
      })
      setDeleting(false)
    }
  }

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

  if (loadError)
    return (
      <PageShell>
        <PageHeader title="无法打开 Agent" backHref="/studio" />
        <ErrorState
          title="Agent 加载失败"
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
        title={isNew ? "新建 Agent" : state.draft.agentName}
        backHref="/studio"
      >
        {!isNew ? (
          <DeleteAction
            resourceLabel="Agent definition"
            explanation="只删除这个 definition。已存在的 runtime Agent、Session 和历史记录不会被删除，但之后不能再用这个名称新建 Agent。"
            pending={deleting}
            onDelete={() => void remove()}
          />
        ) : null}
      </PageHeader>
      <form onSubmit={save} className="grid gap-8">
        <div
          className="flex w-fit rounded-lg bg-muted p-1"
          role="tablist"
          aria-label="Agent 编辑视图"
        >
          {(["structured", "prompt", "raw"] as const).map((item) => (
            <button
              key={item}
              type="button"
              role="tab"
              aria-selected={view === item}
              className="h-8 rounded-md px-3 text-sm font-medium focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-selected:bg-card aria-selected:shadow-sm"
              onClick={() => {
                setView(item)
                if (item === "raw") setRaw(encodeAgentToml(state.draft))
              }}
            >
              {item === "structured"
                ? "结构化"
                : item === "prompt"
                  ? "System prompt"
                  : "Raw TOML"}
            </button>
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
                    href="/studio/settings/models/new"
                    className={buttonVariants({ size: "lg" })}
                  >
                    去配置 Model
                  </Link>
                </div>
              ) : (
                <div className="grid gap-6">
                  <Field label="Model" error={state.violations.model}>
                    <StudioSelect
                      ariaLabel="Model"
                      value={state.draft.model}
                      options={models.map((model) => ({
                        value: model.model,
                        label: model.model,
                      }))}
                      onChange={(next) => {
                        setParametersValid(true)
                        edit({
                          ...state.draft,
                          model: next,
                          parameters: {},
                        })
                      }}
                    />
                  </Field>
                  {selectedModel ? (
                    <ParameterFields
                      key={`${state.draft.model}:${state.etag}`}
                      schema={selectedModel.parameters_schema}
                      parameters={state.draft.parameters}
                      onChange={(parameters) =>
                        edit({ ...state.draft, parameters })
                      }
                      onInvalidEdit={() =>
                        dispatch({ type: "edit", draft: state.draft })
                      }
                      onValidityChange={setParametersValid}
                    />
                  ) : null}
                </div>
              )}
            </FormSection>

            <FormSection
              title="工具"
              description="目录来自 host 当前实际可注册的工具。"
            >
              <div className="grid gap-4">
                <Field label="Tools" error={state.violations.tools}>
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
                  <button
                    key={mode}
                    type="button"
                    role="tab"
                    aria-selected={promptMode === mode}
                    className="h-8 rounded-md px-3 text-sm font-medium focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-selected:bg-card aria-selected:shadow-sm"
                    onClick={() => setPromptMode(mode)}
                  >
                    {mode === "edit" ? "修改" : "预览"}
                  </button>
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
              onChange={(event) => {
                const source = event.target.value
                setRaw(source)
                const parsed = parseAgentToml(source)
                if (parsed.ok) {
                  setRawError(null)
                  setParametersValid(true)
                  dispatch({
                    type: "edit",
                    draft: {
                      ...parsed.draft,
                      agentName: state.draft.agentName,
                    },
                  })
                } else {
                  setRawError(`第 ${parsed.line} 行：${parsed.message}`)
                }
              }}
            />
          </Field>
        )}

        <FormStatus
          message={state.message}
          tone={errorTone ? "error" : state.message ? "success" : "neutral"}
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
            重新加载服务端版本
          </Button>
        ) : null}
        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button
            type="button"
            variant="ghost"
            size="lg"
            onClick={() => {
              if (confirmNavigation()) router.push("/studio")
            }}
          >
            取消
          </Button>
          <SaveButton
            saving={state.phase === "saving"}
            disabled={rawError !== null || !parametersValid}
          />
        </div>
      </form>
    </PageShell>
  )
}
