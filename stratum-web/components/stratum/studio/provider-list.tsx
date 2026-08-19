"use client"

import Link from "next/link"
import { useRouter, useSearchParams } from "next/navigation"
import { useCallback, useEffect, useRef, useState } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"
import { Box, KeyRound, Plug, Plus, Search } from "lucide-react"

import {
  ErrorState,
  LoadingState,
  PageHeader,
  Pagination,
  ResourceCard,
  StatusChip,
  StudioInput,
  useDelayedFlag,
} from "@/components/stratum/studio/primitives"
import { Button, buttonVariants } from "@/components/ui/button"
import { EmptyState } from "@/components/stratum/empty-state"
import { safeStudioErrorMessage } from "@/features/studio-management/client"
import {
  fetchProvidersPage,
  type ProvidersPageData,
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
  prefersReducedMotion,
} from "@/lib/motion"
import type { PageEnvelope, ProviderView } from "@/lib/stratum/api"

gsap.registerPlugin(useGSAP)

function safePage(value: string | null): number {
  const parsed = Number(value)
  return Number.isInteger(parsed) && parsed > 0 ? parsed : 1
}

export function ProviderList() {
  const router = useRouter()
  const searchParams = useSearchParams()
  const query = searchParams.get("q")?.trim() ?? ""
  const page = safePage(searchParams.get("page"))
  const returnTo = safeStudioReturn(searchParams.get("returnTo"))
  const cacheKey = `studio-settings:providers:${page}:${query}`
  const [cached, setCached] = useState(() =>
    readPageCache<ProvidersPageData>(cacheKey)
  )
  const [result, setResult] = useState<PageEnvelope<ProviderView> | null>(
    () => cached?.result ?? null
  )
  const [hasResources, setHasResources] = useState(
    () => cached?.hasResources ?? false
  )
  const [error, setError] = useState<string | null>(null)
  const requestIdRef = useRef(0)

  // 参数变化时同步切缓存，不等 effect
  const [seenKey, setSeenKey] = useState(cacheKey)
  if (seenKey !== cacheKey) {
    setSeenKey(cacheKey)
    const next = readPageCache<ProvidersPageData>(cacheKey)
    setCached(next)
    setResult(next?.result ?? null)
    setHasResources(next?.hasResources ?? false)
    setError(null)
  }

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current
    setError(null)
    try {
      const nextCache = await fetchProvidersPage(page, query)
      if (requestId !== requestIdRef.current) return
      writePageCache(cacheKey, nextCache)
      setCached(nextCache)
      setResult(nextCache.result)
      setHasResources(nextCache.hasResources)
    } catch (caught) {
      if (requestId !== requestIdRef.current) return
      setError(safeStudioErrorMessage(caught, "无法加载设置"))
    }
  }, [page, query, cacheKey])

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0)
    return () => window.clearTimeout(timer)
  }, [load])

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
    router.replace(`/studio/settings/providers?${params}`)
  }, [pageOutOfRange, query, returnTo, router, totalPages])

  // 单层入场契约：导航（pathname 变化）时入场由容器淡入/整页转场独占，
  // 卡片不叠加；级联只在无导航的参数变化（搜索/翻页）后数据到达时播放。
  // 后台刷新替换（defined → defined）永不播。ref 读写都在 effect 内。
  const gridRef = useRef<HTMLDivElement>(null)
  const prevItemsRef = useRef(items)
  const prevKeyRef = useRef(cacheKey)
  const paramArrivalPendingRef = useRef(false)
  useGSAP(
    () => {
      const keyChanged = prevKeyRef.current !== cacheKey
      prevKeyRef.current = cacheKey
      if (keyChanged) paramArrivalPendingRef.current = true
      const arrived = prevItemsRef.current === undefined && items !== undefined
      prevItemsRef.current = items
      if (!arrived) return
      const byParamChange = paramArrivalPendingRef.current
      paramArrivalPendingRef.current = false
      const grid = gridRef.current
      if (!byParamChange || !grid || prefersReducedMotion()) return
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
    { scope: gridRef, dependencies: [items, cacheKey] }
  )

  // 加载指示延迟 150ms：本地接口常在 10ms 内返回，不闪一帧 spinner
  const showLoading = useDelayedFlag(items === undefined && !error)

  const updateQuery = (nextQuery: string, nextPage = 1) => {
    const params = new URLSearchParams({ returnTo })
    if (nextQuery.trim() !== "") params.set("q", nextQuery.trim())
    if (nextPage > 1) params.set("page", String(nextPage))
    router.replace(`/studio/settings/providers?${params}`)
  }

  return (
    <>
      <PageHeader title="设置" backHref={returnTo}>
        <Link
          href={withStudioReturn(`/studio/settings/providers/new`, returnTo)}
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
          aria-label="搜索 Provider"
          placeholder="搜索 Provider"
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
            title="Provider 列表加载失败"
            message={error}
            onRetry={() => void load()}
          />
        </div>
      ) : null}
      {items === undefined && !error ? (
        showLoading ? <LoadingState label="正在加载 Provider" /> : null
      ) : items?.length === 0 && !pageOutOfRange ? (
        <EmptyState
          title={
            hasQuery && hasResources ? "没有匹配的 Provider" : "尚未配置 Provider"
          }
          description={
            hasQuery && hasResources
              ? "调整搜索词，或清除筛选查看全部资源。"
              : "创建受支持的 Provider 后，可以继续配置它的 Model。"
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
          {items.map((provider) => (
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
          ))}
        </div>
      ) : null}
      {result ? (
        <Pagination
          page={page}
          totalPages={totalPages}
          onPageChange={(next) => updateQuery(query, next)}
          label="Provider 分页"
        />
      ) : null}
    </>
  )
}
