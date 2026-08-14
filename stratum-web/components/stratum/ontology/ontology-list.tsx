import { AlignLeft, Clock, Hash, Plus, Trash2Icon } from "lucide-react"

import {
  ErrorState,
  LoadingState,
  PageHeader,
  Pagination,
  ResourceCard,
} from "@/components/stratum/studio/primitives"
import { Button } from "@/components/ui/button"
import type { OntologySummary } from "@/features/ontology-editor/types"
import {
  ONTOLOGY_LIST_PER_PAGE,
  type OntologyListState,
} from "@/hooks/use-ontology-list"

/**
 * Ontology 列表（数据经 props 下发，见 app/(site)/ontologies/page.tsx）。
 * 与仪表盘/设置列表共享 ResourceCard 扫读语言：squircle 字母标识 +
 * 虚线分隔的 mono meta 行；四态齐全（骨架 / 错误 / 空态 / 分页网格）。
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
    <>
      <PageHeader title="本体">
        <Button size="lg" onClick={onRequestCreate}>
          <Plus aria-hidden />
          新建本体
        </Button>
      </PageHeader>

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
        <div className="rounded-2xl border border-dashed border-border p-7 sm:p-10">
          <h2 className="font-semibold">尚未创建本体</h2>
          <p className="mt-2 max-w-[65ch] text-sm leading-6 text-muted-foreground">
            新建一个本体，开始定义对象类型与关系。
          </p>
          <Button size="lg" className="mt-4" onClick={onRequestCreate}>
            <Plus aria-hidden />
            新建本体
          </Button>
        </div>
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
