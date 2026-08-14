"use client"

import * as React from "react"
import Link from "next/link"
import { usePathname, useRouter } from "next/navigation"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import {
  MOTION_DURATION,
  MOTION_EASE,
  prefersReducedMotion,
} from "@/lib/motion"

gsap.registerPlugin(useGSAP)

/**
 * 页面转场（GSAP）：内部页面构成左 → 右的空间序列（PAGE_ORDER），
 * 向序列右侧跳转时当前页向左滑出、新页从右滑入；向左侧反之。
 * hash 锚点、外链、修饰键/非主键点击、prefers-reduced-motion 全部绕行（默认行为）。
 * 方向来源：TransitionLink 点击的一次性预告（armedDirection），
 * 否则按 pathname 在 PAGE_ORDER 中的索引差推导（浏览器前进/后退也有正确方向）。
 * 时长/缓动走全站统一尺度（lib/motion.ts）：进场 base、退场 fast。
 *
 * 注意：新增内部页面必须登记到 PAGE_ORDER，否则跳转无出场、只有入场。
 */

const PAGE_ORDER = ["/conversation", "/studio", "/ontologies", "/excalidraw"]

/** 子路由归并到一级入口：/studio/agents/x → /studio。 */
function pageIndex(pathname: string): number {
  return PAGE_ORDER.indexOf(`/${pathname.split("/")[1]}`)
}

// 模块级共享状态（应用级单例，仅客户端运行）
let pageElement: HTMLElement | null = null
let armedDirection: 1 | -1 | null = null
let lastPathname: string | null = null

/** 页面容器：路由变化时从新页方向滑入。挂在 (site)/layout。 */
export function PageTransition({ children }: { children: React.ReactNode }) {
  const ref = React.useRef<HTMLDivElement>(null)
  const pathname = usePathname()

  useGSAP(
    () => {
      const el = ref.current
      if (!el) return
      pageElement = el

      // 首屏不播入场：SSR 内容直接可见，不推迟 LCP
      const isFirstPaint = lastPathname === null
      let direction: 1 | -1 = 1
      if (armedDirection !== null) {
        direction = armedDirection
      } else if (lastPathname !== null) {
        const from = pageIndex(lastPathname)
        const to = pageIndex(pathname)
        if (from !== -1 && to !== -1 && from !== to) {
          direction = to > from ? 1 : -1
        }
      }
      armedDirection = null
      lastPathname = pathname

      if (!isFirstPaint && !prefersReducedMotion()) {
        gsap.fromTo(
          el,
          { x: 40 * direction, opacity: 0 },
          {
            x: 0,
            opacity: 1,
            duration: MOTION_DURATION.base,
            ease: MOTION_EASE.enter,
            // 播完清除内联样式：残留 transform 会把 fixed 的 dock 锁进本容器
            // （transform 使祖先成为 fixed 后代的包含块），破坏其视口定位
            clearProps: "transform,opacity",
          }
        )
      }
      return () => {
        if (pageElement === el) pageElement = null
      }
    },
    { scope: ref, dependencies: [pathname] }
  )

  return <div ref={ref} data-page-transition>{children}</div>
}

/** 带出场转场的 Link：点击 → 当前页滑出 → router.push → 新页滑入。 */
export function TransitionLink({
  href,
  onClick,
  children,
  ...props
}: React.ComponentProps<typeof Link>) {
  const router = useRouter()
  const pathname = usePathname()

  const handleClick = (e: React.MouseEvent<HTMLAnchorElement>) => {
    onClick?.(e)
    if (e.defaultPrevented) return
    if (e.button !== 0) return
    if (e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return
    if (props.target === "_blank") return
    if (typeof href !== "string") return
    // hash 锚点（含跨页锚点）与外链走默认导航，不播转场
    if (href.includes("#") || /^[a-z][a-z0-9+.-]*:/i.test(href)) return

    const from = pageIndex(pathname)
    const to = pageIndex(href)
    if (to === -1 || to === from) return // 未知页或当前页：交给 Link 默认导航

    const el = pageElement
    if (!el || prefersReducedMotion()) return

    e.preventDefault()
    const direction = to > from ? 1 : -1
    armedDirection = direction
    // 顶掉可能在播的入场/出场 tween，由本次出场统一接管
    gsap.killTweensOf(el)
    gsap.to(el, {
      x: -32 * direction,
      opacity: 0,
      duration: MOTION_DURATION.fast,
      ease: MOTION_EASE.exit,
      overwrite: "auto",
      onComplete: () => {
        router.push(href)
        // 保险丝：导航未提交（如被前进/后退取消）时恢复页面可见，
        // 否则容器会停在透明状态
        setTimeout(() => {
          if (window.location.pathname !== href && document.contains(el)) {
            gsap.set(el, { clearProps: "transform,opacity" })
          }
        }, 1500)
      },
    })
  }

  return (
    <Link href={href} onClick={handleClick} {...props}>
      {children}
    </Link>
  )
}
