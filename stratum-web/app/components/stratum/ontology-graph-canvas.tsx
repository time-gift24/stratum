"use client"

import { useEffect, useMemo } from "react"
import {
  Background,
  BackgroundVariant,
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
} from "@xyflow/react"
import "@xyflow/react/dist/style.css"
import { MinusIcon, PlusIcon, ScanIcon } from "lucide-react"
import { useTranslation } from "react-i18next"
import { useReducedMotion } from "motion/react"

import { OntologyNode } from "~/components/stratum/ontology-node"
import type { OntologyGraph } from "~/lib/ontology-api"
import {
  createOntologyFlow,
  type OntologyFlowEdge,
  type OntologyFlowNode,
  type OntologySelection,
} from "~/lib/ontology-graph"

const nodeTypes = { ontology: OntologyNode }

type OntologyGraphCanvasProps = {
  graph: OntologyGraph
  selection: OntologySelection
  onSelectionChange(selection: OntologySelection): void
}

function CanvasInner({
  graph,
  selection,
  onSelectionChange,
}: OntologyGraphCanvasProps) {
  const { t } = useTranslation()
  const reduceMotion = useReducedMotion()
  const flow = useMemo(() => createOntologyFlow(graph), [graph])
  const nodes = useMemo(
    () =>
      flow.nodes.map((node) => ({
        ...node,
        selected: selection?.kind === "node" && selection.id === node.id,
      })),
    [flow.nodes, selection]
  )
  const edges = useMemo(
    () =>
      flow.edges.map((edge) => ({
        ...edge,
        selected: selection?.kind === "edge" && selection.id === edge.id,
      })),
    [flow.edges, selection]
  )
  const { fitView, zoomIn, zoomOut } = useReactFlow<
    OntologyFlowNode,
    OntologyFlowEdge
  >()

  useEffect(() => {
    if (!selection) return
    const targets =
      selection.kind === "node"
        ? nodes.filter((node) => node.id === selection.id)
        : edges
            .filter((edge) => edge.id === selection.id)
            .flatMap((edge) =>
              nodes.filter(
                (node) => node.id === edge.source || node.id === edge.target
              )
            )
    if (targets.length === 0) return
    void fitView({
      nodes: targets,
      padding: 0.8,
      duration: reduceMotion ? 0 : 180,
    })
  }, [edges, fitView, nodes, reduceMotion, selection])

  const duration = reduceMotion ? 0 : 180

  return (
    <div
      className="relative h-full min-h-0 w-full bg-wyse-canvas"
      role="region"
      aria-label={t("ontology.canvas.label")}
    >
      <ReactFlow<OntologyFlowNode, OntologyFlowEdge>
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        fitView
        fitViewOptions={{ padding: 0.3 }}
        minZoom={0.2}
        maxZoom={2}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable
        nodesFocusable
        edgesFocusable
        onNodeClick={(_, node) =>
          onSelectionChange({ kind: "node", id: node.id })
        }
        onEdgeClick={(_, edge) =>
          onSelectionChange({ kind: "edge", id: edge.id })
        }
        onPaneClick={() => onSelectionChange(null)}
      >
        <Background
          variant={BackgroundVariant.Dots}
          gap={18}
          size={1}
          color="var(--wyse-line-strong)"
        />
      </ReactFlow>
      <div className="absolute bottom-3 left-3 z-10 flex rounded-md border border-wyse-line bg-wyse-paper p-0.5">
        <button
          type="button"
          className="grid size-11 place-items-center rounded-sm hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
          aria-label={t("ontology.canvas.zoomOut")}
          onClick={() => void zoomOut({ duration })}
        >
          <MinusIcon className="size-4" aria-hidden="true" />
        </button>
        <button
          type="button"
          className="grid size-11 place-items-center rounded-sm hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
          aria-label={t("ontology.canvas.zoomIn")}
          onClick={() => void zoomIn({ duration })}
        >
          <PlusIcon className="size-4" aria-hidden="true" />
        </button>
        <button
          type="button"
          className="grid size-11 place-items-center rounded-sm hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
          aria-label={t("ontology.canvas.fitView")}
          onClick={() => void fitView({ padding: 0.3, duration })}
        >
          <ScanIcon className="size-4" aria-hidden="true" />
        </button>
      </div>
    </div>
  )
}

export function OntologyGraphCanvas(props: OntologyGraphCanvasProps) {
  return (
    <ReactFlowProvider>
      <CanvasInner {...props} />
    </ReactFlowProvider>
  )
}
