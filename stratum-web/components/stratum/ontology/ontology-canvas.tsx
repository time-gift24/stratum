"use client"

import { useCallback, useMemo, useState } from "react"
import {
  applyEdgeChanges,
  applyNodeChanges,
  Background,
  BackgroundVariant,
  Controls,
  MarkerType,
  MiniMap,
  ReactFlow,
  type Connection,
  type EdgeChange,
  type NodeChange,
  type OnSelectionChangeParams,
} from "@xyflow/react"

// xyflow 样式表随本模块动态 chunk 懒加载（不进首屏 CSS）
import "@xyflow/react/dist/style.css"

import { resolveNodePositions } from "@/features/ontology-editor/layout"
import type { LocalNeighborhood } from "@/features/ontology-editor/neighborhood"
import type {
  OntologyDocument,
  OntologyNeighborhood,
} from "@/features/ontology-editor/types"
import { cn } from "@/lib/utils"

import {
  OntologyLinkTypeEdge,
  type LinkTypeEdge,
} from "@/components/stratum/ontology/ontology-edge"
import {
  OntologyObjectTypeNode,
  type ObjectTypeNode,
  type ObjectTypePropertyActions,
} from "@/components/stratum/ontology/ontology-node"
import styles from "@/components/stratum/ontology/ontology-theme.module.css"

/**
 * Ontology 编辑画布：受控渲染 candidate —— 节点 = object_types，
 * 边 = link_types，坐标 = canvas.positions（缺失走确定性网格）。
 * 拖拽过程在本地应用 xyflow 变更保证流畅，落笔经 onNodeDragStop 写回
 * candidate；删除键关闭，删除只走编辑面板的确认流程。
 */

const nodeTypes = { ontologyObjectType: OntologyObjectTypeNode }
const edgeTypes = { ontologyLinkType: OntologyLinkTypeEdge }

const MINIMAP_PROPS = { pannable: true, zoomable: true } as const

export type CanvasSelection = {
  kind: "objectType" | "linkType"
  id: string
} | null

type CanvasMessages = ReadonlyMap<string, readonly string[]>

function toNodes(
  document: OntologyDocument,
  focus: LocalNeighborhood | null,
  objectViolations: CanvasMessages,
  propertyActions: ObjectTypePropertyActions
): ObjectTypeNode[] {
  const positions = resolveNodePositions(document)
  return document.object_types.map((objectType) => ({
    id: objectType.id,
    type: "ontologyObjectType" as const,
    position: positions.get(objectType.id) ?? { x: 0, y: 0 },
    data: {
      objectType,
      violations: objectViolations.get(objectType.id) ?? [],
      dimmed: focus !== null && !focus.objectTypeIds.has(objectType.id),
      propertyActions,
    },
  }))
}

function toEdges(
  linkTypes: OntologyDocument["link_types"],
  focus: LocalNeighborhood | null,
  linkViolations: CanvasMessages
): LinkTypeEdge[] {
  return linkTypes.map((linkType) => ({
    id: linkType.id,
    type: "ontologyLinkType" as const,
    source: linkType.source_object_type_id,
    target: linkType.target_object_type_id,
    markerEnd: { type: MarkerType.ArrowClosed },
    className: cn(
      focus !== null && !focus.linkTypeIds.has(linkType.id) && "edgeDimmed"
    ),
    data: {
      linkType,
      violations: linkViolations.get(linkType.id) ?? [],
      dimmed: focus !== null && !focus.linkTypeIds.has(linkType.id),
    },
  }))
}

export function OntologyCanvas({
  document,
  focus,
  objectViolations,
  linkViolations,
  propertyActions,
  onSelectionChange,
  onConnectNodes,
  onNodeDragStop,
}: {
  document: OntologyDocument
  focus: LocalNeighborhood | null
  objectViolations: CanvasMessages
  linkViolations: CanvasMessages
  /** 属性行内增删改（来自 use-ontology-editor，需引用稳定） */
  propertyActions: ObjectTypePropertyActions
  onSelectionChange(selection: CanvasSelection): void
  onConnectNodes(sourceId: string, targetId: string): void
  onNodeDragStop(objectTypeId: string, x: number, y: number): void
}) {
  const baseNodes = useMemo(
    () => toNodes(document, focus, objectViolations, propertyActions),
    [document, focus, objectViolations, propertyActions]
  )
  const baseEdges = useMemo(
    () => toEdges(document.link_types, focus, linkViolations),
    [document, focus, linkViolations]
  )

  // 本地渲染态：拖拽/框选等交互即时应用；外部数据变化时重建并保留内部选中态
  // （derive-during-render，与 site-chrome 同约定，避免 effect 内同步 setState）
  const [nodes, setNodes] = useState(baseNodes)
  const [edges, setEdges] = useState(baseEdges)
  const [prevBase, setPrevBase] = useState({ nodes: baseNodes, edges: baseEdges })

  if (baseNodes !== prevBase.nodes) {
    setPrevBase({ nodes: baseNodes, edges: baseEdges })
    setNodes(
      baseNodes.map((node) => {
        const before = nodes.find((entry) => entry.id === node.id)
        return before === undefined
          ? node
          : { ...node, selected: before.selected }
      })
    )
  }
  if (baseEdges !== prevBase.edges) {
    setPrevBase({ nodes: baseNodes, edges: baseEdges })
    setEdges(
      baseEdges.map((edge) => {
        const before = edges.find((entry) => entry.id === edge.id)
        return before === undefined
          ? edge
          : { ...edge, selected: before.selected }
      })
    )
  }

  const handleNodesChange = useCallback(
    (changes: NodeChange<ObjectTypeNode>[]) =>
      setNodes((previous) => applyNodeChanges(changes, previous)),
    []
  )
  const handleEdgesChange = useCallback(
    (changes: EdgeChange<LinkTypeEdge>[]) =>
      setEdges((previous) => applyEdgeChanges(changes, previous)),
    []
  )

  const handleSelectionChange = useCallback(
    (params: OnSelectionChangeParams<ObjectTypeNode, LinkTypeEdge>) => {
      const node = params.nodes[0]
      if (node !== undefined) {
        onSelectionChange({ kind: "objectType", id: node.id })
        return
      }
      const edge = params.edges[0]
      if (edge !== undefined) {
        onSelectionChange({ kind: "linkType", id: edge.id })
        return
      }
      onSelectionChange(null)
    },
    [onSelectionChange]
  )

  const handleConnect = useCallback(
    (connection: Connection) => {
      if (connection.source === "" || connection.target === "") return
      onConnectNodes(connection.source, connection.target)
    },
    [onConnectNodes]
  )

  return (
    <div className={cn("h-full w-full", styles.theme)}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        onNodesChange={handleNodesChange}
        onEdgesChange={handleEdgesChange}
        onSelectionChange={handleSelectionChange}
        onConnect={handleConnect}
        onNodeDragStop={(_, node) =>
          onNodeDragStop(node.id, node.position.x, node.position.y)
        }
        deleteKeyCode={null}
        fitView
        minZoom={0.2}
        proOptions={{ hideAttribution: false }}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={24}
          size={1.5}
          color="var(--border)"
          bgColor="transparent"
        />
        <Controls showInteractive={false} />
        <MiniMap {...MINIMAP_PROPS} />
      </ReactFlow>
    </div>
  )
}

/** 邻域只读画布：无编辑入口（不可拖拽、不可连线、不可选中），仅缩放平移。 */
export function NeighborhoodCanvas({
  neighborhood,
}: {
  neighborhood: OntologyNeighborhood
}) {
  const nodes = useMemo<ObjectTypeNode[]>(() => {
    const positions = resolveNodePositions(neighborhood)
    return neighborhood.object_types.map((objectType) => ({
      id: objectType.id,
      type: "ontologyObjectType" as const,
      position: positions.get(objectType.id) ?? { x: 0, y: 0 },
      draggable: false,
      data: { objectType, violations: [], dimmed: false },
    }))
  }, [neighborhood])

  const edges = useMemo<LinkTypeEdge[]>(
    () =>
      neighborhood.link_types.map((linkType) => ({
        id: linkType.id,
        type: "ontologyLinkType" as const,
        source: linkType.source_object_type_id,
        target: linkType.target_object_type_id,
        markerEnd: { type: MarkerType.ArrowClosed },
        selectable: false,
        data: { linkType, violations: [], dimmed: false },
      })),
    [neighborhood]
  )

  return (
    <div className={cn("h-full w-full", styles.theme)}>
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        edgesFocusable={false}
        nodesFocusable={false}
        deleteKeyCode={null}
        fitView
        minZoom={0.2}
        proOptions={{ hideAttribution: false }}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={24}
          size={1.5}
          color="var(--border)"
          bgColor="transparent"
        />
        <Controls showInteractive={false} />
      </ReactFlow>
    </div>
  )
}
