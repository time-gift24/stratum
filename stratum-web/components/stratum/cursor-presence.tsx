import * as React from "react"
import { MousePointer2 } from "lucide-react"

import { cn } from "@/lib/utils"

/**
 * CursorPresence —— 协作光标：彩色箭头指针 + 同色名牌。
 * 颜色代表协作者身份，由调用方传入（非 token）；位置用 className 或 style 定位。
 */
function CursorPresence({
  name,
  color,
  className,
  style,
}: {
  name: string
  color: string
  className?: string
  style?: React.CSSProperties
}) {
  return (
    <div
      data-slot="cursor-presence"
      aria-hidden
      className={cn("pointer-events-none absolute flex items-start", className)}
      style={style}
    >
      <MousePointer2
        className="size-4"
        style={{ color, fill: color }}
        strokeWidth={1.5}
      />
      <span
        className="mt-2.5 ml-1 rounded-full px-1.5 py-0.5 font-sans text-[0.625rem] leading-none font-medium text-background"
        style={{ backgroundColor: color }}
      >
        {name}
      </span>
    </div>
  )
}

export { CursorPresence }
