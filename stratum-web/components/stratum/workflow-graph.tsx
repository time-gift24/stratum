"use client"

import * as React from "react"

import {
  CanvasEdge,
  CanvasEdges,
  type Point,
} from "@/components/stratum/canvas-edge"
import { cn } from "@/lib/utils"

/**
 * WorkflowGraph —— 数据驱动的画布世界：固定坐标系容器 + 自动连线。
 *
 * 边用锚点键描述：`"nodeId"`（节点级，输出取右缘中点、输入取左缘中点）
 * 或 `"nodeId:portId"`（端口级，取端口色点中心）。挂载后与窗口 resize 时
 * 测量一次锚点位置并绘制贝塞尔连线，调用方不再需要手填像素坐标。
 */

export type GraphEdge = { from: string; to: string }

function locate(
  world: HTMLElement,
  worldBox: DOMRect,
  key: string,
  side: "from" | "to",
  scale: number
): Point | null {
  const s = scale || 1
  const [nodeId, portId] = key.split(":")

  if (portId) {
    const el = world.querySelector(`[data-port="${nodeId}:${portId}"]`)
    if (!el) return null
    const box = el.getBoundingClientRect()
    return {
      x: (box.left + box.width / 2 - worldBox.left) / s,
      y: (box.top + box.height / 2 - worldBox.top) / s,
    }
  }

  const el = world.querySelector(`[data-node="${nodeId}"]`)
  if (!el) return null
  const box = el.getBoundingClientRect()
  return {
    x: ((side === "from" ? box.right : box.left) - worldBox.left) / s,
    y: (box.top + box.height / 2 - worldBox.top) / s,
  }
}

function WorkflowGraph({
  edges,
  width,
  height,
  scale = 1,
  measureKey,
  className,
  children,
  ...props
}: React.ComponentProps<"div"> & {
  edges: GraphEdge[]
  width: number
  height: number
  /** 视口缩放倍率：锚点测量值需要除以它还原到世界坐标 */
  scale?: number
  /** 变化时重新测量锚点（如节点被拖拽后） */
  measureKey?: unknown
}) {
  const worldRef = React.useRef<HTMLDivElement>(null)
  const [paths, setPaths] = React.useState<{ from: Point; to: Point }[]>([])
  // scale 走 ref 而非依赖：locate 的差值与 scale 天然对消，缩放后 paths 恒不变，
  // 但测量发生时需要读到最新值
  const scaleRef = React.useRef(scale)
  React.useEffect(() => {
    scaleRef.current = scale
  }, [scale])

  React.useLayoutEffect(() => {
    function measure() {
      const world = worldRef.current
      if (!world) return
      const worldBox = world.getBoundingClientRect()
      setPaths(
        edges.flatMap((edge) => {
          const from = locate(world, worldBox, edge.from, "from", scaleRef.current)
          const to = locate(world, worldBox, edge.to, "to", scaleRef.current)
          return from && to ? [{ from, to }] : []
        })
      )
    }

    measure()
    window.addEventListener("resize", measure)
    return () => window.removeEventListener("resize", measure)
    // scale 不在依赖里：locate 的差值与 scale 天然对消，缩放后 paths 恒不变
  }, [edges, measureKey])

  return (
    <div
      ref={worldRef}
      data-slot="workflow-graph"
      className={cn("relative", className)}
      style={{ width, height }}
      {...props}
    >
      <CanvasEdges width={width} height={height}>
        {paths.map((path, index) => (
          <CanvasEdge key={index} from={path.from} to={path.to} />
        ))}
      </CanvasEdges>
      {children}
    </div>
  )
}

export { WorkflowGraph }
