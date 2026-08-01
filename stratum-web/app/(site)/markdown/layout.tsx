import type { CSSProperties } from "react"

/**
 * Markdown 世界的局部主色：把 --primary 重映射为 port-image 蓝。
 * 绿归站点行动/品牌（SiteNav 在容器外，不受影响）；
 * 蓝归 markdown/AI 生成场景——容器内所有消费 primary 的组件自动跟随。
 */
const markdownTheme = {
  "--primary": "var(--port-image)",
  "--primary-foreground": "oklch(0.25 0.05 250)",
  "--ring": "var(--port-image)",
} as CSSProperties

export default function MarkdownLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return <div style={markdownTheme}>{children}</div>
}
