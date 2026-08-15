"use client"

import { SiteNav } from "@/components/react-bits/site-nav"

/**
 * 站点导航外壳（client 组件：图标是函数，不能从 Server Component 传入）。
 * SiteNavChrome —— root 级业务导航，由 (site) 路由组 layout 挂载，fixed 悬浮于所有页面之上。
 * 当前入口：对话（/conversation）、仪表盘（/studio）、本体（/ontologies）、Excalidraw（/excalidraw），
 * 右端 CTA 为设置入口（/studio/settings/providers）。
 *
 * 导航在所有页面常开，包括白板（/excalidraw）与本体编辑器（/ontologies/[id]）
 * 等沉浸页——不再自动收起，也没有唤出手柄与感应条。
 */
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
      cta={{ label: "设置", href: "/studio/settings/providers" }}
    />
  )
}
