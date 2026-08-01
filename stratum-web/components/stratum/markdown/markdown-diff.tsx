"use client"

import { useMemo, useState } from "react"
import { diffWordsWithSpace, type ChangeObject } from "diff"
import { Columns2, FileDiff, Merge } from "lucide-react"
import dynamic from "next/dynamic"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

/**
 * MarkdownDiff —— Markdown 版本对比，三种视图：
 * 「原文 diff」：diff 库词级对比，等宽字体保留源文结构；
 * 「内联渲染」：单文档内联看增删（GitHub rich diff 形态）；
 * 「并排渲染」：旧版列只留删除高亮、新版列只留新增高亮。
 * 新增片段染 primary 绿、删除片段染 destructive 红并加删除线。
 * 后两个视图依赖 node-htmldiff + remark 管线，next/dynamic 按需加载。
 */

// 重型视图按需加载（bundle-conditional）：依赖见 markdown-diff-views.tsx
const viewLoading = () => (
  <div className="px-6 py-5 text-sm text-muted-foreground">渲染中…</div>
)
const InlineDiffView = dynamic(
  () =>
    import("./markdown-diff-views").then((m) => ({
      default: m.InlineDiffView,
    })),
  { ssr: false, loading: viewLoading }
)
const SplitDiffView = dynamic(
  () =>
    import("./markdown-diff-views").then((m) => ({
      default: m.SplitDiffView,
    })),
  { ssr: false, loading: viewLoading }
)

const VIEWS = [
  { key: "source", label: "原文 diff", icon: FileDiff },
  { key: "inline", label: "内联渲染", icon: Merge },
  { key: "split", label: "并排渲染", icon: Columns2 },
] as const

type ViewKey = (typeof VIEWS)[number]["key"]

function DiffViewToggle({
  view,
  onChange,
}: {
  view: ViewKey
  onChange: (view: ViewKey) => void
}) {
  return (
    <div className="flex items-center rounded-lg border border-border p-0.5">
      {VIEWS.map(({ key, label, icon: Icon }) => (
        <Button
          key={key}
          variant={view === key ? "secondary" : "ghost"}
          size="sm"
          className="gap-1.5"
          onClick={() => onChange(key)}
        >
          <Icon aria-hidden />
          {label}
        </Button>
      ))}
    </div>
  )
}

function DiffLegend({ added, removed }: { added: number; removed: number }) {
  return (
    <p className="flex items-center gap-3 font-mono text-xs text-muted-foreground">
      <span className="flex items-center gap-1.5">
        <span className="size-2 rounded-full bg-primary" />
        新增 {added} 处
      </span>
      <span className="flex items-center gap-1.5">
        <span className="size-2 rounded-full bg-destructive" />
        删除 {removed} 处
      </span>
    </p>
  )
}

function SourceDiffView({ parts }: { parts: ChangeObject<string>[] }) {
  return (
    <pre className="px-6 py-5 font-mono text-sm leading-relaxed whitespace-pre-wrap">
      {parts.map((part, index) => (
        <span
          key={index}
          className={cn(
            part.added && "box-decoration-clone rounded-sm bg-primary/20 px-0.5",
            part.removed &&
              "box-decoration-clone rounded-sm bg-destructive/15 px-0.5 text-muted-foreground line-through"
          )}
        >
          {part.value}
        </span>
      ))}
    </pre>
  )
}

export function MarkdownDiff({
  before,
  after,
  className,
}: {
  /** 旧版 markdown 源文 */
  before: string
  /** 新版 markdown 源文 */
  after: string
  className?: string
}) {
  const [view, setView] = useState<ViewKey>("source")

  // 词级 diff 只在 before/after 变化时重算（rerender-memo）；
  // 增删统计并入同一趟遍历
  const { parts, added, removed } = useMemo(() => {
    const parts = diffWordsWithSpace(before, after)
    let added = 0
    let removed = 0
    for (const part of parts) {
      if (part.added) added++
      if (part.removed) removed++
    }
    return { parts, added, removed }
  }, [before, after])

  return (
    <div data-slot="markdown-diff" className={className}>
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-border px-4 py-2">
        <DiffViewToggle view={view} onChange={setView} />
        <DiffLegend added={added} removed={removed} />
      </div>

      {view === "source" ? (
        <SourceDiffView parts={parts} />
      ) : view === "inline" ? (
        <InlineDiffView before={before} after={after} />
      ) : (
        <SplitDiffView before={before} after={after} />
      )}
    </div>
  )
}
