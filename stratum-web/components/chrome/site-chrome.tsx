"use client"

import { Suspense, useEffect, useRef, useSyncExternalStore } from "react"
import { usePathname, useSearchParams } from "next/navigation"
import { Moon, Settings, Sun } from "lucide-react"
import { useTheme } from "next-themes"

import { TransitionLink } from "@/components/chrome/page-transition"
import { SiteNav } from "@/components/react-bits/site-nav"
import { Button } from "@/components/ui/button"
import { withStudioReturn } from "@/features/studio-management/navigation"

import styles from "./site-chrome.module.css"

/**
 * 站点导航外壳（client 组件：图标是函数，不能从 Server Component 传入）。
 * SiteNavChrome —— root 级业务导航，由 (site) 路由组 layout 挂载，fixed 悬浮于所有页面之上。
 * 当前入口：对话（/conversation）、计划任务（/schedulers）、仪表盘（/studio）、本体（/ontologies）、Excalidraw（/excalidraw）。
 * 右端是图标操作：主题切换 + 设置入口（/studio/settings/providers），均为纯图标。
 *
 * 导航在所有页面常开，包括白板（/excalidraw）与本体编辑器（/ontologies/[id]）
 * 等沉浸页——不再自动收起，也没有唤出手柄与感应条。
 */

const actionIconClass =
  "flex size-11 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground aria-[current=page]:bg-muted aria-[current=page]:text-foreground"

function ThemeToggleAction() {
  const { resolvedTheme, setTheme } = useTheme()
  // next-themes hydration 期间 resolvedTheme 未定：服务端/首帧按浅色渲染，
  // 水合后切到真实主题，避免 hydration mismatch（useSyncExternalStore 的
  // getServerSnapshot 正是干这个的，不需要 mounted state）
  const hydrated = useSyncExternalStore(
    () => () => {},
    () => true,
    () => false
  )

  const dark = hydrated && resolvedTheme === "dark"
  return (
    <Button
      type="button"
      aria-label={dark ? "切换到浅色模式" : "切换到深色模式"}
      variant="ghost"
      size="icon-lg"
      className={actionIconClass}
      onClick={() => setTheme(dark ? "light" : "dark")}
    >
      {dark ? (
        <Sun aria-hidden className="size-4" />
      ) : (
        <Moon aria-hidden className="size-4" />
      )}
    </Button>
  )
}

function SettingsLink({
  href,
  current = false,
}: {
  href: string
  current?: boolean
}) {
  return (
    <TransitionLink
      href={href}
      aria-label="设置"
      aria-current={current ? "page" : undefined}
      title="设置"
      className={actionIconClass}
    >
      <Settings aria-hidden className="size-4" />
    </TransitionLink>
  )
}

/** 从仪表盘进入设置时保留可恢复的搜索/分页；其他页面安全回到仪表盘。 */
function SettingsAction() {
  const pathname = usePathname()
  const searchParams = useSearchParams()
  const dashboardParams = new URLSearchParams()
  if (pathname === "/studio") {
    const query = searchParams.get("q")?.trim()
    const page = Number(searchParams.get("page"))
    if (query) dashboardParams.set("q", query)
    if (Number.isInteger(page) && page > 1)
      dashboardParams.set("page", String(page))
  }
  const returnTo =
    pathname === "/studio" && dashboardParams.size > 0
      ? `/studio?${dashboardParams}`
      : "/studio"
  return (
    <SettingsLink
      href={withStudioReturn("/studio/settings/providers", returnTo)}
      current={
        pathname === "/studio/settings" ||
        pathname.startsWith("/studio/settings/")
      }
    />
  )
}

export function SiteNavChrome() {
  const pathname = usePathname()
  const chromeRef = useRef<HTMLDivElement>(null)

  // The protected SiteNav owns its menu state. Close an expanded mobile menu
  // after a client-side route commit from this business wrapper boundary.
  useEffect(() => {
    const toggle = chromeRef.current?.querySelector<HTMLButtonElement>(
      '[data-nav-mobile] button[aria-expanded="true"]'
    )
    toggle?.click()
  }, [pathname])

  return (
    <div ref={chromeRef} className={styles.siteChrome}>
      <SiteNav
        brand={{ name: "Stratum", href: "/conversation" }}
        links={[
          { label: "对话", href: "/conversation" },
          { label: "计划任务", href: "/schedulers" },
          { label: "仪表盘", href: "/studio" },
          { label: "本体", href: "/ontologies" },
          { label: "Excalidraw", href: "/excalidraw" },
        ]}
        actions={
          <>
            <ThemeToggleAction />
            <Suspense
              fallback={
                <SettingsLink
                  href={withStudioReturn(
                    "/studio/settings/providers",
                    "/studio"
                  )}
                />
              }
            >
              <SettingsAction />
            </Suspense>
          </>
        }
      />
    </div>
  )
}
