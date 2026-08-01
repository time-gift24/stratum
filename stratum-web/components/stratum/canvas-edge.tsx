import * as React from "react"

import { cn } from "@/lib/utils"

type Point = { x: number; y: number }

/**
 * CanvasEdges —— 画布连线层。覆盖整个画布世界坐标的 SVG，
 * 子元素为若干 CanvasEdge。viewBox 需与节点定位所用坐标系一致。
 */
function CanvasEdges({
  width,
  height,
  className,
  children,
  ...props
}: React.ComponentProps<"svg"> & { width: number; height: number }) {
  return (
    <svg
      data-slot="canvas-edges"
      aria-hidden
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      className={cn("pointer-events-none absolute inset-0 size-full", className)}
      {...props}
    >
      {children}
    </svg>
  )
}

/**
 * CanvasEdge —— 两端口间的水平贝塞尔连线：1.5px、--edge 色、半透明，无箭头无动画。
 */
function CanvasEdge({ from, to }: { from: Point; to: Point }) {
  const handle = Math.max(48, Math.abs(to.x - from.x) / 2)
  const d = [
    `M ${from.x} ${from.y}`,
    `C ${from.x + handle} ${from.y}, ${to.x - handle} ${to.y}, ${to.x} ${to.y}`,
  ].join(" ")

  return (
    <path
      d={d}
      fill="none"
      stroke="var(--edge)"
      strokeOpacity={0.45}
      strokeWidth={1.5}
      vectorEffect="non-scaling-stroke"
    />
  )
}

export { CanvasEdges, CanvasEdge }
export type { Point }
