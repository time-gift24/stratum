"use client"

import { useEffect, useState, useSyncExternalStore } from "react"
import { Check, LoaderCircle, RotateCcw } from "lucide-react"
import { Streamdown } from "streamdown"

import { Button } from "@/components/ui/button"
import { cn } from "@/lib/utils"

import styles from "../styles/prose-medium.module.css"

/**
 * MarkdownStream —— AI 流式输出的 Markdown 渲染。
 * 按 chunk 模拟 token 逐个到达，Streamdown 以 mode="streaming" 边收边排，
 * 未闭合的语法（半截加粗、未完成的链接）由 remend 自动补全；
 * 生成中显示块状 caret，完成后可重新播放。
 * prefers-reduced-motion 下直接呈现全文，不播流式动画。
 */
export function MarkdownStream({
  source,
  chunkSize = 4,
  interval = 30,
  className,
}: {
  /** 完整的 markdown 源文，组件负责模拟流式到达 */
  source: string
  /** 每个 tick 追加的字符数 */
  chunkSize?: number
  /** tick 间隔（ms） */
  interval?: number
  className?: string
}) {
  const [length, setLength] = useState(0)

  // reduced-motion 用 useSyncExternalStore 订阅（SSR 恒 false，无 hydration 分叉）
  const reducedMotion = useSyncExternalStore(
    (callback) => {
      const mq = window.matchMedia("(prefers-reduced-motion: reduce)")
      mq.addEventListener("change", callback)
      return () => mq.removeEventListener("change", callback)
    },
    () => window.matchMedia("(prefers-reduced-motion: reduce)").matches,
    () => false
  )
  // 渲染期派生：reduced-motion 直接呈现全文（不写 effect）
  if (reducedMotion && length !== source.length) setLength(source.length)

  // 换源时在渲染期复位（同一派生模式）
  const [prevSource, setPrevSource] = useState(source)
  if (source !== prevSource) {
    setPrevSource(source)
    setLength(0)
  }

  const done = length >= source.length

  useEffect(() => {
    if (done || reducedMotion) return
    const timer = setInterval(() => {
      setLength((n) => Math.min(n + chunkSize, source.length))
    }, interval)
    return () => clearInterval(timer)
  }, [done, reducedMotion, chunkSize, interval, source.length])

  return (
    <div data-slot="markdown-stream" className={className}>
      <div className="flex items-center justify-between border-b border-border px-4 py-2">
        <p className="flex items-center gap-2 font-mono text-xs text-muted-foreground">
          {done ? (
            <Check aria-hidden className="size-3.5 text-primary" />
          ) : (
            <LoaderCircle
              aria-hidden
              className="size-3.5 animate-spin text-port-image"
            />
          )}
          {done ? "生成完成" : "正在生成…"}
        </p>
        <Button
          variant="ghost"
          size="sm"
          className="gap-1.5"
          onClick={() => setLength(0)}
        >
          <RotateCcw aria-hidden />
          重新播放
        </Button>
      </div>
      <div className={cn(styles.proseMedium, "p-6")}>
        <Streamdown
          mode="streaming"
          isAnimating={!done}
          animated={!done}
          caret={done ? undefined : "block"}
        >
          {source.slice(0, length)}
        </Streamdown>
      </div>
    </div>
  )
}
