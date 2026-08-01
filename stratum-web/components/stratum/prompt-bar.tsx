import * as React from "react"

import { cn } from "@/lib/utils"

/**
 * PromptBar —— 画布底部的提示词输入条：卡片内嵌只读展示文本 + 底部动作行。
 * 动作为调用方传入的图标按钮；左侧可挂主入口按钮（leading）。
 */
function PromptBar({
  value,
  label,
  leading,
  actions,
  className,
  ...props
}: React.ComponentProps<"div"> & {
  value: string
  label?: string
  leading?: React.ReactNode
  actions?: React.ReactNode
}) {
  return (
    <div
      data-slot="prompt-bar"
      className={cn("flex items-end gap-2", className)}
      {...props}
    >
      {leading}
      <div className="relative">
        {label ? (
          <span className="absolute -top-5 left-1 flex items-center gap-1.5 font-sans text-[0.625rem] text-muted-foreground">
            <span aria-hidden className="size-1.5 rounded-full bg-muted-foreground" />
            {label}
          </span>
        ) : null}
        <div className="w-96 rounded-2xl border border-border bg-card px-3.5 pt-3 pb-2 shadow-[0_8px_30px] shadow-black/35">
          <p className="line-clamp-2 font-sans text-xs leading-relaxed text-muted-foreground">
            {value}
          </p>
          <div className="mt-2 flex items-center justify-end gap-0.5">
            {actions}
          </div>
        </div>
      </div>
    </div>
  )
}

export { PromptBar }
