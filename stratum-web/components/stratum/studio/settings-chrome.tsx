"use client"

import { usePathname, useSearchParams } from "next/navigation"
import { useRef } from "react"
import { useGSAP } from "@gsap/react"
import gsap from "gsap"

import {
  PageShell,
  SettingsNav,
  type SettingsSection,
} from "@/components/stratum/studio/primitives"
import { safeStudioReturn } from "@/features/studio-management/navigation"
import {
  MOTION_DURATION,
  MOTION_EASE,
  motionDuration,
  prefersReducedMotion,
} from "@/lib/motion"

gsap.registerPlugin(useGSAP)

/**
 * 设置区共享外壳（/studio/settings/** 的 layout）：PageShell + 左侧导航常驻。
 * 区内导航（Provider ↔ Model 页签、下钻编辑器、返回列表）只有右侧内容变化，
 * 内容区做一次快速淡入上浮；首屏到达交给整页转场，不重复播放；
 * prefers-reduced-motion 瞬时。
 */
export function SettingsChrome({ children }: { children: React.ReactNode }) {
  const pathname = usePathname()
  const searchParams = useSearchParams()
  const returnTo = safeStudioReturn(searchParams.get("returnTo"))
  const current: SettingsSection =
    pathname.split("/")[3] === "models" ? "models" : "providers"
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
      <div className="grid gap-6 lg:grid-cols-[12rem_minmax(0,1fr)] lg:gap-8">
        <SettingsNav current={current} returnTo={returnTo} />
        <div key={pathname} ref={contentRef} className="min-w-0">
          {children}
        </div>
      </div>
    </PageShell>
  )
}
