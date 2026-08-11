"use client"

import { useState, type CSSProperties } from "react"
import { Check, Plus, Trash2Icon, XIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  CommitInput,
  CommitTextarea,
  FieldRow,
} from "@/components/stratum/ontology/form-controls"
import type {
  OntologyObjectType,
  OntologyProperty,
  OntologyPropertyValueType,
} from "@/features/ontology-editor/types"
import { nodeHue } from "@/features/ontology-editor/hue"
import {
  nextPropertyName,
  PROPERTY_VALUE_TYPES,
  validatePropertyDisplayName,
  validatePropertyName,
} from "@/features/ontology-editor/property"
import { MAX_NEIGHBORHOOD_DEPTH } from "@/features/ontology-editor/neighborhood"
import { cn } from "@/lib/utils"

import auroraStyles from "./ontology-aurora.module.css"

/**
 * Object Type 编辑面板——方向契约：
 * THESIS：面板是选中节点的「规格表」——属性区是真实 <table>（sticky 表头
 * 声明列语义、# 序号栅格），不是卡片堆；身份头与画布节点共享同一
 * --node-hue 极光染色，表里表外是同一个对象。
 * HEADER：kicker（对象类型）+ 可编辑 display_name 标题 + name chip + 统计
 * （N 属性 · M 必填 · K 违例）；顶部衬 .aurora 磨砂染色。
 * TABLE：默认只读（名称 mono / 显示名 / 类型 badge / 必填勾选 / 序号），
 * 双击行进入编辑模式——该行变为 ghost 输入 + Select + 勾选 + 删除，
 * Escape 退出；422 违例以 colSpan 子行内联在对应属性下方。
 * FOOTER：固定底栏「+ 添加属性」自动命名；画布聚焦（深度 + 聚焦邻域）
 * 与安静的删除入口。删除 Object Type 本身由调用方弹确认对话框。
 */

/** 表格单元格的 ghost 输入：平时无边无底，悬停/聚焦才显现控件边界 */
const GHOST_CELL =
  "border-transparent bg-transparent px-1.5 shadow-none hover:border-input hover:bg-input/20 dark:bg-transparent dark:hover:bg-input/30"

export type PropertyInput = {
  name: string
  display_name: string
  description?: string
  value_type: OntologyPropertyValueType
  required: boolean
}

export function ObjectTypePanel({
  objectType,
  messages,
  propertyMessages,
  canAddProperty,
  propertyLimitMessage,
  focusDepth,
  onFocusDepthChange,
  onFocus,
  onUpdate,
  onDelete,
  onAddProperty,
  onUpdateProperty,
  onRemoveProperty,
  onClose,
}: {
  objectType: OntologyObjectType
  messages: readonly string[]
  propertyMessages: ReadonlyMap<string, readonly string[]>
  canAddProperty: boolean
  propertyLimitMessage: string | null
  focusDepth: number
  onFocusDepthChange(depth: number): void
  onFocus(): void
  onUpdate(next: OntologyObjectType): void
  onDelete(): void
  onAddProperty(input: PropertyInput): void
  onUpdateProperty(property: OntologyProperty): void
  onRemoveProperty(propertyId: string): void
  onClose(): void
}) {
  /** 双击进入编辑的属性行；Escape / 切换 Object Type 退出 */
  const [editingPropertyId, setEditingPropertyId] = useState<string | null>(
    null
  )
  // derive-during-render：切换选中对象时重置行编辑态
  const [prevObjectTypeId, setPrevObjectTypeId] = useState(objectType.id)
  if (objectType.id !== prevObjectTypeId) {
    setPrevObjectTypeId(objectType.id)
    setEditingPropertyId(null)
  }

  const addProperty = () => {
    const name = nextPropertyName(objectType.properties)
    onAddProperty({
      name,
      display_name: name,
      value_type: "string",
      required: false,
    })
  }
  const requiredCount = objectType.properties.filter(
    (property) => property.required
  ).length
  const violationCount =
    messages.length +
    objectType.properties.reduce(
      (count, property) =>
        count + (propertyMessages.get(property.id)?.length ?? 0),
      0
    )

  return (
    <aside
      aria-label={`Object Type ${objectType.display_name} 编辑面板`}
      className="flex h-full w-full flex-col overflow-hidden rounded-xl border border-border bg-popover text-popover-foreground shadow-[0_8px_30px] shadow-black/10"
      style={{ "--node-hue": nodeHue(objectType.id) } as CSSProperties}
    >
      {/* 身份头：与画布节点同色相的极光染色 + 标题/name/统计 */}
      <header className="relative border-b border-border px-3 pt-2.5 pb-3">
        <div
          aria-hidden
          className={cn(
            "pointer-events-none absolute inset-x-2 top-0 h-16 blur-xl",
            auroraStyles.aurora
          )}
        />
        <div className="relative flex items-center justify-between gap-2 px-1.5">
          <p className="font-mono text-[0.625rem] tracking-[0.2em] text-muted-foreground">
            对象类型
          </p>
          <Button
            variant="ghost"
            size="icon-sm"
            aria-label="关闭面板"
            onClick={onClose}
          >
            <XIcon />
          </Button>
        </div>
        <div className="relative mt-0.5">
          <CommitInput
            ariaLabel="Object Type 显示名"
            value={objectType.display_name}
            validate={validatePropertyDisplayName}
            onCommit={(displayName) =>
              onUpdate({ ...objectType, display_name: displayName })
            }
            className={cn(GHOST_CELL, "h-9 text-base font-medium")}
          />
        </div>
        <div className="relative mt-1 flex items-center gap-2 px-1.5">
          <code className="rounded border border-border/60 bg-muted/40 px-1.5 py-0.5 font-mono text-[0.6875rem] text-muted-foreground">
            {objectType.name}
          </code>
          <span className="text-[0.6875rem] text-muted-foreground">
            {objectType.properties.length} 属性 · {requiredCount} 必填
          </span>
          {violationCount > 0 && (
            <span className="text-[0.6875rem] text-destructive">
              {violationCount} 违例
            </span>
          )}
        </div>
      </header>

      {messages.length > 0 && (
        <div
          role="alert"
          className="mx-3 mt-2 rounded-lg border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-xs text-destructive"
        >
          {messages.map((message) => (
            <p key={message}>{message}</p>
          ))}
        </div>
      )}

      {/* 元信息：name / description 失焦提交（display_name 在身份头直接改） */}
      <div className="flex flex-col gap-2 border-b border-border px-4 py-3">
        <FieldRow label="名称（name）">
          <CommitInput
            mono
            ariaLabel="Object Type 名称"
            value={objectType.name}
            validate={validatePropertyName}
            onCommit={(name) => onUpdate({ ...objectType, name })}
          />
        </FieldRow>
        <FieldRow label="描述（description，可选）">
          <CommitTextarea
            ariaLabel="Object Type 描述"
            value={objectType.description ?? ""}
            placeholder="留空表示无描述"
            onCommit={(description) =>
              onUpdate({
                ...objectType,
                description: description === "" ? undefined : description,
              })
            }
          />
        </FieldRow>
      </div>

      {/* 属性规格表：sticky 表头 + # 序号栅格；默认只读，双击行进入编辑 */}
      <section
        aria-label="属性列表"
        className="flex min-h-0 flex-1 flex-col"
      >
        <h3 className="px-4 pt-3 pb-1 text-[0.6875rem] font-medium text-muted-foreground">
          属性（{objectType.properties.length}）
          <span className="ml-1.5 font-normal">· 双击行进入编辑</span>
        </h3>
        <div className="min-h-0 flex-1 overflow-y-auto">
          {/* border-separate：collapsed 边框在 sticky thead 上会丢失（Chrome），
              分隔线一律挂在 th/td 单元格上 */}
          <table className="w-full table-fixed border-separate border-spacing-0 text-xs">
            <thead className="sticky top-0 z-10 bg-popover">
              <tr className="text-[0.6875rem] text-muted-foreground">
                <th
                  scope="col"
                  className="w-7 border-b border-border px-1 py-1.5 text-right font-normal"
                >
                  #
                </th>
                <th
                  scope="col"
                  className="border-b border-border px-1.5 py-1.5 text-left font-medium"
                >
                  属性名
                </th>
                <th
                  scope="col"
                  className="w-[26%] border-b border-border px-1.5 py-1.5 text-left font-medium"
                >
                  显示名
                </th>
                <th
                  scope="col"
                  className="w-[6.25rem] border-b border-border px-1.5 py-1.5 text-left font-medium"
                >
                  类型
                </th>
                <th
                  scope="col"
                  className="w-10 border-b border-border px-1 py-1.5 text-center font-medium"
                >
                  必填
                </th>
                <th
                  scope="col"
                  className="w-8 border-b border-border"
                  aria-label="操作"
                />
              </tr>
            </thead>
            <tbody>
              {objectType.properties.length === 0 && (
                <tr>
                  <td
                    colSpan={6}
                    className="border-b border-border/50 px-4 py-6 text-center text-xs text-muted-foreground"
                  >
                    暂无属性，点击下方「添加属性」创建第一个字段
                  </td>
                </tr>
              )}
              {objectType.properties.map((property, index) => (
                <PropertyRow
                  key={property.id}
                  index={index + 1}
                  property={property}
                  messages={propertyMessages.get(property.id) ?? []}
                  editing={editingPropertyId === property.id}
                  onStartEdit={() => setEditingPropertyId(property.id)}
                  onExitEdit={() => setEditingPropertyId(null)}
                  onUpdate={(next) => onUpdateProperty(next)}
                  onRemove={() => onRemoveProperty(property.id)}
                />
              ))}
            </tbody>
          </table>
        </div>
        <div className="border-t border-border">
          <button
            type="button"
            disabled={!canAddProperty}
            onClick={addProperty}
            className="flex w-full items-center gap-1.5 px-4 py-2.5 text-xs text-muted-foreground transition-colors hover:bg-muted/40 hover:text-foreground disabled:pointer-events-none disabled:opacity-50"
          >
            <Plus aria-hidden className="size-3.5" />
            添加属性
          </button>
          {!canAddProperty && propertyLimitMessage !== null && (
            <p className="px-4 pb-2 text-[0.6875rem] text-muted-foreground">
              {propertyLimitMessage}
            </p>
          )}
        </div>
      </section>

      <div className="border-t border-border">
        <section
          aria-label="聚焦邻域"
          className="flex items-center gap-2 px-4 py-2.5"
        >
          <h3 className="shrink-0 text-[0.6875rem] font-medium text-muted-foreground">
            画布聚焦
          </h3>
          <Select
            value={focusDepth}
            onValueChange={(next) => onFocusDepthChange(next ?? 1)}
          >
            <SelectTrigger
              aria-label="聚焦深度"
              className="ml-auto w-24"
              size="sm"
            >
              <SelectValue />
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
          <Button variant="outline" size="sm" onClick={onFocus}>
            聚焦邻域
          </Button>
        </section>
        <div className="border-t border-border px-4 py-2">
          <Button
            variant="ghost"
            size="sm"
            className="w-full text-destructive hover:bg-destructive/10 hover:text-destructive"
            onClick={onDelete}
          >
            <Trash2Icon data-icon="inline-start" />
            删除该 Object Type
          </Button>
        </div>
      </div>
    </aside>
  )
}

/**
 * 属性行（表体行 + 违例子行），两态：
 * 只读态（默认）——# 序号 / 名称 mono / 显示名 / 类型 badge / 必填勾选图标，
 * 行悬停轻微反馈，双击整行进入编辑态；
 * 编辑态——ghost 输入（名称失焦提交并校验）/ ghost Select / 勾选 / 删除
 * 常显，Escape 退出编辑态。
 * 422 违例以 colSpan 子行内联在本行下方。
 */
function PropertyRow({
  index,
  property,
  messages,
  editing,
  onStartEdit,
  onExitEdit,
  onUpdate,
  onRemove,
}: {
  index: number
  property: OntologyProperty
  messages: readonly string[]
  editing: boolean
  onStartEdit(): void
  onExitEdit(): void
  onUpdate(next: OntologyProperty): void
  onRemove(): void
}) {
  const hasViolations = messages.length > 0
  const cellBorder = "border-b border-border/50"
  return (
    <>
      <tr
        tabIndex={editing ? -1 : 0}
        aria-label={`属性 ${property.name}，双击进入编辑`}
        onDoubleClick={onStartEdit}
        onKeyDown={(event) => {
          if (editing && event.key === "Escape") onExitEdit()
          if (!editing && event.key === "Enter") onStartEdit()
        }}
        className={cn(
          "outline-none transition-colors",
          editing ? "bg-muted/50" : "hover:bg-muted/30 focus-visible:bg-muted/30",
          hasViolations && "bg-destructive/5"
        )}
      >
        <td
          className={cn(
            cellBorder,
            "px-1 py-0.5 text-right font-mono text-[0.625rem] text-muted-foreground"
          )}
        >
          {index}
        </td>
        <td className={cn(cellBorder, "px-0.5 py-0.5")}>
          {editing ? (
            <CommitInput
              mono
              ariaLabel={`属性 ${property.name} 名称`}
              value={property.name}
              validate={validatePropertyName}
              onCommit={(name) => onUpdate({ ...property, name })}
              className={GHOST_CELL}
            />
          ) : (
            <span className="block truncate px-1.5 font-mono text-xs leading-7">
              {property.name}
            </span>
          )}
        </td>
        <td className={cn(cellBorder, "px-0.5 py-0.5")}>
          {editing ? (
            <CommitInput
              ariaLabel={`属性 ${property.name} 显示名`}
              value={property.display_name}
              validate={validatePropertyDisplayName}
              onCommit={(displayName) =>
                onUpdate({ ...property, display_name: displayName })
              }
              className={GHOST_CELL}
            />
          ) : (
            <span className="block truncate px-1.5 text-xs leading-7">
              {property.display_name}
            </span>
          )}
        </td>
        <td className={cn(cellBorder, "px-0.5 py-0.5")}>
          {editing ? (
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
                className={cn(
                  GHOST_CELL,
                  "w-full gap-1 font-mono text-[0.6875rem]"
                )}
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
          ) : (
            <span className="mx-1.5 inline-block rounded border border-border/60 bg-muted/40 px-1 font-mono text-[0.625rem] leading-5 text-muted-foreground">
              {property.value_type}
            </span>
          )}
        </td>
        <td className={cn(cellBorder, "px-1 py-0.5 text-center")}>
          {editing ? (
            <input
              type="checkbox"
              aria-label={`属性 ${property.name} 必填`}
              className="size-3.5 accent-primary"
              checked={property.required}
              onChange={(event) =>
                onUpdate({ ...property, required: event.target.checked })
              }
            />
          ) : property.required ? (
            <Check aria-label="必填" className="inline size-3.5 text-primary" />
          ) : (
            <span aria-hidden className="text-muted-foreground/60">
              —
            </span>
          )}
        </td>
        <td className={cn(cellBorder, "px-1 py-0.5")}>
          {editing && (
            <Button
              variant="ghost"
              size="icon-xs"
              aria-label={`删除属性 ${property.display_name}`}
              onClick={onRemove}
              className="text-muted-foreground hover:text-destructive"
            >
              <Trash2Icon />
            </Button>
          )}
        </td>
      </tr>
      {hasViolations && (
        <tr className="bg-destructive/5">
          <td colSpan={6} className={cn(cellBorder, "px-4 py-1.5")}>
            {messages.map((message) => (
              <p
                key={message}
                role="alert"
                className="text-[0.6875rem] text-destructive"
              >
                {message}
              </p>
            ))}
          </td>
        </tr>
      )}
    </>
  )
}
