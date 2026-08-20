"use client"

import { usePathname } from "next/navigation"
import { useRef } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import { PageShell } from "@/components/stratum/studio/primitives"
import {
  MOTION_DURATION,
  MOTION_EASE,
  motionDuration,
  prefersReducedMotion,
} from "@/lib/motion"

gsap.registerPlugin(useGSAP)

/**
 * 设置区共享外壳（/studio/settings/** 的 layout）：Provider 列表与编辑器
 * 共用 PageShell，区内导航（列表 ↔ 编辑器）只做一次快速内容淡入上浮；
 * 首屏到达交给整页转场，不重复播放；prefers-reduced-motion 瞬时。
 */
export function SettingsChrome({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const contentRef = useRef<HTMLDivElement>(null)
  const firstRef = useRef(true)

  useGSAP(
    () => {
      // 首屏到达由整页转场负责，区内导航才播内容淡入
      if (firstRef.current) {
        firstRef.current = false
        return
      }
      const el = contentRef.current
      if (!el || prefersReducedMotion()) return
      gsap.fromTo(
        el,
        { opacity: 0, y: 6 },
        {
          opacity: 1,
          y: 0,
          duration: motionDuration(MOTION_DURATION.fast),
          ease: MOTION_EASE.enter,
          clearProps: "transform,opacity",
        }
      )
    },
    { scope: contentRef, dependencies: [pathname] }
  )

  return (
    <PageShell>
      <div key={pathname} ref={contentRef} className="min-w-0">
        {children}
      </div>
    </PageShell>
  )
}
