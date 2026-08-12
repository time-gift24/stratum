"use client"

import Link from "next/link"
import { useRouter, useSearchParams } from "next/navigation"
import { useCallback, useEffect, useState } from "react"
import { Plus } from "lucide-react"

import {
  FormStatus,
  ListRow,
  SettingsNav,
  StudioHeader,
  StudioPage,
} from "@/components/stratum/studio/primitives"
import { Button, buttonVariants } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { studioApi } from "@/features/studio-management/client"
import {
  safeStudioReturn,
  withStudioReturn,
} from "@/features/studio-management/navigation"
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
  const [providers, setProviders] = useState<PageEnvelope<ProviderView> | null>(
    null
  )
  const [models, setModels] = useState<PageEnvelope<ManagedModelView> | null>(
    null
  )
  const [hasResources, setHasResources] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const load = useCallback(async () => {
    setError(null)
    try {
      if (kind === "providers") {
        const all = await studioApi.listProviders({ page: 1, perPage: 100 })
        const normalized = query.toLocaleLowerCase()
        const filtered = all.data.filter(
          (provider) =>
            normalized === "" ||
            provider.provider.toLocaleLowerCase().includes(normalized)
        )
        const start = (page - 1) * PER_PAGE
        setProviders({
          data: filtered.slice(start, start + PER_PAGE),
          pagination: {
            page,
            per_page: PER_PAGE,
            total: filtered.length,
          },
        })
        setHasResources(all.pagination.total > 0)
      } else {
        const [modelPage, providerPage] = await Promise.all([
          studioApi.listManagedModels({
            page,
            perPage: PER_PAGE,
            search: query,
          }),
          studioApi.listProviders({ page: 1, perPage: 100 }),
        ])
        setModels(modelPage)
        setHasResources(
          providerPage.data.some((provider) => provider.models_count > 0)
        )
      }
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "无法加载设置")
    }
  }, [kind, page, query])

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
    <StudioPage>
      <StudioHeader title="设置" backHref={returnTo} backLabel="返回 Studio">
        <Link
          href={withStudioReturn(`/studio/settings/${kind}/new`, returnTo)}
          className={buttonVariants({
            className: "min-h-11 rounded-xl px-4 text-sm",
          })}
        >
          <Plus aria-hidden />
          新建
        </Link>
      </StudioHeader>
      <SettingsNav current={kind} returnTo={returnTo} />
      <form
        role="search"
        className="relative mb-6 max-w-xl"
        onSubmit={(event) => {
          event.preventDefault()
          const data = new FormData(event.currentTarget)
          updateQuery(String(data.get("q") ?? ""))
        }}
      >
        <input
          key={query}
          name="q"
          defaultValue={query}
          aria-label={`搜索 ${kind === "providers" ? "Provider" : "Model"}`}
          placeholder={`搜索 ${kind === "providers" ? "Provider" : "Model"}`}
          className="h-11 w-full rounded-xl border border-border bg-card px-3 pr-20 text-sm text-foreground outline-none placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring/25"
        />
        <Button
          type="submit"
          variant="ghost"
          className="absolute top-1 right-1 h-9 rounded-lg px-3"
        >
          搜索
        </Button>
      </form>
      {error ? (
        <div className="mb-5 grid gap-3">
          <FormStatus message={error} tone="error" />
          <Button
            type="button"
            variant="outline"
            className="min-h-11 w-fit rounded-xl"
            onClick={() => void load()}
          >
            重试
          </Button>
        </div>
      ) : null}
      {items === undefined && !error ? (
        <div className="rounded-2xl border border-border bg-card p-5">
          {Array.from({ length: 4 }, (_, index) => (
            <div
              key={index}
              className="flex min-h-16 items-center border-b border-border last:border-none"
            >
              <div className="w-full">
                <Skeleton className="h-5 w-32" />
                <Skeleton className="mt-2 h-4 w-56 max-w-full" />
              </div>
            </div>
          ))}
        </div>
      ) : items?.length === 0 ? (
        <div className="rounded-2xl border border-border bg-card p-8">
          <h2 className="font-semibold">
            {hasQuery && hasResources
              ? `没有匹配的 ${kind === "providers" ? "Provider" : "Model"}`
              : kind === "providers"
                ? "尚未配置 Provider"
                : "尚未配置 Model"}
          </h2>
          <p className="mt-2 text-sm leading-6 text-muted-foreground">
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
              className="mt-5 min-h-11 rounded-xl"
              onClick={() => updateQuery("")}
            >
              清除筛选
            </Button>
          ) : null}
        </div>
      ) : items ? (
        <div className="rounded-2xl border border-border bg-card px-4 sm:px-5">
          {kind === "providers"
            ? (items as readonly ProviderView[]).map((provider) => (
                <ListRow
                  key={provider.provider}
                  href={withStudioReturn(
                    `/studio/settings/providers/${provider.provider}`,
                    returnTo
                  )}
                  title={provider.provider}
                  meta={`${provider.models_count} 个模型 · ${provider.credential_configured ? "凭据已配置" : "未配置凭据"}`}
                >
                  <span className="hidden text-sm text-muted-foreground sm:block">
                    {provider.credential_configured ? "凭据已配置" : "需要凭据"}
                  </span>
                </ListRow>
              ))
            : (items as readonly ManagedModelView[]).map((model) => (
                <ListRow
                  key={model.model_id}
                  href={withStudioReturn(
                    `/studio/settings/models/${encodeURIComponent(model.model_id)}`,
                    returnTo
                  )}
                  title={model.name}
                  meta={`${model.provider}${model.is_default ? " · 默认 Model" : ""}`}
                />
              ))}
        </div>
      ) : null}
      {result && totalPages > 1 ? (
        <nav
          aria-label={`${kind === "providers" ? "Provider" : "Model"} 分页`}
          className="mt-8 flex items-center justify-between gap-4"
        >
          <Button
            type="button"
            variant="outline"
            className="min-h-11 rounded-xl"
            disabled={page <= 1}
            onClick={() => updateQuery(query, page - 1)}
          >
            上一页
          </Button>
          <span className="text-sm text-muted-foreground">
            第 {page} / {totalPages} 页
          </span>
          <Button
            type="button"
            variant="outline"
            className="min-h-11 rounded-xl"
            disabled={page >= totalPages}
            onClick={() => updateQuery(query, page + 1)}
          >
            下一页
          </Button>
        </nav>
      ) : null}
    </StudioPage>
  )
}
