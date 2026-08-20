"use client"

import {
  BaseEdge,
  EdgeLabelRenderer,
  getBezierPath,
  type Edge,
  type EdgeProps,
} from "@xyflow/react"

import type {
  OntologyLinkCardinality,
  OntologyLinkType,
  OntologyObjectType,
} from "@/features/ontology-editor/types"
import { PenLine, Trash2Icon } from "lucide-react"

import {
  CardIconButton,
  LinkTypeEditAction,
} from "@/components/stratum/ontology/ontology-chrome"
import { cn } from "@/lib/utils"

/**
 * Link Type 有向边：贝塞尔路径 + 两端 cardinality 标注（target 端标
 * source_to_target，source 端标 target_to_source；1 = 零或一，N = 零或多），
 * 中线标 display_name；选中时标签下方浮出边级动作（编辑 Popover / 删除），
 * 操作长在边上，不去侧栏。422 违例边描红并附首条消息。
 */

/** 边级动作回调（编辑更新 / 删除） */
export type LinkTypeEdgeActions = {
  onUpdate(next: OntologyLinkType): void
  onDelete(linkType: OntologyLinkType): void
}

export type LinkTypeEdgeData = {
  linkType: OntologyLinkType
  violations: readonly string[]
  dimmed: boolean
  /** 两端 Object Type（编辑 Popover 的源/目标展示） */
  source?: OntologyObjectType
  target?: OntologyObjectType
  /** 编辑画布传入；邻域只读画布省略 */
  edgeActions?: LinkTypeEdgeActions
}

export type LinkTypeEdge = Edge<LinkTypeEdgeData, "ontologyLinkType">

const CARDINALITY_TEXT: Record<OntologyLinkCardinality, string> = {
  one: "1",
  many: "N",
}

const CARDINALITY_TITLE: Record<OntologyLinkCardinality, string> = {
  one: "零或一（one）",
  many: "零或多（many）",
}

function CardinalityLabel({
  cardinality,
  x,
  y,
}: {
  cardinality: OntologyLinkCardinality
  x: number
  y: number
}) {
  return (
    <div
      style={{
        transform: `translate(-50%, -50%) translate(${x}px, ${y}px)`,
      }}
      className="pointer-events-none absolute"
    >
      <span
        title={CARDINALITY_TITLE[cardinality]}
        className="rounded-full border border-border bg-popover px-1 py-px font-mono text-[0.625rem] leading-none text-muted-foreground"
      >
        {CARDINALITY_TEXT[cardinality]}
      </span>
    </div>
  )
}

export function OntologyLinkTypeEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  markerEnd,
  data,
  selected,
}: EdgeProps<LinkTypeEdge>) {
  const [path, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    targetX,
    targetY,
    sourcePosition,
    targetPosition,
  })

  const violations = data?.violations ?? []
  // 两端标注向路径中点收拢，避免压到节点句柄
  const sourceLabelX = sourceX + (labelX - sourceX) * 0.2
  const sourceLabelY = sourceY + (labelY - sourceY) * 0.2
  const targetLabelX = targetX + (labelX - targetX) * 0.2
  const targetLabelY = targetY + (labelY - targetY) * 0.2

  return (
    <>
      <BaseEdge
        id={id}
        path={path}
        markerEnd={markerEnd}
        className={cn(violations.length > 0 && "edgeViolation")}
      />
      <EdgeLabelRenderer>
        {data !== undefined && (
          <>
            <CardinalityLabel
              cardinality={data.linkType.target_to_source}
              x={sourceLabelX}
              y={sourceLabelY}
            />
            <CardinalityLabel
              cardinality={data.linkType.source_to_target}
              x={targetLabelX}
              y={targetLabelY}
            />
            <div
              style={{
                transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
              }}
              className="pointer-events-none absolute flex flex-col items-center gap-0.5"
            >
              <span
                className={cn(
                  "rounded-full border bg-popover px-1.5 py-0.5 text-[0.625rem] leading-none dark:shadow-sm",
                  selected
                    ? "border-primary text-foreground"
                    : "border-border text-popover-foreground"
                )}
              >
                {data.linkType.display_name}
              </span>
              {selected && data.edgeActions !== undefined && (
                <div className="nodrag pointer-events-auto flex items-center gap-0.5 rounded-full border border-border bg-popover p-0.5 dark:shadow-sm">
                  <LinkTypeEditAction
                    linkType={data.linkType}
                    source={data.source}
                    target={data.target}
                    messages={violations}
                    onUpdate={data.edgeActions.onUpdate}
                    icon={<PenLine aria-hidden className="size-3" />}
                  />
                  <CardIconButton
                    label="删除该 Link Type"
                    tone="danger"
                    onClick={() => data.edgeActions?.onDelete(data.linkType)}
                  >
                    <Trash2Icon aria-hidden className="size-3" />
                  </CardIconButton>
                </div>
              )}
              {violations.length > 0 && (
                <span className="max-w-48 rounded-md border border-destructive/40 bg-popover px-1.5 py-0.5 text-[0.625rem] text-destructive">
                  {violations[0]}
                </span>
              )}
            </div>
          </>
        )}
      </EdgeLabelRenderer>
    </>
  )
}
