"use client"

import Link from "next/link"
import { useRouter, useSearchParams } from "next/navigation"
import { useCallback, useEffect, useRef, useState } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import { Box, Cpu, KeyRound, Plug, Plus, Search } from "lucide-react"

import {
  ErrorState,
  PageHeader,
  Pagination,
  ResourceCard,
  ResourceGridSkeleton,
  StatusChip,
  StudioInput,
} from "@/components/stratum/studio/primitives"
import { Button, buttonVariants } from "@/components/ui/button"
import { EmptyState } from "@/components/stratum/empty-state"
import { safeStudioErrorMessage } from "@/features/studio-management/client"
import {
  fetchSettingsPage,
  type SettingsKind,
  type SettingsPageData,
} from "@/features/studio-management/settings-data"
import {
  safeStudioReturn,
  withStudioReturn,
} from "@/features/studio-management/navigation"
import { readPageCache, writePageCache } from "@/lib/page-cache"
import {
  MOTION_DURATION,
  MOTION_EASE,
  motionDuration,
  shouldAnimateChoreographedMotion,
} from "@/lib/motion"
import type {
  ManagedModelSummary,
  PageEnvelope,
  ProviderView,
} from "@/lib/stratum/api"

gsap.registerPlugin(useGSAP)

function safePage(value: string | null): number {
  const parsed = Number(value)
  return Number.isInteger(parsed) && parsed > 0 ? parsed : 1
}

export function SettingsList({ kind }: { kind: SettingsKind }) {
  const router = useRouter()
  const searchParams = useSearchParams()
  const query = searchParams.get("q")?.trim() ?? ""
  const page = safePage(searchParams.get("page"))
  const returnTo = safeStudioReturn(searchParams.get("returnTo"))
  const cacheKey = `studio-settings:${kind}:${page}:${query}`
  const [cached, setCached] = useState(() =>
    readPageCache<SettingsPageData>(cacheKey)
  )
  const [providers, setProviders] = useState<PageEnvelope<ProviderView> | null>(
    () => (cached?.result as PageEnvelope<ProviderView> | undefined) ?? null
  )
  const [models, setModels] =
    useState<PageEnvelope<ManagedModelSummary> | null>(
      () =>
        (cached?.result as PageEnvelope<ManagedModelSummary> | undefined) ??
        null
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
    const next = readPageCache<SettingsPageData>(cacheKey)
    setCached(next)
    if (kind === "providers") {
      setProviders(
        (next?.result as PageEnvelope<ProviderView> | undefined) ?? null
      )
      setModels(null)
    } else {
      setModels(
        (next?.result as PageEnvelope<ManagedModelSummary> | undefined) ?? null
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
      const nextCache = await fetchSettingsPage(kind, page, query)
      if (requestId !== requestIdRef.current) return
      writePageCache(cacheKey, nextCache)
      setCached(nextCache)
      if (kind === "providers") {
        setProviders(nextCache.result as PageEnvelope<ProviderView>)
      } else {
        setModels(nextCache.result as PageEnvelope<ManagedModelSummary>)
      }
      setHasResources(nextCache.hasResources)
    } catch (caught) {
      if (requestId !== requestIdRef.current) return
      setError(safeStudioErrorMessage(caught, "无法加载设置"))
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
  const pageOutOfRange =
    result !== null && result.pagination.total > 0 && page > totalPages

  useEffect(() => {
    if (!pageOutOfRange) return
    const params = new URLSearchParams({ returnTo })
    if (query !== "") params.set("q", query)
    if (totalPages > 1) params.set("page", String(totalPages))
    router.replace(`/studio/settings/${kind}?${params}`)
  }, [kind, pageOutOfRange, query, returnTo, router, totalPages])

  // 加载态 → 数据到达时卡片级联入场；挂载即有数据（缓存/悬停预取命中）
  // 和后台刷新替换都不播——前者由容器淡入覆盖，后者避免刷新闪动
  const gridRef = useRef<HTMLDivElement>(null)
  const prevItemsRef = useRef(items)
  useGSAP(
    () => {
      const arrived = prevItemsRef.current === undefined && items !== undefined
      prevItemsRef.current = items
      const grid = gridRef.current
      if (!arrived || !grid || !shouldAnimateChoreographedMotion()) return
      gsap.fromTo(
        grid.children,
        { opacity: 0, y: 10 },
        {
          opacity: 1,
          y: 0,
          duration: motionDuration(MOTION_DURATION.fast),
          ease: MOTION_EASE.enter,
          stagger: 0.04,
          overwrite: "auto",
          clearProps: "transform,opacity",
        }
      )
    },
    { scope: gridRef, dependencies: [items] }
  )

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
          className={buttonVariants({ size: "lg", className: "min-h-11 px-4" })}
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
        <Button
          type="submit"
          variant="ghost"
          className="absolute top-0 right-0 min-h-11 px-3 sm:top-1 sm:right-1 sm:min-h-7"
        >
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
        <ResourceGridSkeleton
          label={`正在加载 ${kind === "providers" ? "Provider" : "Model"}`}
          metaRows={2}
        />
      ) : items?.length === 0 && !pageOutOfRange ? (
        <EmptyState
          title={
            hasQuery && hasResources
              ? `没有匹配的 ${kind === "providers" ? "Provider" : "Model"}`
              : kind === "providers"
                ? "尚未配置 Provider"
                : "尚未配置 Model"
          }
          description={
            hasQuery && hasResources
              ? "调整搜索词，或清除筛选查看全部资源。"
              : kind === "providers"
                ? "创建受支持的 Provider 后，可以继续配置它的 Model。"
                : "添加一个真实可用的模型名称。"
          }
        >
          {hasQuery && hasResources ? (
            <Button
              type="button"
              variant="outline"
              size="lg"
              className="min-h-11"
              onClick={() => updateQuery("")}
            >
              清除筛选
            </Button>
          ) : null}
        </EmptyState>
      ) : items ? (
        <div ref={gridRef} className="grid gap-3 sm:grid-cols-2">
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
            : (items as readonly ManagedModelSummary[]).map((model) => (
                <ResourceCard
                  key={model.model_id}
                  href={withStudioReturn(
                    `/studio/settings/models/${encodeURIComponent(model.model_id)}`,
                    returnTo
                  )}
                  title={model.name}
                  leading={<Cpu aria-hidden className="size-5" />}
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
