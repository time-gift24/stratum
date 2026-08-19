"use client"

/**
 * Provider 编辑器内的 Model 列表：该 Provider 名下的全部 Model。
 * 每行可进入编辑器，也可发一次真实消息验证可用性（显示延迟或失败原因）；
 * 数据走页面缓存（SWR），新建/删除由 model 编辑器按前缀失效。
 */

import Link from "next/link"
import { useCallback, useEffect, useState } from "react"
import { ChevronRight, LoaderCircle, Play, Plus } from "lucide-react"

import {
  FormSection,
  LoadingState,
  useDelayedFlag,
} from "@/components/stratum/studio/primitives"
import { Button, buttonVariants } from "@/components/ui/button"
import {
  safeStudioErrorMessage,
  studioApi,
} from "@/features/studio-management/client"
import { withStudioReturn } from "@/features/studio-management/navigation"
import { readPageCache, writePageCache } from "@/lib/page-cache"
import type { ManagedModelSummary, ProviderKind } from "@/lib/stratum/api"

type TestState =
  | { status: "idle" }
  | { status: "testing" }
  | { status: "ok"; latencyMs: number }
  | { status: "failed"; message: string }

export function ProviderModelsSection({
  provider,
  returnTo,
}: {
  provider: ProviderKind
  returnTo: string
}) {
  const cacheKey = `studio:provider-models:${provider}`
  const [models, setModels] = useState<readonly ManagedModelSummary[] | null>(
    () => readPageCache(cacheKey)
  )
  const [error, setError] = useState<string | null>(null)
  const [tests, setTests] = useState<Record<string, TestState>>({})

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
                <Link
                  href={withStudioReturn(
                    `/studio/settings/models/${encodeURIComponent(model.model_id)}`,
                    returnTo
                  )}
                  className="min-w-0 flex-1 truncate font-mono text-sm text-foreground outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring"
                >
                  {model.name}
                </Link>
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
                <Link
                  href={withStudioReturn(
                    `/studio/settings/models/${encodeURIComponent(model.model_id)}`,
                    returnTo
                  )}
                  aria-label={`编辑 ${model.name}`}
                  className="flex shrink-0 items-center rounded-md p-1 text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
                >
                  <ChevronRight aria-hidden className="size-4" />
                </Link>
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
      <div className="mt-4">
        <Link
          href={withStudioReturn(
            `/studio/settings/providers/${provider}/models/new`,
            returnTo
          )}
          className={buttonVariants({ variant: "outline", size: "sm" })}
        >
          <Plus aria-hidden />
          添加 Model
        </Link>
      </div>
    </FormSection>
  )
}
