"use client"

import { useState, type CSSProperties } from "react"
import { Handle, Position, type Node, type NodeProps } from "@xyflow/react"
import { CircleAlert, Plus, Trash2Icon } from "lucide-react"

import { CommitInput } from "@/components/stratum/ontology/form-controls"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import type {
  OntologyObjectType,
  OntologyProperty,
  OntologyPropertyValueType,
} from "@/features/ontology-editor/types"
import { nodeHue } from "@/features/ontology-editor/hue"
import {
  nextPropertyName,
  PROPERTY_VALUE_TYPES,
  validatePropertyName,
} from "@/features/ontology-editor/property"
import { cn } from "@/lib/utils"

import styles from "./ontology-aurora.module.css"

/**
 * Object Type 画布节点（双层结构）：玻璃背板承载头部（display_name + name +
 * 描述），背板顶部衬一层多色极光（ontology-aurora.module.css 的 .aurora 三段
 * 渐变，色相由节点 ID 稳定散列为 --node-hue 注入容器，每节点不同，blur 化开
 * 形成磨砂染色，仅漫在头部区域）；内层实心面板承载属性列表——属性行内直接
 * 增删改（改名失焦提交、value_type shadcn Select、必填勾选、悬停删除、
 * 底部虚线「添加属性」行）。
 * 422 违例挂红框与首条消息（完整列表在编辑面板内联展示）；聚焦模式下非邻域
 * 节点淡出。编辑画布经 propertyActions 传入增删改回调；邻域只读画布省略，
 * 属性行退化为只读。
 */

export type ObjectTypePropertyDraft = {
  name: string
  display_name: string
  description?: string
  value_type: OntologyPropertyValueType
  required: boolean
}

/** 属性增删改回调，与 use-ontology-editor 的对应方法同签名 */
export type ObjectTypePropertyActions = {
  onAddProperty(objectTypeId: string, input: ObjectTypePropertyDraft): string
  onUpdateProperty(objectTypeId: string, property: OntologyProperty): void
  onRemoveProperty(objectTypeId: string, propertyId: string): void
}

export type ObjectTypeNodeData = {
  objectType: OntologyObjectType
  violations: readonly string[]
  dimmed: boolean
  /** 编辑画布传入；邻域只读画布省略（属性行只读、无添加行） */
  propertyActions?: ObjectTypePropertyActions
}

export type ObjectTypeNode = Node<ObjectTypeNodeData, "ontologyObjectType">

export function OntologyObjectTypeNode({
  data,
  selected,
}: NodeProps<ObjectTypeNode>) {
  const { objectType, violations, dimmed, propertyActions } = data
  const hasViolations = violations.length > 0
  const [renamingId, setRenamingId] = useState<string | null>(null)

  const addProperty = () => {
    if (propertyActions === undefined) return
    const name = nextPropertyName(objectType.properties)
    const id = propertyActions.onAddProperty(objectType.id, {
      name,
      display_name: name,
      value_type: "string",
      required: false,
    })
    // 新建后直接进入行内改名
    setRenamingId(id)
  }

  return (
    <div
      className={cn(
        "w-64 rounded-2xl border bg-card/50 p-1.5 text-card-foreground shadow-[0_8px_30px] shadow-black/10 backdrop-blur-xl transition-opacity",
        selected ? "border-primary ring-2 ring-ring/30" : "border-border",
        hasViolations && "border-destructive ring-2 ring-destructive/30",
        dimmed && "opacity-30"
      )}
      style={{ "--node-hue": nodeHue(objectType.id) } as CSSProperties}
    >
      <Handle type="target" position={Position.Left} />
      {/* 顶部极光：色相按节点 ID 散列（--node-hue），blur 化开只漫在头部 */}
      <div
        aria-hidden
        className={cn(
          "pointer-events-none absolute inset-x-3 top-0 h-14 rounded-t-2xl blur-xl",
          styles.aurora
        )}
      />
      <header className="relative px-1.5 pt-0.5 pb-1.5">
        <p className="truncate text-sm font-medium">{objectType.display_name}</p>
        <p className="truncate font-mono text-[0.6875rem] text-muted-foreground">
          {objectType.name}
        </p>
        {objectType.description !== undefined &&
          objectType.description !== "" && (
            <p className="truncate text-[0.6875rem] text-muted-foreground">
              {objectType.description}
            </p>
          )}
      </header>
      <div className="nodrag relative flex flex-col gap-0.5 rounded-xl border border-border/50 bg-popover px-1.5 py-1.5 text-popover-foreground">
        {objectType.properties.length === 0 && (
          <p className="px-1 py-0.5 text-[0.6875rem] text-muted-foreground">
            暂无属性
          </p>
        )}
        {objectType.properties.map((property) => (
          <NodePropertyRow
            key={property.id}
            property={property}
            renaming={renamingId === property.id}
            onStartRename={() => setRenamingId(property.id)}
            onFinishRename={() => setRenamingId(null)}
            onUpdate={
              propertyActions === undefined
                ? undefined
                : (next) =>
                    propertyActions.onUpdateProperty(objectType.id, next)
            }
            onRemove={
              propertyActions === undefined
                ? undefined
                : () =>
                    propertyActions.onRemoveProperty(objectType.id, property.id)
            }
          />
        ))}
        {propertyActions !== undefined && (
          <button
            type="button"
            onClick={addProperty}
            className="mt-0.5 flex w-full items-center justify-center gap-1 rounded-md border border-dashed border-border px-2 py-1 text-[0.6875rem] text-muted-foreground hover:border-foreground/40 hover:text-foreground"
          >
            <Plus aria-hidden className="size-3" />
            添加属性
          </button>
        )}
      </div>
      {hasViolations && (
        <div className="flex items-start gap-1 px-1.5 pt-1.5 pb-0.5 text-xs text-destructive">
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

/**
 * 属性行：display_name 主文本 + mono value_type + 必填标记。
 * 有 onUpdate/onRemove（编辑画布）时行内可改：点名字进入失焦提交的改名输入，
 * value_type shadcn Select、必填 checkbox、删除按钮悬停显现；否则整行只读。
 * 节点层改名以 name 为准（需通过命名校验），display_name 同步为同值；
 * 两者需要不同时走编辑面板分别修改。
 */
function NodePropertyRow({
  property,
  renaming,
  onStartRename,
  onFinishRename,
  onUpdate,
  onRemove,
}: {
  property: OntologyProperty
  renaming: boolean
  onStartRename(): void
  onFinishRename(): void
  onUpdate?(next: OntologyProperty): void
  onRemove?(): void
}) {
  const editable = onUpdate !== undefined && onRemove !== undefined

  if (!editable) {
    return (
      <div className="flex items-center gap-1.5 px-1 py-0.5">
        <span className="min-w-0 flex-1 truncate font-mono text-xs">
          {property.name}
        </span>
        <span className="shrink-0 rounded border border-border/60 bg-muted/40 px-1 font-mono text-[0.5625rem] text-muted-foreground">
          {property.value_type}
        </span>
        {property.required && (
          <span className="shrink-0 text-[0.5625rem] text-muted-foreground">
            必填
          </span>
        )}
      </div>
    )
  }

  return (
    <div className="group flex items-center gap-1 px-1 py-0.5">
      {renaming ? (
        <div className="min-w-0 flex-1">
          <CommitInput
            mono
            autoFocus
            ariaLabel={`属性 ${property.name} 名称`}
            value={property.name}
            validate={validatePropertyName}
            onCommit={(name) => {
              onUpdate({ ...property, name, display_name: name })
              onFinishRename()
            }}
          />
        </div>
      ) : (
        <button
          type="button"
          aria-label={`重命名属性 ${property.name}`}
          onClick={onStartRename}
          className="min-w-0 flex-1 truncate rounded px-0.5 text-left font-mono text-xs hover:bg-accent"
        >
          {property.name}
        </button>
      )}
      <Select
        value={property.value_type}
        onValueChange={(valueType) =>
          onUpdate({
            ...property,
            value_type: valueType as OntologyPropertyValueType,
          })
        }
      >
        <SelectTrigger
          size="sm"
          aria-label={`属性 ${property.name} 值类型`}
          className="nodrag nowheel w-auto shrink-0 gap-1 px-1.5 py-0 font-mono text-[0.625rem]"
        >
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          {PROPERTY_VALUE_TYPES.map((valueType) => (
            <SelectItem
              key={valueType}
              value={valueType}
              className="font-mono text-[0.6875rem]"
            >
              {valueType}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
      <input
        type="checkbox"
        aria-label={`属性 ${property.name} 必填`}
        className="size-3.5 shrink-0 accent-primary"
        checked={property.required}
        onChange={(event) =>
          onUpdate({ ...property, required: event.target.checked })
        }
      />
      <button
        type="button"
        aria-label={`删除属性 ${property.display_name}`}
        onClick={onRemove}
        className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 group-hover:opacity-100 hover:text-destructive focus-visible:opacity-100 motion-safe:transition-opacity"
      >
        <Trash2Icon aria-hidden className="size-3.5" />
      </button>
    </div>
  )
}
