"use client"

import { useMemo } from "react"
import htmldiff from "node-htmldiff"

import { renderMarkdownToHtml } from "@/lib/markdown-html"
import { cn } from "@/lib/utils"

import styles from "../styles/prose-medium.module.css"

/**
 * MarkdownDiff 的重型视图（单独成文件：node-htmldiff + remark 管线
 * 只在这两个视图被激活时经 next/dynamic 加载，不进首屏 bundle；
 * ssr:false 也使 DOMParser 等浏览器 API 无需 SSR 守卫）。
 *
 * InlineDiffView —— 内联渲染 diff：两版各渲染成 HTML 后 htmldiff 合并，
 *   增删以 ins/del 标记内联呈现（GitHub rich diff 形态）。
 * SplitDiffView —— 并排渲染 diff：合并结果拆两半，旧版列只留 del
 *   （删除高亮）、新版列只留 ins（新增高亮）。
 * 注：均走 dangerouslySetInnerHTML，内容为调用方可信输入。
 */

function DiffPane({
  label,
  html,
  compact = true,
  className,
}: {
  label: string
  html: string
  compact?: boolean
  className?: string
}) {
  return (
    <div className={className}>
      <p className="border-b border-border px-4 py-2 text-xs text-muted-foreground">
        {label}
      </p>
      <div
        className={cn(
          styles.proseMedium,
          compact && styles.proseMediumSm,
          "px-6 py-5"
        )}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </div>
  )
}

export function InlineDiffView({
  before,
  after,
}: {
  before: string
  after: string
}) {
  const mergedHtml = useMemo(
    () => htmldiff(renderMarkdownToHtml(before), renderMarkdownToHtml(after)),
    [before, after]
  )

  return (
    <div
      className={cn(styles.proseMedium, "px-6 py-5")}
      dangerouslySetInnerHTML={{ __html: mergedHtml }}
    />
  )
}

/** 从合并 diff HTML 中剔除一类标记（保留另一类做高亮）。直接修改传入文档。 */
function stripTag(doc: Document, tag: "ins" | "del") {
  doc.querySelectorAll(tag).forEach((el) => el.remove())
}

export function SplitDiffView({
  before,
  after,
}: {
  before: string
  after: string
}) {
  const { oldHtml, newHtml } = useMemo(() => {
    const merged = htmldiff(
      renderMarkdownToHtml(before),
      renderMarkdownToHtml(after)
    )
    // 解析一次，顺序剔除：先去 ins 留旧版，再去 del 留新版
    const doc = new DOMParser().parseFromString(merged, "text/html")
    stripTag(doc, "ins")
    const oldHtml = doc.body.innerHTML
    stripTag(doc, "del")
    const newHtml = doc.body.innerHTML
    return { oldHtml, newHtml }
  }, [before, after])

  return (
    <div className="grid sm:grid-cols-2">
      <DiffPane
        label="旧版"
        html={oldHtml}
        className="border-b border-border sm:border-r sm:border-b-0"
      />
      <DiffPane label="新版" html={newHtml} />
    </div>
  )
}
