import * as React from "react"

import { cn } from "@/lib/utils"

/**
 * NodeCanvas —— 点阵网格画布容器。近黑底 + 径向点阵（--canvas-grid），
 * 内部用 absolute 定位放置节点、连线层与协作光标。
 */
function NodeCanvas({
  className,
  children,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="node-canvas"
      className={cn("relative overflow-hidden bg-background", className)}
      {...props}
    >
      <div
        aria-hidden
        className="absolute inset-0"
        style={{
          backgroundImage:
            "radial-gradient(circle, var(--canvas-grid) 1px, transparent 1px)",
          backgroundSize: "24px 24px",
        }}
      />
      {/* 光场：中央径向受光 + 四周暗角，节点群浮在有深度的空间里 */}
      <div
        aria-hidden
        className="absolute inset-0"
        style={{
          background: [
            "radial-gradient(ellipse 65% 60% at 50% 42%, rgb(255 255 255 / 0.05), transparent 70%)",
            "radial-gradient(ellipse at center, transparent 55%, rgb(0 0 0 / 0.45) 100%)",
          ].join(", "),
        }}
      />
      {children}
    </div>
  )
}

export { NodeCanvas }
