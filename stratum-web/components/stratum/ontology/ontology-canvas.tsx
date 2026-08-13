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

// 共享空违例列表 / 边箭头：避免每次 derive 分配新对象，保持 node/edge data 引用稳定
const EMPTY_MESSAGES: readonly string[] = []
const EDGE_MARKER_END = { type: MarkerType.ArrowClosed } as const

export type CanvasSelection = {
  kind: "objectType" | "linkType"
  id: string
} | null

type CanvasMessages = ReadonlyMap<string, readonly string[]>

// reducer 保证未触碰的 objectType/linkType 引用不变；对象复用发生在下方的
// derive-during-render 重同步里（拿新 derive 结果与渲染中的 node/edge 比对）。
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
      violations: objectViolations.get(objectType.id) ?? EMPTY_MESSAGES,
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
  return linkTypes.map((linkType) => {
    const dimmed = focus !== null && !focus.linkTypeIds.has(linkType.id)
    return {
      id: linkType.id,
      type: "ontologyLinkType" as const,
      source: linkType.source_object_type_id,
      target: linkType.target_object_type_id,
      markerEnd: EDGE_MARKER_END,
      className: cn(dimmed && "edgeDimmed"),
      data: {
        linkType,
        violations: linkViolations.get(linkType.id) ?? EMPTY_MESSAGES,
        dimmed,
      },
    }
  })
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
    [document.link_types, focus, linkViolations]
  )

  // 本地渲染态：拖拽/框选等交互即时应用；外部数据变化时重建并保留内部选中态
  // （derive-during-render，与 site-chrome 同约定，避免 effect 内同步 setState）
  const [nodes, setNodes] = useState(baseNodes)
  const [edges, setEdges] = useState(baseEdges)
  const [prevBase, setPrevBase] = useState({
    nodes: baseNodes,
    edges: baseEdges,
  })

  if (baseNodes !== prevBase.nodes) {
    setPrevBase({ nodes: baseNodes, edges: baseEdges })
    const beforeById = new Map(nodes.map((entry) => [entry.id, entry]))
    setNodes(
      baseNodes.map((node) => {
        const before = beforeById.get(node.id)
        if (before === undefined) return node
        // 输入（实体/违例数组引用、dimmed、propertyActions、坐标）全部一致时
        // 复用渲染中的对象，顺带保留 selected 与本地测量/拖拽态；
        // 否则仅把内部选中态搬到新对象上
        if (
          before.data.objectType === node.data.objectType &&
          before.data.violations === node.data.violations &&
          before.data.dimmed === node.data.dimmed &&
          before.data.propertyActions === node.data.propertyActions &&
          before.position.x === node.position.x &&
          before.position.y === node.position.y
        )
          return before
        return before.selected ? { ...node, selected: true } : node
      })
    )
  }
  if (baseEdges !== prevBase.edges) {
    setPrevBase({ nodes: baseNodes, edges: baseEdges })
    const beforeById = new Map(edges.map((entry) => [entry.id, entry]))
    setEdges(
      baseEdges.map((edge) => {
        const before = beforeById.get(edge.id)
        const beforeData = before?.data
        const data = edge.data
        if (
          before === undefined ||
          beforeData === undefined ||
          data === undefined
        )
          return edge
        if (
          beforeData.linkType === data.linkType &&
          beforeData.violations === data.violations &&
          beforeData.dimmed === data.dimmed
        )
          return before
        return before.selected ? { ...edge, selected: true } : edge
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
        onlyRenderVisibleElements
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
      data: { objectType, violations: EMPTY_MESSAGES, dimmed: false },
    }))
  }, [neighborhood])

  const edges = useMemo<LinkTypeEdge[]>(
    () =>
      neighborhood.link_types.map((linkType) => ({
        id: linkType.id,
        type: "ontologyLinkType" as const,
        source: linkType.source_object_type_id,
        target: linkType.target_object_type_id,
        markerEnd: EDGE_MARKER_END,
        selectable: false,
        data: { linkType, violations: EMPTY_MESSAGES, dimmed: false },
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
        onlyRenderVisibleElements
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
