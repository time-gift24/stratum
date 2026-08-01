"use client"

import { SiteNav } from "@/components/react-bits/site-nav"

/**
 * 站点导航外壳（client 组件：图标是函数，不能从 Server Component 传入）。
 * SiteNavChrome —— root 级业务导航，由 (site) 路由组 layout 挂载，fixed 悬浮于所有页面之上。
 * 产品是单对话页，导航只保留对话入口。
 */
export function SiteNavChrome() {
  return (
    <SiteNav
      brand={{ name: "Stratum", href: "/conversation" }}
      links={[{ label: "对话", href: "/conversation" }]}
    />
  )
}
