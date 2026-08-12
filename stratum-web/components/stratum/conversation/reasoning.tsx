"use client"

import { memo, useEffect, useRef, useState } from "react"
import { Brain, ChevronDown, ChevronsDownUp, ChevronsUpDown } from "lucide-react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import { useSmoothText } from "@/hooks/use-smooth-text"
import { MOTION_DURATION, MOTION_EASE, motionDuration } from "@/lib/motion"
import { cn } from "@/lib/utils"

gsap.registerPlugin(useGSAP)

/**
 * Reasoning —— 助手消息的思考过程展示（assistant-ui reasoning 底稿的数据驱动
 * fork，不用它的 runtime/MarkdownText）。ghost 变体：安静地待在消息正文上方。
 *
 * 三层状态机：
 * - 折叠：只有 trigger 行，不显示 reasoning 内容（容器高度 0 + autoAlpha 0）。
 * - 展开·简略：底部钉住的最新 3 行预览（内层 max-h-18 + justify-end 从顶部裁剪），
 *   顶部渐隐表示前面还有内容（不满 3 行无渐隐）。
 * - 展开·撑开：完整 reasoning，限高 + 内部滚动 + 底部渐隐（滚到底后消失）。
 *
 * 转移：折叠 ⇄ 展开·简略 由 trigger 切换（折叠 → 展开一定落在简略）；
 * 简略 ⇄ 撑开 由展开态才出现的第二个按钮切换；任何展开形态可折叠回折叠。
 * 状态每条消息独立。streaming 期间自动进入展开·简略（trigger 显示"思考中…" +
 * shimmer，图标与 shimmer 用 port-image 蓝表示"处理中"，完成回中性）；
 * streaming 结束后停在 defaultView（本轮新消息 = 简略，历史消息 = 折叠）；
 * 用户手动操作过以用户为准。
 *
 * 三态切换动画（GSAP，useGSAP 带 scope）：外层容器做高度手风琴 + autoAlpha，
 * 从当前实际高度（ResizeObserver 持续记录）平滑到目标高度（量 scrollHeight /
 * line-height，不硬编码）；overwrite + killTweensOf 保证快速连切可打断；
 * 完成后 clearProps 交还 CSS；prefers-reduced-motion 时 duration 0 瞬时切换。
 */

// text-sm + leading-6（1.5rem/行）：3 行预览 = 4.5rem（max-h-18）
const EXPANDED_MAX_HEIGHT = "16rem"

type ReasoningView = "collapsed" | "preview" | "full"

export const Reasoning = memo(function Reasoning({
  text,
  streaming = false,
  defaultView = "collapsed",
  className,
}: {
  text: string
  streaming?: boolean
  /** 非 streaming 时的默认视图：本轮新消息传 "preview"，历史消息默认折叠 */
  defaultView?: "collapsed" | "preview"
  className?: string
}) {
  // null = 用户未手动操作，视图跟随 streaming/defaultView 自动推导
  const [userView, setUserView] = useState<ReasoningView | null>(null)
  const view: ReasoningView =
    userView ?? (streaming ? "preview" : defaultView)
  // streaming 的 reasoning 同样过水流平滑（3 行预览跟着柔和滚动）
  const smoothedText = useSmoothText(text, streaming)

  const [previewOverflow, setPreviewOverflow] = useState(false)
  const [bottomFade, setBottomFade] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const scrollerRef = useRef<HTMLDivElement>(null)
  const innerRef = useRef<HTMLDivElement>(null)
  const nearBottomRef = useRef(true)
  const prevViewRef = useRef<ReasoningView | null>(null)
  // 容器当前视觉高度，ResizeObserver 持续记录——动画起点与 view 变化时
  // React 已切换内层模式 class，直接量 getBoundingClientRect 会被 clamp 污染
  const heightRef = useRef(0)
  // line-height 只需测一次（字体固定），避免每个 RO 回调都 getComputedStyle
  const lineHeightRef = useRef(0)

  // 持续跟踪容器视觉高度（含 tween 逐帧），供动画起点使用
  useEffect(() => {
    const container = containerRef.current
    if (!container) return
    heightRef.current = container.getBoundingClientRect().height
    const observer = new ResizeObserver(() => {
      heightRef.current = container.getBoundingClientRect().height
    })
    observer.observe(container)
    return () => observer.disconnect()
  }, [])

  // 内容高度变化时重测：简略是否超过 3 行（顶部渐隐）、撑开底部渐隐。
  // ResizeObserver 首次回调是异步的，不会触发 effect 内同步 setState。
  useEffect(() => {
    const inner = innerRef.current
    const scroller = scrollerRef.current
    if (!inner || !scroller) return
    const observer = new ResizeObserver(() => {
      if (lineHeightRef.current === 0)
        lineHeightRef.current =
          parseFloat(getComputedStyle(inner).lineHeight) || 24
      setPreviewOverflow(inner.scrollHeight > lineHeightRef.current * 3 + 1)
      updateBottomFade(scroller, setBottomFade)
    })
    observer.observe(inner)
    return () => observer.disconnect()
  }, [])

  // 三态切换动画
  useGSAP(
    () => {
      const container = containerRef.current
      const scroller = scrollerRef.current
      const inner = innerRef.current
      if (!container || !scroller || !inner) return
      const prev = prevViewRef.current
      prevViewRef.current = view

      const lineHeight = parseFloat(getComputedStyle(inner).lineHeight) || 24
      const rem =
        parseFloat(getComputedStyle(document.documentElement).fontSize) || 16
      const target =
        view === "collapsed"
          ? 0
          : Math.min(
              inner.scrollHeight,
              view === "preview"
                ? lineHeight * 3
                : parseFloat(EXPANDED_MAX_HEIGHT) * rem
            )

      if (prev === null || prev === view) {
        // 初始落位：折叠态直接收起，不播动画
        if (view === "collapsed")
          gsap.set(container, { height: 0, autoAlpha: 0 })
        return
      }

      const from = heightRef.current
      const fromAlpha =
        Number.parseFloat(getComputedStyle(container).opacity) || 0
      const duration = motionDuration(MOTION_DURATION.base)
      const ease = MOTION_EASE.enter

      gsap.killTweensOf(container)

      if (view === "full") {
        // 进入撑开：钉住底部向下生长（onUpdate 持续钉底），结束后交还 CSS 滚动容器。
        // 顺序关键：先落 from 高度再钉底——否则 immediateRender 压回起始高度后
        // scrollTop 不再对应底部，首帧会落在内容中段。
        gsap.set(container, { height: from, autoAlpha: fromAlpha })
        scroller.scrollTop = scroller.scrollHeight
        gsap.to(container, {
          height: target,
          autoAlpha: 1,
          duration,
          ease,
          overwrite: "auto",
          onUpdate: () => {
            scroller.scrollTop = scroller.scrollHeight
          },
          onComplete: () => {
            gsap.set(container, { clearProps: "height,opacity,visibility" })
            scroller.scrollTop = scroller.scrollHeight
            nearBottomRef.current = true
            updateBottomFade(scroller, setBottomFade)
          },
        })
      } else {
        // 进入简略/折叠：内层解除 max-height 限制（scroller 保持全文高），
        // 由外层容器的 justify-end 把它钉在容器底部——整个伸缩过程恒贴底、
        // 从顶部裁剪，完成后 clearProps 交还 CSS（区域连续，无跳变；
        // collapsed 保持收起的 inline 终态）
        gsap.set(scroller, { maxHeight: "none" })
        gsap.fromTo(
          container,
          { height: from, autoAlpha: fromAlpha },
          {
            height: target,
            autoAlpha: view === "collapsed" ? 0 : 1,
            duration,
            ease,
            overwrite: "auto",
            onComplete: () => {
              gsap.set(scroller, { clearProps: "maxHeight" })
              if (view === "collapsed")
                gsap.set(container, { height: 0, autoAlpha: 0 })
              else
                gsap.set(container, {
                  clearProps: "height,opacity,visibility",
                })
            },
          }
        )
      }
    },
    { scope: rootRef, dependencies: [view] }
  )

  // streaming 且用户本就贴着底部时，撑开态跟随最新内容钉底（排版行为，不加动画）
  useEffect(() => {
    if (view !== "full" || !streaming) return
    const scroller = scrollerRef.current
    if (scroller && nearBottomRef.current)
      scroller.scrollTop = scroller.scrollHeight
  }, [view, streaming, text])

  if (text.trim() === "") return null

  const label = streaming ? "思考中…" : "思考过程"

  return (
    <div
      ref={rootRef}
      data-slot="reasoning"
      aria-busy={streaming || undefined}
      className={cn("mb-2 flex flex-col", className)}
    >
      <div className="flex items-center gap-1">
        <button
          type="button"
          aria-expanded={view !== "collapsed"}
          onClick={() =>
            setUserView(view === "collapsed" ? "preview" : "collapsed")
          }
          className={cn(
            "flex items-center gap-1.5 rounded-md py-1 text-sm text-muted-foreground outline-none transition-colors",
            "hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
          )}
        >
          <Brain
            aria-hidden
            className={cn("size-4 shrink-0", streaming && "text-port-image")}
          />
          <span className="relative inline-block leading-none">
            <span>{label}</span>
            {streaming ? (
              <span
                aria-hidden
                className="shimmer pointer-events-none absolute inset-0 text-port-image motion-reduce:animate-none"
              >
                {label}
              </span>
            ) : null}
          </span>
          <ChevronDown
            aria-hidden
            className={cn(
              "size-4 shrink-0 transition-transform duration-200 motion-reduce:transition-none",
              view !== "collapsed" && "rotate-180"
            )}
          />
        </button>

        {view !== "collapsed" ? (
          <button
            type="button"
            aria-pressed={view === "full"}
            aria-label={view === "full" ? "收起为简略" : "展开全部"}
            onClick={() =>
              setUserView(view === "full" ? "preview" : "full")
            }
            className={cn(
              "flex items-center gap-1 rounded-md px-1.5 py-1 text-xs text-muted-foreground outline-none transition-colors",
              "hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring/50"
            )}
          >
            {view === "full" ? (
              <ChevronsDownUp aria-hidden className="size-3.5 shrink-0" />
            ) : (
              <ChevronsUpDown aria-hidden className="size-3.5 shrink-0" />
            )}
            {view === "full" ? "简略" : "全部"}
          </button>
        ) : null}
      </div>

      {/* 动画容器：常驻挂载，高度/autoAlpha 由 GSAP 驱动；flex justify-end 把
          内层恒钉在容器底部（高度动画期间从顶部裁剪，首帧即贴底）；内层按形态
          切换裁剪（preview）/滚动（full）模式，动画完成后 clearProps 交还 CSS */}
      <div
        ref={containerRef}
        aria-hidden={view === "collapsed"}
        className="relative mt-1 flex flex-col justify-end overflow-hidden"
      >
        <div
          ref={scrollerRef}
          onScroll={(event) => {
            const scroller = event.currentTarget
            nearBottomRef.current =
              scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 40
            updateBottomFade(scroller, setBottomFade)
          }}
          style={
            view === "full"
              ? { maxHeight: EXPANDED_MAX_HEIGHT, scrollBehavior: "auto" }
              : { scrollBehavior: "auto" }
          }
          className={cn(
            "shrink-0",
            view === "full"
              ? "h-full overflow-y-auto"
              : "flex max-h-18 flex-col justify-end overflow-hidden"
          )}
        >
          <div
            ref={innerRef}
            className="wrap-break-word shrink-0 whitespace-pre-wrap text-sm leading-6 text-muted-foreground"
          >
            {smoothedText}
          </div>
        </div>
        <div
          aria-hidden
          className={cn(
            "pointer-events-none absolute inset-x-0 top-0 h-6 bg-[linear-gradient(to_bottom,var(--color-background),transparent)] transition-opacity duration-200 motion-reduce:transition-none",
            view === "preview" && previewOverflow ? "opacity-100" : "opacity-0"
          )}
        />
        <div
          aria-hidden
          className={cn(
            "pointer-events-none absolute inset-x-0 bottom-0 h-6 bg-[linear-gradient(to_top,var(--color-background),transparent)] transition-opacity duration-200 motion-reduce:transition-none",
            view === "full" && bottomFade ? "opacity-100" : "opacity-0"
          )}
        />
      </div>
    </div>
  )
})

function updateBottomFade(
  scroller: HTMLDivElement | null,
  setBottomFade: (visible: boolean) => void
) {
  if (!scroller) return
  setBottomFade(
    scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight > 8
  )
}
