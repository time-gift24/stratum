"use client"

import { useSyncExternalStore } from "react"
import { Moon, Settings, Sun } from "lucide-react"
import { useTheme } from "next-themes"

import { TransitionLink } from "@/components/chrome/page-transition"
import { SiteNav } from "@/components/react-bits/site-nav"

/**
 * 站点导航外壳（client 组件：图标是函数，不能从 Server Component 传入）。
 * SiteNavChrome —— root 级业务导航，由 (site) 路由组 layout 挂载，fixed 悬浮于所有页面之上。
 * 当前入口：对话（/conversation）、仪表盘（/studio）、本体（/ontologies）、Excalidraw（/excalidraw）。
 * 右端是图标操作：主题切换 + 设置入口（/studio/settings/providers），均为纯图标。
 *
 * 导航在所有页面常开，包括白板（/excalidraw）与本体编辑器（/ontologies/[id]）
 * 等沉浸页——不再自动收起，也没有唤出手柄与感应条。
 */

const actionIconClass =
  "flex size-9 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"

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
    <button
      type="button"
      aria-label={dark ? "切换到浅色模式" : "切换到深色模式"}
      className={actionIconClass}
      onClick={() => setTheme(dark ? "light" : "dark")}
    >
      {dark ? (
        <Sun aria-hidden className="size-4" />
      ) : (
        <Moon aria-hidden className="size-4" />
      )}
    </button>
  )
}

export function SiteNavChrome() {
  return (
    <SiteNav
      brand={{ name: "Stratum", href: "/conversation" }}
      links={[
        { label: "对话", href: "/conversation" },
        { label: "仪表盘", href: "/studio" },
        { label: "本体", href: "/ontologies" },
        { label: "Excalidraw", href: "/excalidraw" },
      ]}
      actions={
        <>
          <ThemeToggleAction />
          <TransitionLink
            href="/studio/settings/providers"
            aria-label="设置"
            className={actionIconClass}
          >
            <Settings aria-hidden className="size-4" />
          </TransitionLink>
        </>
      }
    />
  )
}
