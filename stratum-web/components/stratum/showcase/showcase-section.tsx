import * as React from "react"

import { ScrollReveal } from "@/components/stratum/showcase/scroll-reveal"
import { cn } from "@/lib/utils"

/**
 * ShowcaseSection —— 组件展示页的统一小节容器：标题 + 说明 + demo 面板。
 * 新增组件 = 在首页注册一个 ShowcaseSection。
 */
function ShowcaseSection({
  id,
  title,
  description,
  className,
  children,
  ...props
}: React.ComponentProps<"section"> & {
  id: string
  title: string
  description: string
}) {
  return (
    <section
      id={id}
      data-slot="showcase-section"
      className={cn("scroll-mt-20", className)}
      {...props}
    >
      <ScrollReveal>
        <header className="mb-3">
          <h2 className="font-heading text-lg tracking-tight">{title}</h2>
          <p className="mt-1 max-w-prose text-sm leading-relaxed text-muted-foreground">
            {description}
          </p>
        </header>
        <div className="overflow-hidden rounded-2xl border border-border">
          {children}
        </div>
      </ScrollReveal>
    </section>
  )
}

/**
 * ShowcaseDemo —— demo 面板。canvas 世界的组件用 dark 固定暗色。
 * scale：画布复制件按参考图小尺寸复刻，直接放进面板显得迷你；
 * 用 zoom 放大展示（现代浏览器均支持），不改组件本身尺寸。
 */
function ShowcaseDemo({
  dark,
  scale,
  className,
  children,
  style,
  ...props
}: React.ComponentProps<"div"> & { dark?: boolean; scale?: number }) {
  return (
    <div
      data-slot="showcase-demo"
      className={cn(
        "flex items-center justify-center p-8",
        dark && "dark bg-background text-foreground",
        className
      )}
      style={{ zoom: scale, ...style } as React.CSSProperties}
      {...props}
    >
      {children}
    </div>
  )
}

export { ShowcaseSection, ShowcaseDemo }
