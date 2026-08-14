"use client"

/**
 * THESIS: 仪表盘是 Agent definition 的资产台账——每张卡片是一个可进入的
 * 资源，扫读优先于展示；拒绝营销页式的大 hero 与装饰卡片。
 * OWN-WORLD: 暖纸/石墨双主题 token；squircle 字母标识 + 真实状态 chip +
 * 虚线分隔的 mono meta 行；圆角卡片 + hairline，无阴影堆叠。
 * STORY: 进入即见资产列表（名称、模型、工具数、更新时间），搜索过滤或
 * 新建，点卡片进入编辑器。
 * FIRST VIEWPORT: 页头（标题 + 新建）→ 搜索框 → 两列资源卡片网格，
 * 首屏全部是真实资产。
 * FORM: 用户指定的仪表盘卡片布局（Operate mode），无 concept roll。
 */

import Link from "next/link"
import { useRouter, useSearchParams } from "next/navigation"
import { useCallback, useEffect, useRef, useState } from "react"
import { Clock, Cpu, Plus, Search, Wrench } from "lucide-react"

import {
  ErrorState,
  LoadingState,
  PageHeader,
  PageShell,
  Pagination,
  ResourceCard,
  StudioInput,
} from "@/components/stratum/studio/primitives"
import { Button, buttonVariants } from "@/components/ui/button"
import { studioApi } from "@/features/studio-management/client"
import {
  readPageCache,
  writePageCache,
} from "@/lib/page-cache"
import type { AgentDefinitionView, PageEnvelope } from "@/lib/stratum/api"
import { modelDisplayName } from "@/lib/stratum/model-config"

const PER_PAGE = 12

const dateFormatter = new Intl.DateTimeFormat("zh-CN", {
  year: "numeric",
  month: "short",
  day: "numeric",
})

function safePage(value: string | null): number {
  const parsed = Number(value)
  return Number.isInteger(parsed) && parsed > 0 ? parsed : 1
}

function displayDate(value: string): string {
  const date = new Date(value)
  return Number.isNaN(date.valueOf()) ? value : dateFormatter.format(date)
}

function AgentCard({ agent }: { agent: AgentDefinitionView }) {
  const model = modelDisplayName(agent.model)
  return (
    <ResourceCard
      href={`/studio/agents/${encodeURIComponent(agent.agent_name)}`}
      title={agent.agent_name}
      leading={(agent.agent_name[0] ?? "?").toUpperCase()}
      meta={[
        {
          icon: Cpu,
          text: model.provider ? `${model.provider} / ${model.model}` : model.model,
        },
        { icon: Wrench, text: `${agent.tools.length} 个工具` },
        { icon: Clock, text: `更新于 ${displayDate(agent.updated_at)}` },
      ]}
    />
  )
}

export function StudioDashboard() {
  const router = useRouter()
  const searchParams = useSearchParams()
  const query = searchParams.get("q") ?? ""
  const page = safePage(searchParams.get("page"))
  const cacheKey = `studio-agents:${page}:${query}`
  const [result, setResult] = useState(() =>
    readPageCache<PageEnvelope<AgentDefinitionView>>(cacheKey)
  )
  const [error, setError] = useState<string | null>(null)
  const [requestKey, setRequestKey] = useState(0)
  const requestIdRef = useRef(0)

  // 参数变化（翻页/搜索/回退）时同步切到对应缓存，不等 effect
  const [seenKey, setSeenKey] = useState(cacheKey)
  if (seenKey !== cacheKey) {
    setSeenKey(cacheKey)
    setResult(readPageCache(cacheKey))
    setError(null)
  }

  const load = useCallback(async () => {
    const requestId = ++requestIdRef.current
    setError(null)
    try {
      const next = await studioApi.listAgentDefinitions({
        page,
        perPage: PER_PAGE,
        sort: "-updated_at",
        search: query,
      })
      // 只接受最后一次发起的请求，避免乱序响应覆盖新状态
      if (requestId !== requestIdRef.current) return
      writePageCache(cacheKey, next)
      setResult(next)
    } catch (caught) {
      if (requestId !== requestIdRef.current) return
      // 有缓存内容可展示时不刷新错误面板
      if (readPageCache(cacheKey) === null)
        setError(caught instanceof Error ? caught.message : "无法加载 Agent")
    }
  }, [page, query, cacheKey])

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

  return (
    <PageShell>
      <PageHeader title="仪表盘">
        <Link
          href="/studio/agents/new"
          className={buttonVariants({ size: "lg" })}
        >
          <Plus aria-hidden />
          <span className="hidden sm:inline">新建 Agent</span>
          <span className="sm:hidden">新建</span>
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
          placeholder="搜索 Agent 名称"
          aria-label="搜索 Agent 名称"
          className="pr-16 pl-9"
        />
        <Button
          type="submit"
          variant="ghost"
          className="absolute top-1 right-1"
        >
          搜索
        </Button>
      </form>

      {result === null && error === null ? (
        <LoadingState label="正在加载 Agent" />
      ) : null}

      {error ? (
        <ErrorState
          title="Agent 列表加载失败"
          message={error}
          onRetry={() => setRequestKey((value) => value + 1)}
        />
      ) : null}

      {result && agents.length > 0 ? (
        <div className="grid gap-3 sm:grid-cols-2">
          {agents.map((agent) => (
            <AgentCard key={agent.agent_name} agent={agent} />
          ))}
        </div>
      ) : null}

      {!error && result && agents.length === 0 ? (
        <div className="rounded-2xl border border-dashed border-border p-7 sm:p-10">
          <h2 className="font-semibold">
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
              size="lg"
              className="mt-4"
              onClick={() => updateQuery("")}
            >
              清除筛选
            </Button>
          ) : (
            <Link
              href="/studio/agents/new"
              className={buttonVariants({ size: "lg", className: "mt-4" })}
            >
              <Plus aria-hidden />
              新建 Agent
            </Link>
          )}
        </div>
      ) : null}

      {result ? (
        <Pagination
          page={page}
          totalPages={totalPages}
          onPageChange={(next) => updateQuery(query, next)}
          label="Agent 分页"
        />
      ) : null}
    </PageShell>
  )
}
