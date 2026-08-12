"use client"

import Link from "next/link"
import { useRouter, useSearchParams } from "next/navigation"
import { useCallback, useEffect, useState } from "react"
import { ArrowRight, Plus, Search, Wrench } from "lucide-react"

import {
  StudioHeader,
  StudioInput,
  StudioPage,
} from "@/components/stratum/studio/primitives"
import { Button, buttonVariants } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"
import { studioApi } from "@/features/studio-management/client"
import { withStudioReturn } from "@/features/studio-management/navigation"
import type { AgentDefinitionView, PageEnvelope } from "@/lib/stratum/api"
import { modelDisplayName } from "@/lib/stratum/model-config"

const PER_PAGE = 12

function safePage(value: string | null): number {
  const parsed = Number(value)
  return Number.isInteger(parsed) && parsed > 0 ? parsed : 1
}

function displayDate(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.valueOf())
    ? value
    : new Intl.DateTimeFormat("zh-CN", {
        year: "numeric",
        month: "short",
        day: "numeric",
      }).format(date)
}

function AgentCard({ agent }: { agent: AgentDefinitionView }) {
  const model = modelDisplayName(agent.model)
  return (
    <Link
      href={`/studio/agents/${encodeURIComponent(agent.agent_name)}`}
      title={agent.agent_name}
      className="group flex min-h-52 flex-col rounded-2xl border border-border bg-card p-5 transition-colors outline-none hover:bg-muted/45 focus-visible:ring-2 focus-visible:ring-ring motion-reduce:transition-none"
    >
      <div className="flex min-w-0 items-start justify-between gap-3">
        <h2 className="min-w-0 text-lg font-semibold tracking-[-0.015em] break-words">
          {agent.agent_name}
        </h2>
        <ArrowRight
          aria-hidden
          className="mt-1 size-4 shrink-0 text-muted-foreground"
        />
      </div>
      <div className="mt-7 grid gap-3 text-sm">
        <div>
          <p className="text-muted-foreground">Provider / Model</p>
          <p className="mt-1 truncate font-medium" title={agent.model}>
            {model.provider ? `${model.provider} / ` : ""}
            {model.model}
          </p>
        </div>
        <div className="flex items-center gap-2 text-muted-foreground">
          <Wrench aria-hidden className="size-4" />
          <span>{agent.tools.length} 个工具</span>
        </div>
      </div>
      <p className="mt-auto pt-5 text-xs text-muted-foreground">
        更新于 {displayDate(agent.updated_at)}
      </p>
    </Link>
  )
}

function DashboardSkeleton() {
  return (
    <div
      className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3"
      aria-label="正在加载 Agent"
    >
      {Array.from({ length: 6 }, (_, index) => (
        <div
          key={index}
          className="min-h-52 rounded-2xl border border-border bg-card p-5"
        >
          <Skeleton className="h-6 w-2/3" />
          <Skeleton className="mt-8 h-4 w-24" />
          <Skeleton className="mt-2 h-5 w-3/4" />
          <Skeleton className="mt-5 h-4 w-28" />
          <Skeleton className="mt-7 h-3 w-32" />
        </div>
      ))}
    </div>
  )
}

export function StudioDashboard() {
  const router = useRouter()
  const searchParams = useSearchParams()
  const query = searchParams.get("q") ?? ""
  const page = safePage(searchParams.get("page"))
  const [searchValue, setSearchValue] = useState(query)
  const [result, setResult] =
    useState<PageEnvelope<AgentDefinitionView> | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [requestKey, setRequestKey] = useState(0)

  const load = useCallback(async () => {
    setError(null)
    try {
      const next = await studioApi.listAgentDefinitions({
        page,
        perPage: PER_PAGE,
        sort: "-updated_at",
        search: query,
      })
      setResult(next)
    } catch (caught) {
      setError(caught instanceof Error ? caught.message : "无法加载 Agent")
    }
  }, [page, query])

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), 0)
    return () => window.clearTimeout(timer)
  }, [load, requestKey])

  const updateQuery = (nextQuery: string, nextPage = 1) => {
    const params = new URLSearchParams()
    if (nextQuery.trim() !== "") params.set("q", nextQuery.trim())
    if (nextPage > 1) params.set("page", String(nextPage))
    router.replace(params.size === 0 ? "/studio" : `/studio?${params}`)
  }

  const agents = result?.data ?? []
  const totalPages =
    result?.pagination.total_pages ??
    (result
      ? Math.max(
          1,
          Math.ceil(result.pagination.total / result.pagination.per_page)
        )
      : 1)
  const hasQuery = query.trim() !== ""
  const currentStudioPath =
    searchParams.size === 0 ? "/studio" : `/studio?${searchParams}`

  return (
    <StudioPage>
      <StudioHeader
        title="Studio"
        settings
        settingsHref={withStudioReturn(
          "/studio/settings/providers",
          currentStudioPath
        )}
      >
        <Link
          href="/studio/agents/new"
          className={buttonVariants({
            className:
              "min-h-11 rounded-xl bg-primary px-4 text-sm text-primary-foreground hover:bg-primary/90",
          })}
        >
          <Plus aria-hidden />
          <span className="hidden sm:inline">新建 Agent</span>
          <span className="sm:hidden">新建</span>
        </Link>
      </StudioHeader>

      <form
        role="search"
        className="relative mb-7 max-w-xl"
        onSubmit={(event) => {
          event.preventDefault()
          updateQuery(searchValue)
        }}
      >
        <Search
          aria-hidden
          className="pointer-events-none absolute top-1/2 left-3.5 size-4 -translate-y-1/2 text-muted-foreground"
        />
        <StudioInput
          value={searchValue}
          onChange={(event) => setSearchValue(event.target.value)}
          placeholder="搜索 Agent 名称"
          aria-label="搜索 Agent 名称"
          className="h-11 rounded-xl pr-20 pl-10"
        />
        <Button
          type="submit"
          variant="ghost"
          className="absolute top-1 right-1 h-9 rounded-lg px-3"
        >
          搜索
        </Button>
      </form>

      {result === null && error === null ? <DashboardSkeleton /> : null}

      {error ? (
        <div
          className="rounded-2xl border border-destructive/35 bg-card p-6"
          role="alert"
        >
          <p className="font-medium text-destructive">Agent 列表加载失败</p>
          <p className="mt-2 text-sm text-muted-foreground">{error}</p>
          <Button
            type="button"
            variant="outline"
            className="mt-5 min-h-11 rounded-xl"
            onClick={() => setRequestKey((value) => value + 1)}
          >
            重试
          </Button>
        </div>
      ) : null}

      {result && agents.length > 0 ? (
        <div className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
          {agents.map((agent) => (
            <AgentCard key={agent.agent_name} agent={agent} />
          ))}
        </div>
      ) : null}

      {!error && result && agents.length === 0 ? (
        <div className="flex min-h-64 flex-col items-start justify-center rounded-2xl border border-border bg-card p-7 sm:p-10">
          <h2 className="text-lg font-semibold">
            {hasQuery ? "没有匹配的 Agent" : "尚未创建 Agent"}
          </h2>
          <p className="mt-2 max-w-[65ch] text-sm leading-6 text-muted-foreground">
            {hasQuery
              ? "调整搜索词，或清除筛选查看全部 Agent。"
              : "创建第一个 Agent definition 后，它会显示在这里。"}
          </p>
          {hasQuery ? (
            <Button
              type="button"
              variant="outline"
              className="mt-5 min-h-11 rounded-xl"
              onClick={() => {
                setSearchValue("")
                updateQuery("")
              }}
            >
              清除筛选
            </Button>
          ) : (
            <Link
              href="/studio/agents/new"
              className={buttonVariants({
                className: "mt-5 min-h-11 rounded-xl px-4 text-sm",
              })}
            >
              <Plus aria-hidden />
              新建 Agent
            </Link>
          )}
        </div>
      ) : null}

      {result && totalPages > 1 ? (
        <nav
          aria-label="Agent 分页"
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
