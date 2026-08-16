"use client"

import { useState, type CSSProperties } from "react"
import { Handle, Position, type Node, type NodeProps } from "@xyflow/react"
import {
  CircleAlert,
  Crosshair,
  Plus,
  Trash2Icon,
} from "lucide-react"

import { CardIconButton } from "@/components/stratum/ontology/ontology-chrome"
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
import { MAX_NEIGHBORHOOD_DEPTH } from "@/features/ontology-editor/neighborhood"
import {
  nextPropertyName,
  PROPERTY_VALUE_TYPES,
  validatePropertyDisplayName,
  validatePropertyName,
} from "@/features/ontology-editor/property"
import { cn } from "@/lib/utils"

import styles from "./ontology-aurora.module.css"

/**
 * Object Type 画布节点（双层结构）：root 相对定位，玻璃背板承载头部
 * （display_name + name + 描述），背板顶部衬一层多色极光
 * （ontology-aurora.module.css 的 .aurora 三段渐变，色相由节点 ID 稳定散列为
 * --node-hue 注入容器，blur 化开形成磨砂染色，锚定 root 只漫在头部区域）。
 * 节点级操作长在卡片上：头部文本（display_name / name / description）双击
 * 即行内编辑（失焦提交 + 校验）；头部右侧动作组只剩 聚焦（图标 + 深度直选，
 * 选中即聚焦，无弹窗）与 删除，悬停或选中时显现，nodrag 不抢拖拽。
 * 内层实心面板承载属性列表——属性单行两列（name mono 与 display_name
 * 各自点击行内编辑，失焦提交+校验，两者独立）+ value_type 深色瓦片
 * Select + 必填勾选 + 悬停删除 + 底部虚线「添加属性」行。对象级与属性级 422 违例合并为底部红框首条 + 总数；
 * 聚焦模式下非邻域节点淡出。邻域只读画布省略全部动作。
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
  // 对象级 + 属性级 422 违例合并展示：底部红框首条 + 总数
  const allViolations = [
    ...violations,
    ...objectType.properties.flatMap(
      (property) => propertyMessages.get(property.id) ?? []
    ),
  ]
  const hasViolations = allViolations.length > 0
  const addPropertyDisabledReason =
    propertyActions?.getAddPropertyDisabledReason(objectType) ?? null
  /** 属性行内编辑：{ id, field }——name 与 display_name 两列各自独立编辑 */
  const [editingProperty, setEditingProperty] = useState<{
    id: string
    field: "name" | "display_name"
  } | null>(null)
  /** 头部文本的双击行内编辑：display_name / name / description */
  const [editingField, setEditingField] = useState<
    "display_name" | "name" | "description" | null
  >(null)
  const editableHeader = objectActions !== undefined
  const startEdit = (field: "display_name" | "name" | "description") =>
    editableHeader ? () => setEditingField(field) : undefined

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
    setEditingProperty({ id, field: "name" })
  }

  return (
    <div
      className={cn(
        "group relative w-72 rounded-2xl border bg-card/50 p-1.5 text-card-foreground shadow-[0_8px_30px] shadow-black/10 backdrop-blur-xl transition-opacity",
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
          {editingField === "display_name" && editableHeader ? (
            <CommitInput
              autoFocus
              ariaLabel="Object Type 显示名"
              value={objectType.display_name}
              validate={validatePropertyDisplayName}
              onCommit={(displayName) => {
                objectActions.onUpdate({ ...objectType, display_name: displayName })
                setEditingField(null)
              }}
            />
          ) : (
            <p
              onDoubleClick={startEdit("display_name")}
              title={editableHeader ? "双击修改显示名" : undefined}
              className="truncate text-sm font-medium"
            >
              {objectType.display_name}
            </p>
          )}
          {editingField === "name" && editableHeader ? (
            <CommitInput
              mono
              autoFocus
              ariaLabel="Object Type 名称"
              value={objectType.name}
              validate={validatePropertyName}
              onCommit={(name) => {
                objectActions.onUpdate({ ...objectType, name })
                setEditingField(null)
              }}
            />
          ) : (
            <p
              onDoubleClick={startEdit("name")}
              title={editableHeader ? "双击修改名称" : undefined}
              className="truncate font-mono text-[0.6875rem] text-muted-foreground"
            >
              {objectType.name}
            </p>
          )}
          {editingField === "description" && editableHeader ? (
            <CommitInput
              autoFocus
              ariaLabel="Object Type 描述"
              value={objectType.description ?? ""}
              onCommit={(description) => {
                objectActions.onUpdate({
                  ...objectType,
                  description: description === "" ? undefined : description,
                })
                setEditingField(null)
              }}
            />
          ) : objectType.description !== undefined &&
            objectType.description !== "" ? (
            <p
              onDoubleClick={startEdit("description")}
              title={editableHeader ? "双击修改描述" : undefined}
              className="truncate text-[0.6875rem] text-muted-foreground"
            >
              {objectType.description}
            </p>
          ) : null}
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
            {/* 聚焦邻域：图标 + 深度直选，选中即聚焦，无弹窗 */}
            <div className="flex items-center">
              <Crosshair
                aria-hidden
                className="size-3.5 shrink-0 text-muted-foreground"
              />
              <Select
                onValueChange={(depth) => {
                  if (typeof depth === "number")
                    objectActions.onFocus(objectType.id, depth)
                }}
              >
                <SelectTrigger
                  size="sm"
                  aria-label="聚焦邻域：选择深度"
                  className="nodrag nowheel h-7 w-auto min-w-[3.5rem] gap-0.5 border-0 bg-transparent px-1 text-xs shadow-none"
                >
                  <SelectValue>聚焦</SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {Array.from(
                    { length: MAX_NEIGHBORHOOD_DEPTH + 1 },
                    (_, depth) => (
                      <SelectItem key={depth} value={depth}>
                        深度 {depth}
                      </SelectItem>
                    )
                  )}
                </SelectContent>
              </Select>
            </div>
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
            editing={
              editingProperty?.id === property.id ? editingProperty.field : null
            }
            onStartEdit={(field) =>
              setEditingProperty({ id: property.id, field })
            }
            onFinishEdit={() => setEditingProperty(null)}
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
            {allViolations[0]}
            {allViolations.length > 1 ? `（共 ${allViolations.length} 条）` : ""}
          </span>
        </div>
      )}
      <Handle type="source" position={Position.Right} />
    </div>
  )
}

/**
 * 属性行（单行两列）：name（mono）与 display_name 两列都可点击行内编辑
 * （失焦提交 + 校验，两者独立、不再强制同值）；右侧 value_type 深色瓦片
 * Select + 必填勾选 + 悬停删除。无 onUpdate/onRemove（邻域只读画布）时
 * 整行只读。
 */
function NodePropertyRow({
  property,
  editing,
  onStartEdit,
  onFinishEdit,
  onUpdate,
  onRemove,
}: {
  property: OntologyProperty
  editing: "name" | "display_name" | null
  onStartEdit(field: "name" | "display_name"): void
  onFinishEdit(): void
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
      {editing === "name" ? (
        <div className="min-w-0 flex-1">
          <CommitInput
            mono
            autoFocus
            ariaLabel={`属性 ${property.name} 名称`}
            value={property.name}
            validate={validatePropertyName}
            onCommit={(name) => {
              onUpdate({ ...property, name })
              onFinishEdit()
            }}
          />
        </div>
      ) : (
        <button
          type="button"
          aria-label={`重命名属性 ${property.name}`}
          onClick={() => onStartEdit("name")}
          className="min-w-0 flex-1 truncate rounded px-0.5 text-left font-mono text-xs hover:bg-accent"
        >
          {property.name}
        </button>
      )}
      {editing === "display_name" ? (
        <div className="min-w-0 flex-1">
          <CommitInput
            autoFocus
            ariaLabel={`属性 ${property.name} 显示名`}
            value={property.display_name}
            validate={validatePropertyDisplayName}
            onCommit={(displayName) => {
              onUpdate({ ...property, display_name: displayName })
              onFinishEdit()
            }}
          />
        </div>
      ) : (
        <button
          type="button"
          aria-label={`修改属性 ${property.name} 显示名`}
          title="点击修改显示名"
          onClick={() => onStartEdit("display_name")}
          className="min-w-0 flex-1 truncate rounded px-0.5 text-left text-xs text-muted-foreground hover:bg-accent hover:text-foreground"
        >
          {property.display_name}
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
