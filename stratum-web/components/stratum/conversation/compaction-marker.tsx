"use client"

import { memo, useRef, useState } from "react"
import { ChevronDown, FoldVertical } from "lucide-react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import { MOTION_DURATION, MOTION_EASE, motionDuration } from "@/lib/motion"
import { cn } from "@/lib/utils"

gsap.registerPlugin(useGSAP)

/** 展开态限高（超出内部滚动） */
const EXPANDED_MAX_HEIGHT = "12rem"

/**
 * CompactionMarker —— TranscriptCompacted 的可折叠"上下文已压缩"标记。
 * 不伪装成 system 消息：独立的居中 marker 行，展开显示完整 summary
 * （原消息永久保留，向上滚动分页可见）。
 *
 * 动画与 reasoning.tsx 同语言：外层容器 GSAP 高度手风琴 + autoAlpha，
 * 目标高度量 scrollHeight（限高封顶），overwrite 可打断，完成后
 * clearProps 交还 CSS；prefers-reduced-motion 全程瞬时。
 */
export const CompactionMarker = memo(function CompactionMarker({
  summary,
  className,
}: {
  summary: string
  className?: string
}) {
  const [open, setOpen] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const innerRef = useRef<HTMLDivElement>(null)

  useGSAP(
    () => {
      const container = containerRef.current
      const inner = innerRef.current
      if (!container || !inner) return

      const rem =
        parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
      const target = open
        ? Math.min(inner.scrollHeight, parseFloat(EXPANDED_MAX_HEIGHT) * rem)
        : 0

      gsap.killTweensOf(container)
      gsap.fromTo(
        container,
        {
          height: container.getBoundingClientRect().height,
          autoAlpha: Number.parseFloat(getComputedStyle(container).opacity) || 0,
        },
        {
          height: target,
          autoAlpha: open ? 1 : 0,
          duration: motionDuration(MOTION_DURATION.base),
          ease: MOTION_EASE.enter,
          overwrite: "auto",
          onComplete: () => {
            if (open) gsap.set(container, { clearProps: "height,opacity,visibility" })
            else gsap.set(container, { height: 0, autoAlpha: 0 })
          },
        }
      )
    },
    { scope: rootRef, dependencies: [open] }
  )

  if (summary.trim() === "") return null

  return (
    <div
      ref={rootRef}
      data-slot="compaction-marker"
      className={cn("flex flex-col items-center px-2", className)}
    >
      <button
        type="button"
        aria-expanded={open}
        onClick={() => setOpen((value) => !value)}
        className={cn(
          "flex items-center gap-1.5 rounded-full border border-border bg-muted/40 px-3 py-1 text-xs text-muted-foreground outline-none transition-colors",
          "hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
        )}
      >
        <FoldVertical aria-hidden className="size-3.5 shrink-0" />
        上下文已压缩
        <ChevronDown
          aria-hidden
          className={cn(
            "size-3.5 shrink-0 transition-transform duration-200 motion-reduce:transition-none",
            open && "rotate-180"
          )}
        />
      </button>

      {/* 动画容器：常驻挂载，高度/autoAlpha 由 GSAP 驱动；初始收起 */}
      <div
        ref={containerRef}
        aria-hidden={!open}
        style={{ height: 0, opacity: 0, visibility: "hidden" }}
        className="mt-2 w-full overflow-hidden"
      >
        <div
          ref={innerRef}
          style={{ maxHeight: EXPANDED_MAX_HEIGHT }}
          className="overflow-y-auto rounded-lg border border-border bg-muted/40 px-3 py-2"
        >
          <p className="text-xs font-medium text-muted-foreground">
            此前上下文的压缩摘要
          </p>
          <p className="mt-1 wrap-break-word whitespace-pre-wrap text-sm leading-6 text-foreground/90">
            {summary}
          </p>
        </div>
      </div>
    </div>
  )
})
