"use client"

import { useRouter } from "next/navigation"
import { useEffect, useReducer, useState } from "react"

import { ParameterFields } from "@/components/stratum/studio/parameter-fields"
import {
  BlockerList,
  Field,
  FormStatus,
  InlineDelete,
  SaveButton,
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
  agentDraftToInput,
  agentViewToDraft,
  encodeAgentToml,
  parseAgentToml,
} from "@/features/studio-management/transforms"
import type { AgentDraft } from "@/features/studio-management/types"
import { useDirtyGuard } from "@/features/studio-management/use-dirty-guard"
import { ApiError } from "@/lib/stratum/api"
import type { ModelDescriptor } from "@/lib/stratum/model-config"

const EMPTY_DRAFT: AgentDraft = {
  agentName: "",
  agentVersion: "v1",
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
  const [models, setModels] = useState<readonly ModelDescriptor[]>([])
  const [loading, setLoading] = useState(!isNew)
  const [loadError, setLoadError] = useState<string | null>(null)
  const [notFound, setNotFound] = useState(false)
  const [view, setView] = useState<"structured" | "raw">("structured")
  const [raw, setRaw] = useState(encodeAgentToml(EMPTY_DRAFT))
  const [rawError, setRawError] = useState<string | null>(null)
  const [parametersValid, setParametersValid] = useState(true)
  const [deleting, setDeleting] = useState(false)
  const dirty = isDirtyPhase(state.phase)
  const confirmNavigation = useDirtyGuard(dirty)

  const load = async () => {
    setLoadError(null)
    try {
      const [modelList, resource] = await Promise.all([
        studioApi.getModels(),
        isNew ? Promise.resolve(null) : studioApi.getAgentDefinition(agentName),
      ])
      setModels(modelList)
      if (resource) {
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
    // agentName is a route identity and must reload the editor.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentName])

  const edit = (draft: AgentDraft) => {
    dispatch({ type: "edit", draft })
    if (view === "structured") setRaw(encodeAgentToml(draft))
  }

  const save = async (event: React.FormEvent) => {
    event.preventDefault()
    dispatch({ type: "save" })
    try {
      const input = agentDraftToInput(state.draft)
      const response = isNew
        ? await studioApi.createAgentDefinition(input)
        : await studioApi.updateAgentDefinition(
            agentName,
            {
              agent_version: input.agent_version,
              model: input.model,
              model_parameters: input.model_parameters,
              tools: input.tools,
              prompt: input.prompt,
            },
            state.etag
          )
      const value = agentViewToDraft(response.data)
      dispatch({ type: "acknowledge", value, etag: response.etag })
      setRaw(encodeAgentToml(value))
      if (isNew)
        router.replace(
          `/studio/agents/${encodeURIComponent(response.data.agent_name)}`
        )
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 412) {
        dispatch({
          type: "conflict",
          message: "资源已在别处变更。本地内容仍保留，请重新加载后再决定。",
        })
      } else if (caught instanceof ApiError && caught.status === 409) {
        dispatch({
          type: "blocked",
          message: caught.message,
          blockers: caught.details.blockers ?? [],
        })
      } else if (
        caught instanceof ApiError &&
        (caught.status === 400 || caught.status === 422)
      ) {
        dispatch({
          type: "invalid",
          message: caught.message,
          violations: caught.details.violations,
        })
      } else {
        dispatch({
          type: "invalid",
          message: caught instanceof Error ? caught.message : "保存失败",
        })
      }
    }
  }

  const remove = async () => {
    if (isNew || !agentName) return
    setDeleting(true)
    try {
      await studioApi.deleteAgentDefinition(agentName, state.etag)
      router.replace("/studio")
    } catch (caught) {
      if (caught instanceof ApiError && caught.status === 412)
        dispatch({
          type: "conflict",
          message: "资源已变更，请重新加载后再删除。",
        })
      else if (caught instanceof ApiError && caught.status === 409)
        dispatch({
          type: "blocked",
          message: caught.message,
          blockers: caught.details.blockers ?? [],
        })
      else
        dispatch({
          type: "invalid",
          message: caught instanceof Error ? caught.message : "删除失败",
        })
      setDeleting(false)
    }
  }

  const selectedModel = models.find(
    (model) => model.model === state.draft.model
  )
  const errorTone = state.phase === "invalid" || state.phase === "conflict"

  if (loading)
    return (
      <StudioPage>
        <Skeleton className="h-8 w-52" />
        <Skeleton className="mt-10 h-[36rem] rounded-2xl" />
      </StudioPage>
    )

  if (notFound)
    return (
      <StudioPage>
        <StudioHeader title="Agent 不存在" backHref="/studio" />
        <p className="text-muted-foreground">
          该 definition 不存在或已被删除。
        </p>
      </StudioPage>
    )

  if (loadError)
    return (
      <StudioPage>
        <StudioHeader title="无法打开 Agent" backHref="/studio" />
        <FormStatus message={loadError} tone="error" />
        <Button
          className="mt-5 min-h-11 rounded-xl"
          onClick={() => void load()}
        >
          重试
        </Button>
      </StudioPage>
    )

  return (
    <StudioPage>
      <StudioHeader
        title={isNew ? "新建 Agent" : state.draft.agentName}
        backHref="/studio"
      />
      <form onSubmit={save} className="grid gap-7">
        <div
          className="flex w-fit rounded-xl bg-muted p-1"
          role="tablist"
          aria-label="Agent 编辑视图"
        >
          {(["structured", "raw"] as const).map((item) => (
            <button
              key={item}
              type="button"
              role="tab"
              aria-selected={view === item}
              className="min-h-11 rounded-lg px-4 text-sm font-medium focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-selected:bg-card aria-selected:shadow-sm"
              onClick={() => {
                setView(item)
                if (item === "raw") setRaw(encodeAgentToml(state.draft))
              }}
            >
              {item === "structured" ? "结构化" : "Raw TOML"}
            </button>
          ))}
        </div>

        <section className="rounded-2xl border border-border bg-card p-5 sm:p-7">
          {view === "structured" ? (
            <div className="grid gap-6">
              <Field
                label="名称"
                error={state.violations.agent_name}
                hint={isNew ? "创建后不可修改。" : undefined}
              >
                <StudioInput
                  autoFocus={isNew}
                  disabled={!isNew}
                  value={state.draft.agentName}
                  onChange={(event) =>
                    edit({ ...state.draft, agentName: event.target.value })
                  }
                />
              </Field>
              <Field label="版本" hint="更新行为时必须使用新版本。">
                <StudioInput
                  value={state.draft.agentVersion}
                  onChange={(event) =>
                    edit({ ...state.draft, agentVersion: event.target.value })
                  }
                />
              </Field>
              <Field label="Model" error={state.violations.model}>
                <select
                  className={`${controlClass} h-9 rounded-md border px-3 text-sm outline-none focus-visible:ring-2`}
                  value={state.draft.model}
                  onChange={(event) => {
                    setParametersValid(true)
                    edit({
                      ...state.draft,
                      model: event.target.value,
                      parameters: {},
                    })
                  }}
                >
                  {models.map((model) => (
                    <option key={model.model} value={model.model}>
                      {model.model}
                    </option>
                  ))}
                </select>
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
              <Field
                label="Tools"
                error={state.violations.tools}
                hint="每行一个 tool name。"
              >
                <StudioTextarea
                  rows={5}
                  value={state.draft.tools.join("\n")}
                  onChange={(event) =>
                    edit({
                      ...state.draft,
                      tools: event.target.value.split("\n"),
                    })
                  }
                />
              </Field>
              <Field label="System prompt" error={state.violations.prompt}>
                <StudioTextarea
                  rows={12}
                  value={state.draft.prompt}
                  onChange={(event) =>
                    edit({ ...state.draft, prompt: event.target.value })
                  }
                />
              </Field>
            </div>
          ) : (
            <Field label="Canonical Agent TOML" error={rawError ?? undefined}>
              <StudioTextarea
                rows={22}
                spellCheck={false}
                className="font-mono text-sm"
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
        </section>

        <FormStatus
          message={state.message}
          tone={errorTone ? "error" : state.message ? "success" : "neutral"}
        />
        <BlockerList blockers={state.blockers} />
        {state.phase === "conflict" ? (
          <Button
            type="button"
            variant="outline"
            className="min-h-11 w-fit rounded-xl"
            onClick={() => {
              if (confirmNavigation()) void load()
            }}
          >
            重新加载服务端版本
          </Button>
        ) : null}
        <div className="flex flex-wrap items-center justify-end gap-3">
          <Button
            type="button"
            variant="ghost"
            className="min-h-11 rounded-xl"
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

      {!isNew ? (
        <div className="mt-12">
          <InlineDelete
            resourceLabel="Agent definition"
            explanation="只删除这个 definition。已存在的 runtime Agent、Session 和历史记录不会被删除，但之后不能再用这个名称新建 Agent。"
            pending={deleting}
            onDelete={() => void remove()}
          />
        </div>
      ) : null}
    </StudioPage>
  )
}
