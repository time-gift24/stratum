import { Streamdown } from "streamdown"

import { cn } from "@/lib/utils"

import styles from "../styles/prose-medium.module.css"

/**
 * MarkdownArticle —— Medium 风格的文章排版渲染。
 * 渲染层是 Streamdown（react-markdown 的流式替代，静态内容用 mode="static"）；
 * 排版来自共享的 stratum/styles/prose-medium.module.css：衬线正文 + 无衬线标题 +
 * 居中三点分节符 + 无边斜体引用，颜色只消费外层 token，亮暗随主题切换。
 * compact 用于并排对比等窄栏场景（等比缩小，标题随 em 自适应）。
 */
export function MarkdownArticle({
  children,
  compact,
  className,
}: {
  children: string
  compact?: boolean
  className?: string
}) {
  return (
    <div
      data-slot="markdown-article"
      className={cn(
        styles.proseMedium,
        compact && styles.proseMediumSm,
        className
      )}
    >
      <Streamdown mode="static">{children}</Streamdown>
    </div>
  )
}
