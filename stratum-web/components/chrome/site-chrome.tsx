"use client"

import { useEffect, useRef, useState } from "react"
import { usePathname } from "next/navigation"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import { SiteNav } from "@/components/react-bits/site-nav"
import {
  MOTION_DURATION,
  MOTION_EASE,
  prefersReducedMotion,
} from "@/lib/motion"

gsap.registerPlugin(useGSAP)

/**
 * 站点导航外壳（client 组件：图标是函数，不能从 Server Component 传入）。
 * SiteNavChrome —— root 级业务导航，由 (site) 路由组 layout 挂载，fixed 悬浮于所有页面之上。
 * 当前入口：对话（/conversation）、本体（/ontologies）、Excalidraw（/excalidraw）。
 *
 * 沉浸模式（/excalidraw 与本体编辑器 /ontologies/[id]）：导航默认收起，
 * 只留画布。进入时先 peek 1.6s
 * 展示入口位置再滑出；顶边 8px 感应条（悬停 150ms 意图延迟）或居中的
 * 阶梯两道杠手柄（点击 / 键盘聚焦，Tab 第一站）唤出，离开导航 200ms
 * 后或按 Esc 收回。prefers-reduced-motion 全程瞬时。
 */

const PEEK_MS = 1600
const INTENT_MS = 150
const LEAVE_MS = 200

/** 沉浸路由：白板 + 本体编辑器（/ontologies/<id> 单段动态路由；列表页除外） */
const IMMERSIVE_PATTERN = /^\/ontologies\/[^/]+$/
export function SiteNavChrome() {
  const pathname = usePathname()
  const immersive =
    pathname === "/excalidraw" || IMMERSIVE_PATTERN.test(pathname)
  const [open, setOpen] = useState(true)
  const navWrapRef = useRef<HTMLDivElement>(null)
  const handleRef = useRef<HTMLButtonElement>(null)
  const intentTimerRef = useRef<number | null>(null)
  const leaveTimerRef = useRef<number | null>(null)

  // 路由切换复位为常开（derive-during-render，避免 effect 内同步 setState）；
  // 进入沉浸页的 peek 收起走异步定时器
  const [prevImmersive, setPrevImmersive] = useState(immersive)
  if (immersive !== prevImmersive) {
    setPrevImmersive(immersive)
    setOpen(true)
  }

  useEffect(() => {
    if (!immersive) return
    const timer = window.setTimeout(() => setOpen(false), PEEK_MS)
    return () => window.clearTimeout(timer)
  }, [immersive, pathname])

  // open 变化 → 对 fixed 的 nav 直接做 y/autoAlpha；展开完成后 clearProps
  // transform，避免 transform 包含块困住 nav 内部的 fixed 后代
  useGSAP(
    () => {
      const nav = navWrapRef.current?.firstElementChild as HTMLElement | null
      if (!nav) return
      gsap.killTweensOf(nav)
      if (open) {
        gsap.to(nav, {
          y: 0,
          autoAlpha: 1,
          duration: prefersReducedMotion() ? 0 : MOTION_DURATION.base,
          ease: MOTION_EASE.enter,
          overwrite: "auto",
          onComplete: () => gsap.set(nav, { clearProps: "transform" }),
        })
      } else {
        gsap.to(nav, {
          y: "-110%",
          autoAlpha: 0,
          duration: prefersReducedMotion() ? 0 : MOTION_DURATION.fast,
          ease: MOTION_EASE.exit,
          overwrite: "auto",
        })
      }
    },
    { dependencies: [open, immersive] }
  )

  const clearTimers = () => {
    if (intentTimerRef.current !== null) {
      window.clearTimeout(intentTimerRef.current)
      intentTimerRef.current = null
    }
    if (leaveTimerRef.current !== null) {
      window.clearTimeout(leaveTimerRef.current)
      leaveTimerRef.current = null
    }
  }

  const revealWithIntent = () => {
    if (open || intentTimerRef.current !== null) return
    intentTimerRef.current = window.setTimeout(() => {
      intentTimerRef.current = null
      setOpen(true)
    }, INTENT_MS)
  }

  const scheduleHide = () => {
    if (!immersive) return
    clearTimers()
    leaveTimerRef.current = window.setTimeout(() => {
      leaveTimerRef.current = null
      setOpen(false)
    }, LEAVE_MS)
  }

  const collapse = () => {
    clearTimers()
    setOpen(false)
    // 焦点还回手柄，键盘流不断
    requestAnimationFrame(() => handleRef.current?.focus())
  }

  return (
    <>
      {immersive ? (
        <>
          {/* 全宽顶边感应条：仅悬停唤出，无视觉、不挡下方工具条（8px） */}
          <div
            aria-hidden
            onPointerEnter={revealWithIntent}
            onPointerLeave={clearTimers}
            className="fixed inset-x-0 top-0 z-40 h-2"
          />
          {/* 阶梯手柄：唤出/收起导航的实际按钮，悬停微亮 */}
          <button
            ref={handleRef}
            type="button"
            aria-label={open ? "收起导航" : "显示导航"}
            aria-expanded={open}
            onClick={() => (open ? collapse() : setOpen(true))}
            onPointerEnter={revealWithIntent}
            onPointerLeave={clearTimers}
            onFocus={() => setOpen(true)}
            className="group fixed top-0 left-1/2 z-40 flex h-7 w-14 -translate-x-1/2 items-start justify-center rounded-b-lg outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
          >
            <span
              aria-hidden
              className="mt-1.5 flex flex-col items-center gap-[3px]"
            >
              <span className="h-1 w-8 rounded-full bg-muted-foreground/30 transition-colors group-hover:bg-muted-foreground/50" />
              <span className="h-1 w-5 rounded-full bg-muted-foreground/30 transition-colors group-hover:bg-muted-foreground/50" />
            </span>
          </button>
        </>
      ) : null}
      <div
        ref={navWrapRef}
        onPointerEnter={immersive ? clearTimers : undefined}
        onPointerLeave={immersive ? scheduleHide : undefined}
        onKeyDown={
          immersive
            ? (event) => {
                if (event.key === "Escape") collapse()
              }
            : undefined
        }
      >
        <SiteNav
          brand={{ name: "Stratum", href: "/conversation" }}
          links={[
            { label: "对话", href: "/conversation" },
            { label: "本体", href: "/ontologies" },
            { label: "Excalidraw", href: "/excalidraw" },
          ]}
        />
      </div>
    </>
  )
}
