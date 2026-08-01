import { SiteNavChrome } from "@/components/chrome/site-chrome"
import { PageTransition } from "@/components/chrome/page-transition"

/**
 * 站点业务场景的布局：顶部 SiteNav 为 root 级共享导航（fixed 悬浮于所有页面之上，
 * 不占位）；左侧 dock 由各页面场景自己挂载，同为 fixed 悬浮。
 * 页面自管对悬浮 nav 的避让（如首页的顶部留白）；/canvas 整屏编辑器在 nav 之下铺满。
 * PageTransition 负责内部页面跳转的方向性转场（见 components/chrome/page-transition）。
 */
export default function SiteLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <>
      <SiteNavChrome />
      <PageTransition>{children}</PageTransition>
    </>
  )
}
