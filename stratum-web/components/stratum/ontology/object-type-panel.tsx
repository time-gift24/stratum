"use client"

import { Plus, Trash2Icon, XIcon } from "lucide-react"

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
import { isValidOntologyName } from "@/features/ontology-editor/validation"
import { MAX_NEIGHBORHOOD_DEPTH } from "@/features/ontology-editor/neighborhood"
import { cn } from "@/lib/utils"

/**
 * Object Type 编辑面板（宽列表）：头部 display_name / name + 关闭；
 * name / display_name / description 失焦提交；属性区一属性一行——
 * name（mono）· display_name · value_type（Select）· 必填 · 删除全部
 * 在单行内完成，行间 divide-y、悬停反馈；底部「+ 添加属性」行自动命名。
 * 422 违例在对象级与属性行下方内联展示。删除由调用方弹确认对话框。
 */

const NAME_HINT = "需匹配 ^[a-z][a-z0-9_]{0,63}$（小写字母开头，可含数字与下划线）"

const VALUE_TYPES: readonly OntologyPropertyValueType[] = [
  "string",
  "integer",
  "number",
  "boolean",
  "date",
  "date_time",
]

function validateName(next: string): string | null {
  return isValidOntologyName(next) ? null : NAME_HINT
}

function validateDisplayName(next: string): string | null {
  return next.trim() === "" ? "显示名不能为空" : null
}

/** 新属性的自动命名：field_n，取不冲突的最小序号 */
function nextPropertyName(properties: readonly OntologyProperty[]): string {
  const taken = new Set(properties.map((property) => property.name))
  let index = properties.length + 1
  while (taken.has(`field_${index}`)) index += 1
  return `field_${index}`
}

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
  const addProperty = () => {
    const name = nextPropertyName(objectType.properties)
    onAddProperty({
      name,
      display_name: name,
      value_type: "string",
      required: false,
    })
  }

  return (
    <aside
      aria-label={`Object Type ${objectType.display_name} 编辑面板`}
      className="flex h-full w-full flex-col overflow-y-auto rounded-xl border border-border bg-popover text-popover-foreground shadow-[0_8px_30px] shadow-black/10"
    >
      {/* 列表头：display_name / name + 关闭 */}
      <div className="flex items-start justify-between gap-2 px-4 pt-3 pb-2">
        <div className="min-w-0">
          <h2 className="truncate text-sm font-medium">
            {objectType.display_name}
          </h2>
          <p className="truncate font-mono text-[0.6875rem] text-muted-foreground">
            {objectType.name}
          </p>
        </div>
        <Button
          variant="ghost"
          size="icon-sm"
          aria-label="关闭面板"
          onClick={onClose}
        >
          <XIcon />
        </Button>
      </div>

      {messages.length > 0 && (
        <div
          role="alert"
          className="mx-4 mb-2 rounded-lg border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-xs text-destructive"
        >
          {messages.map((message) => (
            <p key={message}>{message}</p>
          ))}
        </div>
      )}

      {/* 元信息：name / display_name / description 失焦提交 */}
      <div className="flex flex-col gap-2 border-b border-border px-4 pb-3">
        <FieldRow label="名称（name）">
          <CommitInput
            mono
            ariaLabel="Object Type 名称"
            value={objectType.name}
            validate={validateName}
            onCommit={(name) => onUpdate({ ...objectType, name })}
          />
        </FieldRow>
        <FieldRow label="显示名（display_name）">
          <CommitInput
            ariaLabel="Object Type 显示名"
            value={objectType.display_name}
            validate={validateDisplayName}
            onCommit={(displayName) =>
              onUpdate({ ...objectType, display_name: displayName })
            }
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

      {/* 属性列表：一属性一行，行内完成全部编辑 */}
      <section aria-label="属性列表" className="flex flex-col">
        <h3 className="px-4 pt-3 pb-1 text-[0.6875rem] font-medium text-muted-foreground">
          属性（{objectType.properties.length}）
        </h3>
        <div className="flex flex-col divide-y divide-border/60">
          {objectType.properties.length === 0 && (
            <p className="px-4 py-2 text-xs text-muted-foreground">暂无属性</p>
          )}
          {objectType.properties.map((property) => (
            <PropertyRow
              key={property.id}
              property={property}
              messages={propertyMessages.get(property.id) ?? []}
              onUpdate={(next) => onUpdateProperty(next)}
              onRemove={() => onRemoveProperty(property.id)}
            />
          ))}
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

      <section
        aria-label="聚焦邻域"
        className="flex flex-col gap-1.5 border-t border-border px-4 py-3"
      >
        <h3 className="text-[0.6875rem] font-medium text-muted-foreground">
          画布聚焦
        </h3>
        <div className="flex items-center gap-2">
          <Select
            value={focusDepth}
            onValueChange={(next) => onFocusDepthChange(next ?? 1)}
          >
            <SelectTrigger aria-label="聚焦深度" className="w-24">
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
        </div>
        <p className="text-[0.6875rem] text-muted-foreground">
          聚焦基于本地草稿计算，未保存的新节点与连线也会计入。
        </p>
      </section>

      <div className="mt-auto border-t border-border px-4 py-3">
        <Button
          variant="destructive"
          size="sm"
          className="w-full"
          onClick={onDelete}
        >
          <Trash2Icon data-icon="inline-start" />
          删除该 Object Type
        </Button>
      </div>
    </aside>
  )
}

/**
 * 属性行：name（mono）· display_name · value_type（Select）· 必填 · 删除
 * 全部单行内完成；行级悬停反馈，422 违例内联在行下方。
 */
function PropertyRow({
  property,
  messages,
  onUpdate,
  onRemove,
}: {
  property: OntologyProperty
  messages: readonly string[]
  onUpdate(next: OntologyProperty): void
  onRemove(): void
}) {
  const hasViolations = messages.length > 0
  return (
    <div className={cn("flex flex-col", hasViolations && "bg-destructive/5")}>
      <div className="flex items-center gap-2 px-4 py-1.5 transition-colors hover:bg-muted/40">
        <div className="min-w-0 flex-1">
          <CommitInput
            mono
            ariaLabel={`属性 ${property.name} 名称`}
            value={property.name}
            validate={validateName}
            onCommit={(name) => onUpdate({ ...property, name })}
          />
        </div>
        <div className="min-w-0 flex-1">
          <CommitInput
            ariaLabel={`属性 ${property.name} 显示名`}
            value={property.display_name}
            validate={validateDisplayName}
            onCommit={(displayName) =>
              onUpdate({ ...property, display_name: displayName })
            }
          />
        </div>
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
            className="w-28 shrink-0 font-mono text-[0.6875rem]"
          >
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {VALUE_TYPES.map((valueType) => (
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
        <label className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
          <input
            type="checkbox"
            className="size-3.5 accent-primary"
            checked={property.required}
            onChange={(event) =>
              onUpdate({ ...property, required: event.target.checked })
            }
          />
          必填
        </label>
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={`删除属性 ${property.display_name}`}
          onClick={onRemove}
          className="shrink-0 text-muted-foreground hover:text-destructive"
        >
          <Trash2Icon />
        </Button>
      </div>
      {messages.length > 0 && (
        <div className="flex flex-col gap-0.5 px-4 pb-1.5">
          {messages.map((message) => (
            <p
              key={message}
              role="alert"
              className="text-[0.6875rem] text-destructive"
            >
              {message}
            </p>
          ))}
        </div>
      )}
    </div>
  )
}
