import Link from "next/link"
import { Trash2Icon } from "lucide-react"

import type { OntologySummary } from "@/features/ontology-editor/types"
import {
  ONTOLOGY_LIST_PER_PAGE,
  type OntologyListState,
} from "@/hooks/use-ontology-list"
import { Button } from "@/components/ui/button"
import { Skeleton } from "@/components/ui/skeleton"

/**
 * Ontology 列表（数据经 props 下发，见 app/(site)/ontologies/page.tsx）。
 * 覆盖契约四态：loading（骨架行）、error（重试入口）、empty（新建入口，
 * 无虚构数据）、ready（分页表格）。点击行进入画布编辑器 /ontologies/[id]。
 */

const updatedAtFormatter = new Intl.DateTimeFormat("zh-CN", {
  dateStyle: "medium",
  timeStyle: "short",
})

function formatUpdatedAt(iso: string): string {
  const time = Date.parse(iso)
  if (Number.isNaN(time)) return iso
  return updatedAtFormatter.format(time)
}

export type OntologyListProps = {
  state: OntologyListState
  onPageChange(page: number): void
  onRetry(): void
  onRequestCreate(): void
  onRequestDelete(ontology: OntologySummary): void
}

export function OntologyList({
  state,
  onPageChange,
  onRetry,
  onRequestCreate,
  onRequestDelete,
}: OntologyListProps) {
  return (
    <div className="flex flex-col gap-6">
      <header className="flex items-center justify-between gap-4">
        <h1 className="font-heading text-xl tracking-tight">本体</h1>
        <Button onClick={onRequestCreate}>新建本体</Button>
      </header>

      {state.phase === "loading" ? (
        <div aria-busy="true" className="flex flex-col gap-2">
          <span className="sr-only">正在加载本体列表…</span>
          {Array.from({ length: 5 }, (_, index) => (
            <Skeleton key={index} className="h-16 w-full rounded-xl" />
          ))}
        </div>
      ) : state.phase === "error" ? (
        <div
          role="alert"
          className="flex flex-col items-start gap-3 rounded-xl border border-destructive/40 p-4"
        >
          <p className="text-sm text-destructive">
            列表加载失败：{state.message}
          </p>
          <Button variant="outline" onClick={onRetry}>
            重试
          </Button>
        </div>
      ) : state.result.data.length === 0 &&
        state.result.pagination.total === 0 ? (
        <div className="flex flex-col items-start gap-3 rounded-xl border border-dashed border-border p-6">
          <p className="text-sm text-muted-foreground">
            还没有本体。新建一个本体，开始定义对象类型与关系。
          </p>
          <Button variant="outline" onClick={onRequestCreate}>
            新建本体
          </Button>
        </div>
      ) : (
        <>
          <ul className="flex flex-col gap-2">
            {state.result.data.map((ontology) => (
              <li key={ontology.id}>
                <div className="group flex items-center gap-2 rounded-xl border border-border bg-card transition-colors hover:border-foreground/20">
                  <Link
                    href={`/ontologies/${ontology.id}`}
                    className="min-w-0 flex-1 rounded-xl px-4 py-3 outline-none focus-visible:ring-2 focus-visible:ring-ring/30"
                  >
                    <span className="flex items-baseline gap-2">
                      <span className="truncate text-sm font-medium">
                        {ontology.display_name}
                      </span>
                      <span className="truncate font-mono text-xs text-muted-foreground">
                        {ontology.name}
                      </span>
                    </span>
                    {ontology.description ? (
                      <span className="mt-1 line-clamp-1 block text-xs text-muted-foreground">
                        {ontology.description}
                      </span>
                    ) : null}
                    <span className="mt-1 block text-xs text-muted-foreground">
                      更新于 {formatUpdatedAt(ontology.updated_at)}
                    </span>
                  </Link>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`删除 ${ontology.display_name}`}
                    onClick={() => onRequestDelete(ontology)}
                    className="mr-3 text-muted-foreground hover:text-destructive"
                  >
                    <Trash2Icon />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
          <OntologyListPagination
            page={state.page}
            total={state.result.pagination.total}
            onPageChange={onPageChange}
          />
        </>
      )}
    </div>
  )
}

function OntologyListPagination({
  page,
  total,
  onPageChange,
}: {
  page: number
  total: number
  onPageChange(page: number): void
}) {
  const totalPages = Math.max(1, Math.ceil(total / ONTOLOGY_LIST_PER_PAGE))
  return (
    <nav
      aria-label="分页"
      className="flex items-center justify-between gap-4 text-xs text-muted-foreground"
    >
      <span>
        共 {total} 项 · 第 {page} / {totalPages} 页
      </span>
      <span className="flex gap-2">
        <Button
          variant="outline"
          disabled={page <= 1}
          onClick={() => onPageChange(page - 1)}
        >
          上一页
        </Button>
        <Button
          variant="outline"
          disabled={page >= totalPages}
          onClick={() => onPageChange(page + 1)}
        >
          下一页
        </Button>
      </span>
    </nav>
  )
}
