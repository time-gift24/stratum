"use client"

import { useState, type CSSProperties } from "react"
import { Handle, Position, type Node, type NodeProps } from "@xyflow/react"
import {
  CircleAlert,
  Crosshair,
  PenLine,
  Plus,
  Trash2Icon,
} from "lucide-react"

import {
  CardIconButton,
  FocusNeighborhoodAction,
  ObjectTypeDetailsAction,
} from "@/components/stratum/ontology/ontology-chrome"
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
 * Object Type 画布节点（双层结构）：root 相对定位，玻璃背板承载头部
 * （display_name + name + 描述），背板顶部衬一层多色极光
 * （ontology-aurora.module.css 的 .aurora 三段渐变，色相由节点 ID 稳定散列为
 * --node-hue 注入容器，blur 化开形成磨砂染色，锚定 root 只漫在头部区域）。
 * 节点级操作长在卡片上：头部右侧动作组（详情 Popover / 聚焦 Popover /
 * 删除），悬停或选中时显现，nodrag 不抢拖拽。内层实心面板承载属性列表——
 * 属性是双层行（display_name 主文本 + name mono 次级），两行都可点击进入
 * 失焦提交的行内改名；value_type shadcn Select、必填勾选、悬停删除、
 * 底部虚线「添加属性」行。422 违例挂红框与首条消息（完整列表在详情
 * Popover）；聚焦模式下非邻域节点淡出。邻域只读画布省略全部动作。
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
  getAddPropertyDisabledReason(objectType: OntologyObjectType): string | null
  onAddProperty(objectTypeId: string, input: ObjectTypePropertyDraft): string
  onUpdateProperty(objectTypeId: string, property: OntologyProperty): void
  onRemoveProperty(objectTypeId: string, propertyId: string): void
}

/** 节点级动作回调（详情更新 / 请求删除 / 聚焦邻域） */
export type ObjectTypeNodeActions = {
  onUpdate(next: OntologyObjectType): void
  onRequestDelete(objectType: OntologyObjectType): void
  onFocus(objectTypeId: string, depth: number): void
}

export type ObjectTypeNodeData = {
  objectType: OntologyObjectType
  violations: readonly string[]
  /** 属性级 422 违例：property.id → 消息列表 */
  propertyMessages: ReadonlyMap<string, readonly string[]>
  dimmed: boolean
  /** 编辑画布传入；邻域只读画布省略（属性行只读、无添加行） */
  propertyActions?: ObjectTypePropertyActions
  /** 编辑画布传入；邻域只读画布省略（无头部动作组） */
  objectActions?: ObjectTypeNodeActions
}

export type ObjectTypeNode = Node<ObjectTypeNodeData, "ontologyObjectType">

export function OntologyObjectTypeNode({
  data,
  selected,
}: NodeProps<ObjectTypeNode>) {
  const { objectType, violations, propertyMessages, dimmed, propertyActions, objectActions } =
    data
  const hasViolations = violations.length > 0
  const addPropertyDisabledReason =
    propertyActions?.getAddPropertyDisabledReason(objectType) ?? null
  const [renamingId, setRenamingId] = useState<string | null>(null)

  const addProperty = () => {
    if (propertyActions === undefined || addPropertyDisabledReason !== null)
      return
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
        "group relative w-64 rounded-2xl border bg-card/50 p-1.5 text-card-foreground shadow-[0_8px_30px] shadow-black/10 backdrop-blur-xl transition-opacity",
        selected ? "border-primary ring-2 ring-ring/30" : "border-border",
        hasViolations && "border-destructive ring-2 ring-destructive/30",
        dimmed && "opacity-30"
      )}
      style={{ "--node-hue": nodeHue(objectType.id) } as CSSProperties}
    >
      <Handle type="target" position={Position.Left} />
      {/* 顶部极光：锚定 root（relative），blur 化开只漫在头部 */}
      <div
        aria-hidden
        className={cn(
          "pointer-events-none absolute inset-x-3 top-0 h-14 rounded-t-2xl blur-xl",
          styles.aurora
        )}
      />
      <header className="relative flex items-start gap-1 px-1.5 pt-0.5 pb-1.5">
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">
            {objectType.display_name}
          </p>
          <p className="truncate font-mono text-[0.6875rem] text-muted-foreground">
            {objectType.name}
          </p>
          {objectType.description !== undefined &&
            objectType.description !== "" && (
              <p className="truncate text-[0.6875rem] text-muted-foreground">
                {objectType.description}
              </p>
            )}
        </div>
        {objectActions !== undefined && (
          <div
            className={cn(
              "flex shrink-0 items-center gap-0.5 transition-opacity motion-reduce:transition-none",
              selected
                ? "opacity-100"
                : "opacity-0 group-hover:opacity-100 focus-within:opacity-100"
            )}
          >
            <ObjectTypeDetailsAction
              objectType={objectType}
              messages={violations}
              propertyMessages={propertyMessages}
              onUpdate={objectActions.onUpdate}
              icon={<PenLine aria-hidden className="size-3.5" />}
            />
            <FocusNeighborhoodAction
              objectType={objectType}
              onFocus={(depth) => objectActions.onFocus(objectType.id, depth)}
              icon={<Crosshair aria-hidden className="size-3.5" />}
            />
            <CardIconButton
              label="删除该 Object Type"
              tone="danger"
              onClick={() => objectActions.onRequestDelete(objectType)}
            >
              <Trash2Icon aria-hidden className="size-3.5" />
            </CardIconButton>
          </div>
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
            disabled={addPropertyDisabledReason !== null}
            title={addPropertyDisabledReason ?? undefined}
            onClick={addProperty}
            className="mt-0.5 flex w-full items-center justify-center gap-1 rounded-md border border-dashed border-border px-2 py-1 text-[0.6875rem] text-muted-foreground hover:border-foreground/40 hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:border-border disabled:hover:text-muted-foreground"
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
 * 属性行（单行）：name mono 主文本（点击进入行内改名，失焦提交并校验，
 * display_name 同步为同值——一处填写两者一致，需要不同值时走头部详情
 * Popover）；右侧 value_type 深色瓦片 Select + 必填勾选 + 悬停删除。
 * 无 onUpdate/onRemove（邻域只读画布）时整行只读。
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
    <div className="group/row flex items-center gap-1 rounded-md px-1 py-0.5">
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
          className="nodrag nowheel h-7 w-[5.25rem] shrink-0 gap-1 rounded-md border-0 bg-muted/70 px-1.5 font-mono text-[0.625rem] shadow-none"
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
        className="shrink-0 rounded p-0.5 text-muted-foreground opacity-0 group-hover/row:opacity-100 hover:text-destructive focus-visible:opacity-100 motion-safe:transition-opacity"
      >
        <Trash2Icon aria-hidden className="size-3.5" />
      </button>
    </div>
  )
}
