import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"

import { cn } from "@/lib/utils"

const floatingToolbarVariants = cva(
  "flex items-center gap-0.5 rounded-xl border border-border bg-card p-1 shadow-[0_8px_30px] shadow-black/35",
  {
    variants: {
      orientation: {
        vertical: "flex-col",
        horizontal: "flex-row",
      },
    },
    defaultVariants: { orientation: "vertical" },
  }
)

/**
 * FloatingToolbar —— 悬浮图标工具条（画布右侧竖排 / 预览下方横排）。
 * 子元素为一组图标按钮。
 */
function FloatingToolbar({
  orientation,
  className,
  children,
  ...props
}: React.ComponentProps<"div"> & VariantProps<typeof floatingToolbarVariants>) {
  return (
    <div
      data-slot="floating-toolbar"
      role="toolbar"
      aria-orientation={orientation ?? "vertical"}
      className={cn(floatingToolbarVariants({ orientation }), className)}
      {...props}
    >
      {children}
    </div>
  )
}

/**
 * FloatingToolbarButton —— 工具条内的图标按钮。
 */
function FloatingToolbarButton({
  label,
  active,
  className,
  children,
  ...props
}: React.ComponentProps<"button"> & { label: string; active?: boolean }) {
  return (
    <button
      type="button"
      aria-label={label}
      aria-pressed={active}
      className={cn(
        "rounded-lg p-2 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground aria-pressed:bg-muted aria-pressed:text-foreground [&_svg]:size-3.5",
        className
      )}
      {...props}
    >
      {children}
    </button>
  )
}

export { FloatingToolbar, FloatingToolbarButton }
