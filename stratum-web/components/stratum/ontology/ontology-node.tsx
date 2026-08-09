"use client"

import { Handle, Position, type Node, type NodeProps } from "@xyflow/react"
import { CircleAlert } from "lucide-react"

import type { OntologyObjectType } from "@/features/ontology-editor/types"
import { cn } from "@/lib/utils"

/**
 * Object Type 画布节点：display_name + name + 属性计数；422 违例挂红框与
 * 首条消息（完整列表在编辑面板内联展示）；聚焦模式下非邻域节点淡出。
 */

export type ObjectTypeNodeData = {
  objectType: OntologyObjectType
  violations: readonly string[]
  dimmed: boolean
}

export type ObjectTypeNode = Node<ObjectTypeNodeData, "ontologyObjectType">

export function OntologyObjectTypeNode({
  data,
  selected,
}: NodeProps<ObjectTypeNode>) {
  const { objectType, violations, dimmed } = data
  const hasViolations = violations.length > 0

  return (
    <div
      className={cn(
        "w-60 rounded-xl border bg-card text-card-foreground shadow-[0_8px_30px] shadow-black/10 transition-opacity",
        selected ? "border-primary ring-2 ring-ring/30" : "border-border",
        hasViolations && "border-destructive ring-2 ring-destructive/30",
        dimmed && "opacity-30"
      )}
    >
      <Handle type="target" position={Position.Left} />
      <div className="border-b border-border px-3 py-2">
        <p className="truncate text-sm font-medium">{objectType.display_name}</p>
        <p className="truncate font-mono text-[0.6875rem] text-muted-foreground">
          {objectType.name}
        </p>
      </div>
      <div className="px-3 py-2 text-xs text-muted-foreground">
        {objectType.properties.length === 0
          ? "暂无属性"
          : `${objectType.properties.length} 个属性`}
      </div>
      {hasViolations && (
        <div className="flex items-start gap-1 border-t border-destructive/40 px-3 py-1.5 text-xs text-destructive">
          <CircleAlert aria-hidden className="mt-0.5 size-3.5 shrink-0" />
          <span>
            {violations[0]}
            {violations.length > 1 ? `（共 ${violations.length} 条）` : ""}
          </span>
        </div>
      )}
      <Handle type="source" position={Position.Right} />
    </div>
  )
}
