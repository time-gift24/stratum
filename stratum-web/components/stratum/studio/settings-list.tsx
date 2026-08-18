"use client"

import Link from "next/link"
import { useRouter, useSearchParams } from "next/navigation"
import { useCallback, useEffect, useRef, useState } from "react"
import { Box, Cpu, KeyRound, Plug, Plus, Search } from "lucide-react"

import {
  ErrorState,
  LoadingState,
  PageHeader,
  Pagination,
  ResourceCard,
  StatusChip,
  StudioInput,
} from "@/components/stratum/studio/primitives"
import { Button, buttonVariants } from "@/components/ui/button"
import { studioApi } from "@/features/studio-management/client"
import {
  safeStudioReturn,
  withStudioReturn,
} from "@/features/studio-management/navigation"
import {
  readPageCache,
  writePageCache,
} from "@/lib/page-cache"
import type {
  ManagedModelView,
  PageEnvelope,
  ProviderView,
} from "@/lib/stratum/api"

const PER_PAGE = 12

function safePage(value: string | null): number {
  const parsed = Number(value)
  return Number.isInteger(parsed) && parsed > 0 ? parsed : 1
}

export function SettingsList({ kind }: { kind: "providers" | "models" }) {
  const router = useRouter()
  const searchParams = useSearchParams()
  const query = searchParams.get("q")?.trim() ?? ""
  const page = safePage(searchParams.get("page"))
  const returnTo = safeStudioReturn(searchParams.get("returnTo"))
  const cacheKey = `studio-settings:${kind}:${page}:${query}`
  const [cached, setCached] = useState(() =>
    readPageCache<{
      result: PageEnvelope<ProviderView> | PageEnvelope<ManagedModelView>
      hasResources: boolean
    }>(cacheKey)
  )
  const [providers, setProviders] = useState<PageEnvelope<ProviderView> | null>(
    () => (cached?.result as PageEnvelope<ProviderView> | undefined) ?? null
  )
  const [models, setModels] = useState<PageEnvelope<ManagedModelView> | null>(
    () => (cached?.result as PageEnvelope<ManagedModelView> | undefined) ?? null
  )
  const [hasResources, setHasResources] = useState(
    () => cached?.hasResources ?? false
  )
  const [error, setError] = useState<string | null>(null)
  const requestIdRef = useRef(0)

  // kind/参数变化时同步切缓存，不等 effect
  const [seenKey, setSeenKey] = useState(cacheKey)
  if (seenKey !== cacheKey) {
    setSeenKey(cacheKey)
    const next = readPageCache<{
      result: PageEnvelope<ProviderView> | PageEnvelope<ManagedModelView>
      hasResources: boolean
    }>(cacheKey)
    setCached(next)
    if (kind === "providers") {
      setProviders(
        (next?.result as PageEnvelope<ProviderView> | undefined) ?? null
      )
      setModels(null)
    } else {
      setModels(
        (next?.result as PageEnvelope<ManagedModelView> | undefined) ?? null
      )
      setProviders(null)
    }
    setHasResources(next?.hasResources ?? false)
    setError(null)
  }

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current
    setError(null)
    try {
      if (kind === "providers") {
        const all = await studioApi.listProviders({ page: 1, perPage: 100 })
        if (requestId !== requestIdRef.current) return
        const normalized = query.toLocaleLowerCase()
        const filtered = all.data.filter(
          (provider) =>
            normalized === "" ||
            provider.provider.toLocaleLowerCase().includes(normalized)
        )
        const start = (page - 1) * PER_PAGE
        const result: PageEnvelope<ProviderView> = {
          data: filtered.slice(start, start + PER_PAGE),
          pagination: {
            page,
            per_page: PER_PAGE,
            total: filtered.length,
          },
        }
        const nextCache = {
          result,
          hasResources: all.pagination.total > 0,
        }
        writePageCache(cacheKey, nextCache)
        setCached(nextCache)
        setProviders(result)
        setHasResources(nextCache.hasResources)
      } else {
        const [modelPage, providerPage] = await Promise.all([
          studioApi.listManagedModels({
            page,
            perPage: PER_PAGE,
            search: query,
          }),
          studioApi.listProviders({ page: 1, perPage: 100 }),
        ])
        if (requestId !== requestIdRef.current) return
        const nextCache = {
          result: modelPage,
          hasResources: providerPage.data.some(
            (provider) => provider.models_count > 0
          ),
        }
        writePageCache(cacheKey, nextCache)
        setCached(nextCache)
        setModels(modelPage)
        setHasResources(nextCache.hasResources)
      }
    } catch (caught) {
      if (requestId !== requestIdRef.current) return
      if (readPageCache(cacheKey) === null)
        setError(caught instanceof Error ? caught.message : "无法加载设置")
    }
  }, [kind, page, query, cacheKey])

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0)
    return () => window.clearTimeout(timer)
  }, [load])

  const result = kind === "providers" ? providers : models
  const items = result?.data
  const totalPages = result
    ? Math.max(
        1,
        Math.ceil(result.pagination.total / result.pagination.per_page)
      )
    : 1
  const hasQuery = query !== ""

  const updateQuery = (nextQuery: string, nextPage = 1) => {
    const params = new URLSearchParams({ returnTo })
    if (nextQuery.trim() !== "") params.set("q", nextQuery.trim())
    if (nextPage > 1) params.set("page", String(nextPage))
    router.replace(`/studio/settings/${kind}?${params}`)
  }

  return (
    <>
      <PageHeader title="设置" backHref={returnTo}>
        <Link
          href={withStudioReturn(`/studio/settings/${kind}/new`, returnTo)}
          className={buttonVariants({ size: "lg" })}
        >
          <Plus aria-hidden />
          新建
        </Link>
      </PageHeader>
      <form
        role="search"
        className="relative mb-6 max-w-xl"
        onSubmit={(event) => {
          event.preventDefault()
          const data = new FormData(event.currentTarget)
          updateQuery(String(data.get("q") ?? ""))
        }}
      >
        <Search
          aria-hidden
          className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground"
        />
        <StudioInput
          key={query}
          name="q"
          defaultValue={query}
          aria-label={`搜索 ${kind === "providers" ? "Provider" : "Model"}`}
          placeholder={`搜索 ${kind === "providers" ? "Provider" : "Model"}`}
          className="pr-16 pl-9"
        />
        <Button type="submit" variant="ghost" className="absolute top-1 right-1">
          搜索
        </Button>
      </form>
      {error ? (
        <div className="mb-5">
          <ErrorState
            title={`${kind === "providers" ? "Provider" : "Model"} 列表加载失败`}
            message={error}
            onRetry={() => void load()}
          />
        </div>
      ) : null}
      {items === undefined && !error ? (
        <LoadingState
          label={`正在加载 ${kind === "providers" ? "Provider" : "Model"}`}
        />
      ) : items?.length === 0 ? (
        <div className="rounded-2xl border border-dashed border-border p-7 sm:p-10">
          <h2 className="font-semibold">
            {hasQuery && hasResources
              ? `没有匹配的 ${kind === "providers" ? "Provider" : "Model"}`
              : kind === "providers"
                ? "尚未配置 Provider"
                : "尚未配置 Model"}
          </h2>
          <p className="mt-2 max-w-[65ch] text-sm leading-6 text-muted-foreground">
            {hasQuery && hasResources
              ? "调整搜索词，或清除筛选查看全部资源。"
              : kind === "providers"
                ? "创建受支持的 Provider 后，可以继续配置它的 Model。"
                : "添加一个真实可用的模型名称。"}
          </p>
          {hasQuery && hasResources ? (
            <Button
              type="button"
              variant="outline"
              size="lg"
              className="mt-4"
              onClick={() => updateQuery("")}
            >
              清除筛选
            </Button>
          ) : null}
        </div>
      ) : items ? (
        <div className="grid gap-3 sm:grid-cols-2">
          {kind === "providers"
            ? (items as readonly ProviderView[]).map((provider) => (
                <ResourceCard
                  key={provider.provider}
                  href={withStudioReturn(
                    `/studio/settings/providers/${provider.provider}`,
                    returnTo
                  )}
                  title={provider.provider}
                  leading={<Plug aria-hidden className="size-5" />}
                  badge={
                    provider.credential_configured ? (
                      <StatusChip tone="ok">已配置</StatusChip>
                    ) : (
                      <StatusChip tone="warn">需要凭据</StatusChip>
                    )
                  }
                  meta={[
                    { icon: Box, text: `${provider.models_count} 个模型` },
                    {
                      icon: KeyRound,
                      text: provider.credential_configured
                        ? "凭据已配置"
                        : "未配置凭据",
                    },
                  ]}
                />
              ))
            : (items as readonly ManagedModelView[]).map((model) => (
                <ResourceCard
                  key={model.model_id}
                  href={withStudioReturn(
                    `/studio/settings/models/${encodeURIComponent(model.model_id)}`,
                    returnTo
                  )}
                  title={model.name}
                  leading={<Cpu aria-hidden className="size-5" />}
                  badge={
                    model.is_default ? (
                      <StatusChip tone="neutral">默认</StatusChip>
                    ) : undefined
                  }
                  meta={[{ icon: Plug, text: model.provider }]}
                />
              ))}
        </div>
      ) : null}
      {result ? (
        <Pagination
          page={page}
          totalPages={totalPages}
          onPageChange={(next) => updateQuery(query, next)}
          label={`${kind === "providers" ? "Provider" : "Model"} 分页`}
        />
      ) : null}
    </>
  )
}
