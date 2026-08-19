"use client"

/**
 * Provider 编辑器内的 Model 管理：名下 Model 列表，每行可发一次真实消息
 * 测试（显示延迟或失败原因）或删除（Popover 确认，删除前先取 ETag）；
 * 底部行内输入名称直接添加。数据走页面缓存（SWR）。
 */

import { useCallback, useEffect, useState } from "react"
import { LoaderCircle, Play, Plus, Trash2 } from "lucide-react"

import {
  FormSection,
  LoadingState,
  StudioInput,
  useDelayedFlag,
} from "@/components/stratum/studio/primitives"
import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  safeStudioErrorMessage,
  studioApi,
} from "@/features/studio-management/client"
import {
  invalidatePageCache,
  readPageCache,
  writePageCache,
} from "@/lib/page-cache"
import { ApiError } from "@/lib/stratum/api"
import type { ManagedModelSummary, ProviderKind } from "@/lib/stratum/api"

type TestState =
  | { status: "idle" }
  | { status: "testing" }
  | { status: "ok"; latencyMs: number }
  | { status: "failed"; message: string }

export function ProviderModelsSection({ provider }: { provider: ProviderKind }) {
  const cacheKey = `studio:provider-models:${provider}`
  const [models, setModels] = useState<readonly ManagedModelSummary[] | null>(
    () => readPageCache(cacheKey)
  )
  const [error, setError] = useState<string | null>(null)
  const [tests, setTests] = useState<Record<string, TestState>>({})
  const [newName, setNewName] = useState("")
  const [adding, setAdding] = useState(false)
  const [deleting, setDeleting] = useState<string | null>(null)
  const [actionError, setActionError] = useState<string | null>(null)

  const load = useCallback(async () => {
    try {
      const page = await studioApi.listManagedModels({
        provider,
        page: 1,
        perPage: 100,
      })
      writePageCache(cacheKey, page.data)
      setModels(page.data)
      setError(null)
    } catch (caught) {
      if (readPageCache(cacheKey) === null)
        setError(safeStudioErrorMessage(caught, "无法加载 Model"))
    }
  }, [cacheKey, provider])

  useEffect(() => {
    void load()
  }, [load])

  const invalidate = () => {
    invalidatePageCache("studio:provider-models:")
    invalidatePageCache("studio-settings:")
    invalidatePageCache("studio:provider:")
  }

  const test = async (model: ManagedModelSummary) => {
    setTests((prev) => ({ ...prev, [model.model_id]: { status: "testing" } }))
    try {
      const result = await studioApi.testManagedModel(
        model.provider,
        model.name
      )
      setTests((prev) => ({
        ...prev,
        [model.model_id]: { status: "ok", latencyMs: result.latency_ms },
      }))
    } catch (caught) {
      setTests((prev) => ({
        ...prev,
        [model.model_id]: {
          status: "failed",
          message: safeStudioErrorMessage(caught, "测试失败"),
        },
      }))
    }
  }

  const add = async () => {
    const name = newName.trim()
    if (name === "" || adding) return
    setAdding(true)
    setActionError(null)
    try {
      await studioApi.createManagedModel({ provider, name })
      setNewName("")
      invalidate()
      await load()
    } catch (caught) {
      setActionError(
        caught instanceof ApiError && caught.status === 409
          ? "同名 Model 已存在。"
          : safeStudioErrorMessage(caught, "添加失败")
      )
    } finally {
      setAdding(false)
    }
  }

  // 列表项不带 ETag：删除前先 GET 一次取当前 ETag，412 时提示重试
  const remove = async (model: ManagedModelSummary) => {
    if (deleting !== null) return
    setDeleting(model.model_id)
    setActionError(null)
    try {
      const current = await studioApi.getManagedModel(
        model.provider,
        model.name
      )
      await studioApi.deleteManagedModel(
        model.provider,
        model.name,
        current.etag
      )
      invalidate()
      await load()
    } catch (caught) {
      setActionError(
        caught instanceof ApiError && caught.status === 412
          ? "Model 已在别处变更，请重试。"
          : safeStudioErrorMessage(caught, "删除失败")
      )
    } finally {
      setDeleting(null)
    }
  }

  // 加载指示延迟 150ms：本地接口常在 10ms 内返回，不闪一帧 spinner
  const showLoading = useDelayedFlag(models === null && error === null)

  return (
    <FormSection
      title="Model"
      description="测试会向该 Model 发送一条真实消息，验证它当前可用。"
    >
      {models === null && error === null ? (
        showLoading ? (
          <LoadingState label="正在加载 Model" />
        ) : null
      ) : error !== null ? (
        <p className="text-sm text-destructive" role="alert">
          {error}
        </p>
      ) : (
        <ul className="divide-y divide-border rounded-xl border border-border">
          {models?.map((model) => {
            const testState = tests[model.model_id] ?? { status: "idle" }
            return (
              <li
                key={model.model_id}
                className="flex items-center gap-3 px-4 py-2.5"
              >
                <span className="min-w-0 flex-1 truncate font-mono text-sm text-foreground">
                  {model.name}
                </span>
                {testState.status === "ok" ? (
                  <span className="shrink-0 text-xs text-success">
                    可用 · {testState.latencyMs}ms
                  </span>
                ) : testState.status === "failed" ? (
                  <span
                    className="max-w-64 shrink-0 truncate text-xs text-destructive"
                    title={testState.message}
                  >
                    {testState.message}
                  </span>
                ) : null}
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  aria-label={`测试 ${model.name}`}
                  title="发送真实消息测试"
                  disabled={testState.status === "testing"}
                  onClick={() => void test(model)}
                >
                  {testState.status === "testing" ? (
                    <LoaderCircle
                      aria-hidden
                      className="animate-spin motion-reduce:animate-none"
                    />
                  ) : (
                    <Play aria-hidden />
                  )}
                </Button>
                <Popover>
                  <PopoverTrigger
                    aria-label={`删除 ${model.name}`}
                    className="flex size-9 shrink-0 items-center justify-center rounded-lg text-muted-foreground transition-colors outline-none hover:bg-destructive/10 hover:text-destructive focus-visible:ring-2 focus-visible:ring-ring"
                  >
                    <Trash2 aria-hidden className="size-4" />
                  </PopoverTrigger>
                  <PopoverContent align="end" className="w-72">
                    <p className="text-sm font-medium">删除 {model.name}？</p>
                    <p className="mt-1 text-sm text-muted-foreground">
                      若有 Agent definition 引用它，系统会拒绝并列出引用。
                    </p>
                    <div className="mt-3 flex justify-end">
                      <Button
                        type="button"
                        variant="destructive"
                        size="sm"
                        disabled={deleting === model.model_id}
                        onClick={() => void remove(model)}
                      >
                        {deleting === model.model_id ? "正在删除" : "确认删除"}
                      </Button>
                    </div>
                  </PopoverContent>
                </Popover>
              </li>
            )
          })}
        </ul>
      )}
      {models !== null && models.length === 0 ? (
        <p className="mt-2 text-sm text-muted-foreground">
          尚无 Model，添加后 Agent 才能使用这个 Provider。
        </p>
      ) : null}
      {actionError !== null ? (
        <p className="mt-2 text-sm text-destructive" role="alert">
          {actionError}
        </p>
      ) : null}
      <div className="mt-4 flex gap-2">
        <StudioInput
          value={newName}
          onChange={(event) => setNewName(event.target.value)}
          onKeyDown={(event) => {
            // 本区块嵌在 Provider 表单内，不能用原生 form 提交；Enter 手动触发
            if (event.key === "Enter") {
              event.preventDefault()
              void add()
            }
          }}
          placeholder="Model 名称，如 deepseek-v4-flash"
          aria-label="新 Model 名称"
          className="max-w-xs font-mono"
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={adding || newName.trim() === ""}
          onClick={() => void add()}
        >
          <Plus aria-hidden />
          {adding ? "添加中" : "添加 Model"}
        </Button>
      </div>
    </FormSection>
  )
}
