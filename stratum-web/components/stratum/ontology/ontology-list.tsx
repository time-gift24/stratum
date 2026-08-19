import { AlignLeft, Clock, Hash, Plus, Trash2Icon } from "lucide-react"

import {
  ErrorState,
  LoadingState,
  PageHeader,
  Pagination,
  ResourceCard,
  SearchRow,
} from "@/components/stratum/studio/primitives"
import { Button } from "@/components/ui/button"
import { EmptyState } from "@/components/stratum/empty-state"
import type { OntologySummary } from "@/features/ontology-editor/types"
import {
  ONTOLOGY_LIST_PER_PAGE,
  type OntologyListState,
} from "@/hooks/use-ontology-list"

/**
 * Ontology 列表（数据经 props 下发，见 app/(site)/ontologies/page.tsx）。
 * 与仪表盘/设置列表共享 ResourceCard 扫读语言：squircle 字母标识 +
 * 虚线分隔的 mono meta 行；四态齐全（骨架 / 错误 / 空态 / 分页网格）。
 * 搜索（SearchRow，匹配 name/display_name）+ 图标化新建入口在列表上方；
 * 点击卡片进入画布编辑器 /ontologies/[id]，删除入口在卡片右侧。
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
  /** 当前生效的搜索词（用于空态文案与搜索框回显） */
  query: string
  onPageChange(page: number): void
  onRetry(): void
  onSearch(query: string): void
  onRequestCreate(): void
  onRequestDelete(ontology: OntologySummary): void
}

export function OntologyList({
  state,
  query,
  onPageChange,
  onRetry,
  onSearch,
  onRequestCreate,
  onRequestDelete,
}: OntologyListProps) {
  const hasQuery = query.trim() !== ""
  return (
    <>
      <PageHeader title="本体" />

      <SearchRow
        defaultValue={query}
        placeholder="搜索本体名称"
        onSearch={onSearch}
        action={
          <Button
            size="icon-lg"
            className="size-11 rounded-lg"
            aria-label="新建本体"
            title="新建本体"
            onClick={onRequestCreate}
          >
            <Plus aria-hidden />
          </Button>
        }
      />

      {state.phase === "loading" ? (
        <LoadingState label="正在加载本体列表" />
      ) : state.phase === "error" ? (
        <ErrorState
          title="本体列表加载失败"
          message={state.message}
          onRetry={onRetry}
        />
      ) : state.result.data.length === 0 &&
        state.result.pagination.total === 0 ? (
        <EmptyState
          title={hasQuery ? "没有匹配的本体" : "尚未创建本体"}
          description={
            hasQuery
              ? "调整搜索词，或清除筛选查看全部本体。"
              : "新建一个本体，开始定义对象类型与关系。"
          }
        >
          {hasQuery ? (
            <Button
              type="button"
              variant="outline"
              size="lg"
              className="min-h-11"
              onClick={() => onSearch("")}
            >
              清除筛选
            </Button>
          ) : (
            <Button size="lg" className="min-h-11" onClick={onRequestCreate}>
              <Plus aria-hidden />
              新建本体
            </Button>
          )}
        </EmptyState>
      ) : (
        <>
          <div className="grid gap-3 sm:grid-cols-2">
            {state.result.data.map((ontology) => (
              <ResourceCard
                key={ontology.id}
                href={`/ontologies/${ontology.id}`}
                title={ontology.display_name}
                leading={(ontology.display_name[0] ?? "?").toUpperCase()}
                meta={[
                  { icon: Hash, text: ontology.name },
                  {
                    icon: AlignLeft,
                    text: ontology.description ?? "无描述",
                  },
                  {
                    icon: Clock,
                    text: `更新于 ${formatUpdatedAt(ontology.updated_at)}`,
                  },
                ]}
                action={
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`删除 ${ontology.display_name}`}
                    onClick={() => onRequestDelete(ontology)}
                    className="text-muted-foreground hover:text-destructive"
                  >
                    <Trash2Icon />
                  </Button>
                }
              />
            ))}
          </div>
          <Pagination
            page={state.page}
            totalPages={Math.max(
              1,
              Math.ceil(state.result.pagination.total / ONTOLOGY_LIST_PER_PAGE)
            )}
            onPageChange={onPageChange}
            label="分页"
            summary={`共 ${state.result.pagination.total} 项 · `}
          />
        </>
      )}
    </>
  )
}
