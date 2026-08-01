"use client"

import * as React from "react"

import { NodeCanvas } from "@/components/stratum/node-canvas"
import { cn } from "@/lib/utils"

/**
 * InteractiveCanvas —— 可交互画布视口原语（stratum）。
 * 空白拖拽平移、滚轮/按钮缩放（光标或容器中心锚点）、节点拖拽
 * （受控 positions：指针位移换算后回调 onNodesChange）。
 * 缩放控件经 useInteractiveCanvas() 取 zoomAt 与当前倍率。
 * 节点拖拽与平移共享单指针锁；拖拽期间锁定世界-屏幕比例快照。
 */

export type CanvasPosition = { x: number; y: number }

type View = { x: number; y: number; k: number }

type DragState =
  | { type: "pan"; pointerId: number; lastX: number; lastY: number }
  | {
      type: "node"
      pointerId: number
      id: string
      lastX: number
      lastY: number
      k: number
    }
  | null

const PAN_BLOCK =
  "[data-node], button, a, input, textarea, select, [role='toolbar']"
const NODE_BLOCK = "button, a, input, textarea, select, [role='toolbar']"

function hitsControl(target: EventTarget | null, selector: string) {
  return target instanceof HTMLElement && target.closest(selector) !== null
}

export interface InteractiveCanvasHandle {
  zoomAt: (factor: number, clientX?: number, clientY?: number) => void
  k: number
}

const InteractiveCanvasContext =
  React.createContext<InteractiveCanvasHandle | null>(null)

/** 在 InteractiveCanvas 子树内取缩放控制（如工具栏 +/− 按钮） */
export function useInteractiveCanvas() {
  return React.useContext(InteractiveCanvasContext)
}

export function InteractiveCanvas({
  world,
  nodes,
  onNodesChange,
  minScale = 0.5,
  maxScale = 2,
  className,
  children,
}: {
  world: { width: number; height: number }
  /** 受控节点坐标（data-node=id 的元素可被拖拽）；不传则只支持平移/缩放 */
  nodes?: Record<string, CanvasPosition>
  onNodesChange?: (next: Record<string, CanvasPosition>) => void
  minScale?: number
  maxScale?: number
  className?: string
  children: React.ReactNode
}) {
  const [view, setView] = React.useState<View>({ x: 0, y: 0, k: 1 })
  const [panning, setPanning] = React.useState(false)
  const containerRef = React.useRef<HTMLDivElement>(null)
  const dragRef = React.useRef<DragState>(null)

  const zoomAt = React.useCallback(
    (factor: number, clientX?: number, clientY?: number) => {
      const container = containerRef.current
      if (!container) return
      const rect = container.getBoundingClientRect()
      setView((view) => {
        const k = Math.min(maxScale, Math.max(minScale, view.k * factor))
        if (k === view.k) return view
        // 锚点下的世界点保持不动：p = view + worldPoint * k
        const px = (clientX ?? rect.left + rect.width / 2) - rect.left
        const py = (clientY ?? rect.top + rect.height / 2) - rect.top
        const worldX = (px - view.x) / view.k
        const worldY = (py - view.y) / view.k
        return { x: px - worldX * k, y: py - worldY * k, k }
      })
    },
    [minScale, maxScale]
  )

  // 滚轮缩放必须非 passive（要 preventDefault 阻止页面滚动）
  React.useEffect(() => {
    const container = containerRef.current
    if (!container) return
    const onWheel = (e: WheelEvent) => {
      e.preventDefault()
      zoomAt(Math.exp(-e.deltaY * 0.0012), e.clientX, e.clientY)
    }
    container.addEventListener("wheel", onWheel, { passive: false })
    return () => container.removeEventListener("wheel", onWheel)
  }, [zoomAt])

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0 || dragRef.current) return // 单指针交互

    const target =
      e.target instanceof HTMLElement ? e.target : null
    const nodeEl = target?.closest("[data-node]")
    const nodeId = nodeEl?.getAttribute("data-node")

    if (nodeId && nodes && onNodesChange) {
      if (hitsControl(e.target, NODE_BLOCK)) return
      dragRef.current = {
        type: "node",
        pointerId: e.pointerId,
        id: nodeId,
        lastX: e.clientX,
        lastY: e.clientY,
        k: view.k,
      }
    } else {
      if (hitsControl(e.target, PAN_BLOCK)) return
      dragRef.current = {
        type: "pan",
        pointerId: e.pointerId,
        lastX: e.clientX,
        lastY: e.clientY,
      }
    }

    try {
      e.currentTarget.setPointerCapture(e.pointerId)
    } catch {
      dragRef.current = null // 指针已失效，放弃本次拖拽
      return
    }
    if (dragRef.current.type === "pan") setPanning(true)
  }

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const drag = dragRef.current
    if (!drag || drag.pointerId !== e.pointerId) return
    const dx = e.clientX - drag.lastX
    const dy = e.clientY - drag.lastY
    drag.lastX = e.clientX
    drag.lastY = e.clientY

    if (drag.type === "pan") {
      setView((view) => ({ ...view, x: view.x + dx, y: view.y + dy }))
    } else if (nodes && onNodesChange) {
      const position = nodes[drag.id]
      if (!position) return
      onNodesChange({
        ...nodes,
        [drag.id]: {
          x: Math.round(position.x + dx / drag.k),
          y: Math.round(position.y + dy / drag.k),
        },
      })
    }
  }

  const handlePointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (dragRef.current?.pointerId === e.pointerId) {
      dragRef.current = null
      setPanning(false)
    }
  }

  const handle = React.useMemo<InteractiveCanvasHandle>(
    () => ({ zoomAt, k: view.k }),
    [zoomAt, view.k]
  )

  return (
    <InteractiveCanvasContext.Provider value={handle}>
      <div
        ref={containerRef}
        data-slot="interactive-canvas"
        className={cn(
          "absolute inset-0 touch-none overflow-hidden select-none",
          panning ? "cursor-grabbing" : "cursor-grab",
          className
        )}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerUp}
      >
        <NodeCanvas className="absolute inset-0">
          <div className="flex size-full items-center justify-center">
            <div
              style={{
                width: world.width,
                height: world.height,
                transform: `translate(${view.x}px, ${view.y}px) scale(${view.k})`,
                transformOrigin: "0 0",
              }}
            >
              {children}
            </div>
          </div>
        </NodeCanvas>
      </div>
    </InteractiveCanvasContext.Provider>
  )
}
