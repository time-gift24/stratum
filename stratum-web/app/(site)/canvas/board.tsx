"use client"

import * as React from "react"
import {
  Crosshair,
  Expand,
  Layers,
  LayoutGrid,
  Minus,
  Plus,
} from "lucide-react"

import { CursorPresence } from "@/components/stratum/cursor-presence"
import {
  FloatingToolbar,
  FloatingToolbarButton,
} from "@/components/stratum/floating-toolbar"
import {
  InteractiveCanvas,
  useInteractiveCanvas,
  type CanvasPosition,
} from "@/components/stratum/interactive-canvas"
import {
  WorkflowGraph,
  type GraphEdge,
} from "@/components/stratum/workflow-graph"
import { WorkflowNode } from "@/components/stratum/workflow-node"
import { cn } from "@/lib/utils"

/**
 * WorkflowBoard —— 工作流画布页面组合：数据（节点/边/光标）到
 * InteractiveCanvas 视口与 WorkflowGraph 连线的映射。
 * 通用交互原语在 stratum/interactive-canvas；这里只保留业务数据。
 */

export type BoardNode = {
  id: string
  title: string
  status: React.ComponentProps<typeof WorkflowNode>["status"]
  position: CanvasPosition
  className?: string
  aurora?: boolean
  label?: React.ComponentProps<typeof WorkflowNode>["label"]
  floatingAction?: React.ReactNode
  body: React.ReactNode
}

export function WorkflowBoard({
  nodes: initialNodes,
  edges,
  cursors,
  world,
}: {
  /** 须为引用稳定的静态数据；positions 只在挂载时初始化，重置请换 key */
  nodes: BoardNode[]
  edges: GraphEdge[]
  cursors: { name: string; color: string; position: CanvasPosition }[]
  world: { width: number; height: number }
}) {
  const [positions, setPositions] = React.useState<
    Record<string, CanvasPosition>
  >(() =>
    Object.fromEntries(initialNodes.map((node) => [node.id, node.position]))
  )
  const [measureKey, setMeasureKey] = React.useState(0)

  const handleNodesChange = React.useCallback(
    (next: Record<string, CanvasPosition>) => {
      setPositions(next)
      setMeasureKey((key) => key + 1)
    },
    []
  )

  return (
    <InteractiveCanvas
      world={world}
      nodes={positions}
      onNodesChange={handleNodesChange}
    >
      <BoardContent
        nodes={initialNodes}
        positions={positions}
        edges={edges}
        cursors={cursors}
        measureKey={measureKey}
        world={world}
      />
      <BoardToolbar />
    </InteractiveCanvas>
  )
}

function BoardContent({
  nodes,
  positions,
  edges,
  cursors,
  measureKey,
  world,
}: {
  nodes: BoardNode[]
  positions: Record<string, CanvasPosition>
  edges: GraphEdge[]
  cursors: { name: string; color: string; position: CanvasPosition }[]
  measureKey: number
  world: { width: number; height: number }
}) {
  const canvas = useInteractiveCanvas()

  return (
    <WorkflowGraph
      edges={edges}
      width={world.width}
      height={world.height}
      scale={canvas?.k ?? 1}
      measureKey={measureKey}
    >
      {nodes.map((node) => {
        const position = positions[node.id] ?? node.position
        return (
          <WorkflowNode
            key={node.id}
            nodeId={node.id}
            title={node.title}
            status={node.status}
            aurora={node.aurora}
            label={node.label}
            floatingAction={node.floatingAction}
            className={cn("absolute cursor-default", node.className)}
            style={{ left: position.x, top: position.y }}
          >
            {node.body}
          </WorkflowNode>
        )
      })}
      {cursors.map((cursor) => (
        <CursorPresence
          key={cursor.name}
          name={cursor.name}
          color={cursor.color}
          style={{ left: cursor.position.x, top: cursor.position.y }}
        />
      ))}
    </WorkflowGraph>
  )
}

function BoardToolbar() {
  const canvas = useInteractiveCanvas()

  return (
    <FloatingToolbar
      orientation="vertical"
      className="absolute top-1/2 right-3 -translate-y-1/2"
    >
      <FloatingToolbarButton label="全屏">
        <Expand aria-hidden />
      </FloatingToolbarButton>
      <FloatingToolbarButton label="缩小" onClick={() => canvas?.zoomAt(1 / 1.2)}>
        <Minus aria-hidden />
      </FloatingToolbarButton>
      <FloatingToolbarButton label="放大" onClick={() => canvas?.zoomAt(1.2)}>
        <Plus aria-hidden />
      </FloatingToolbarButton>
      <FloatingToolbarButton label="布局">
        <LayoutGrid aria-hidden />
      </FloatingToolbarButton>
      <FloatingToolbarButton label="聚焦">
        <Crosshair aria-hidden />
      </FloatingToolbarButton>
      <FloatingToolbarButton label="图层">
        <Layers aria-hidden />
      </FloatingToolbarButton>
    </FloatingToolbar>
  )
}
