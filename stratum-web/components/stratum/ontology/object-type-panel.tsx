"use client"

import { useState } from "react"
import { Plus, Trash2Icon, XIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import { Input } from "@/components/ui/input"
import {
  CommitInput,
  CommitTextarea,
  FieldRow,
  nativeSelectClassName,
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
 * Object Type 编辑面板：name / display_name / description 失焦提交；
 * properties 行内管理（value_type 六选枚举 + required 开关）；422 违例
 * 在对象级与属性行内联展示。删除由调用方弹确认对话框。
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
  return (
    <aside
      aria-label={`Object Type ${objectType.display_name} 编辑面板`}
      className="flex h-full flex-col gap-3 overflow-y-auto rounded-xl border border-border bg-popover p-3 text-popover-foreground shadow-[0_8px_30px] shadow-black/10"
    >
      <div className="flex items-start justify-between gap-2">
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
          className="rounded-lg border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-xs text-destructive"
        >
          {messages.map((message) => (
            <p key={message}>{message}</p>
          ))}
        </div>
      )}

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

      <section aria-label="属性列表" className="flex flex-col gap-2">
        <h3 className="text-[0.6875rem] font-medium text-muted-foreground">
          属性（{objectType.properties.length}）
        </h3>
        {objectType.properties.map((property) => (
          <PropertyRow
            key={property.id}
            property={property}
            messages={propertyMessages.get(property.id) ?? []}
            onUpdate={(next) => onUpdateProperty(next)}
            onRemove={() => onRemoveProperty(property.id)}
          />
        ))}
        <AddPropertyForm
          disabled={!canAddProperty}
          limitMessage={propertyLimitMessage}
          onAdd={onAddProperty}
        />
      </section>

      <section aria-label="聚焦邻域" className="flex flex-col gap-1.5">
        <h3 className="text-[0.6875rem] font-medium text-muted-foreground">
          画布聚焦
        </h3>
        <div className="flex items-center gap-2">
          <select
            aria-label="聚焦深度"
            className={cn(nativeSelectClassName, "w-24")}
            value={focusDepth}
            onChange={(event) =>
              onFocusDepthChange(Number(event.target.value))
            }
          >
            {Array.from(
              { length: MAX_NEIGHBORHOOD_DEPTH + 1 },
              (_, depth) => (
                <option key={depth} value={depth}>
                  深度 {depth}
                </option>
              )
            )}
          </select>
          <Button variant="outline" size="sm" onClick={onFocus}>
            聚焦邻域
          </Button>
        </div>
        <p className="text-[0.6875rem] text-muted-foreground">
          聚焦基于本地草稿计算，未保存的新节点与连线也会计入。
        </p>
      </section>

      <div className="mt-auto border-t border-border pt-3">
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
    <div
      className={cn(
        "flex flex-col gap-1.5 rounded-lg border px-2 py-1.5",
        hasViolations ? "border-destructive/50" : "border-border"
      )}
    >
      <div className="flex items-center gap-1.5">
        <CommitInput
          mono
          ariaLabel={`属性 ${property.name} 名称`}
          value={property.name}
          validate={validateName}
          onCommit={(name) => onUpdate({ ...property, name })}
        />
        <Button
          variant="ghost"
          size="icon-xs"
          aria-label={`删除属性 ${property.display_name}`}
          onClick={onRemove}
        >
          <Trash2Icon />
        </Button>
      </div>
      <CommitInput
        ariaLabel={`属性 ${property.name} 显示名`}
        value={property.display_name}
        validate={validateDisplayName}
        onCommit={(displayName) =>
          onUpdate({ ...property, display_name: displayName })
        }
      />
      <div className="flex items-center gap-2">
        <select
          aria-label={`属性 ${property.name} 值类型`}
          className={nativeSelectClassName}
          value={property.value_type}
          onChange={(event) =>
            onUpdate({
              ...property,
              value_type: event.target.value as OntologyPropertyValueType,
            })
          }
        >
          {VALUE_TYPES.map((valueType) => (
            <option key={valueType} value={valueType}>
              {valueType}
            </option>
          ))}
        </select>
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
      </div>
      {messages.map((message) => (
        <p key={message} role="alert" className="text-[0.6875rem] text-destructive">
          {message}
        </p>
      ))}
    </div>
  )
}

function AddPropertyForm({
  disabled,
  limitMessage,
  onAdd,
}: {
  disabled: boolean
  limitMessage: string | null
  onAdd(input: PropertyInput): void
}) {
  const [name, setName] = useState("")
  const [displayName, setDisplayName] = useState("")
  const [valueType, setValueType] =
    useState<OntologyPropertyValueType>("string")
  const [required, setRequired] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const submit = () => {
    const nameError = validateName(name)
    if (nameError !== null) {
      setError(nameError)
      return
    }
    if (displayName.trim() === "") {
      setError("显示名不能为空")
      return
    }
    setError(null)
    onAdd({ name, display_name: displayName, value_type: valueType, required })
    setName("")
    setDisplayName("")
    setValueType("string")
    setRequired(false)
  }

  return (
    <div className="flex flex-col gap-1.5 rounded-lg border border-dashed border-border px-2 py-2">
      <div className="flex items-center gap-1.5">
        <Input
          aria-label="新属性名称"
          placeholder="name"
          value={name}
          onChange={(event) => setName(event.target.value)}
          className="font-mono"
          disabled={disabled}
        />
        <Input
          aria-label="新属性显示名"
          placeholder="显示名"
          value={displayName}
          onChange={(event) => setDisplayName(event.target.value)}
          disabled={disabled}
        />
      </div>
      <div className="flex items-center gap-2">
        <select
          aria-label="新属性值类型"
          className={nativeSelectClassName}
          value={valueType}
          onChange={(event) =>
            setValueType(event.target.value as OntologyPropertyValueType)
          }
          disabled={disabled}
        >
          {VALUE_TYPES.map((option) => (
            <option key={option} value={option}>
              {option}
            </option>
          ))}
        </select>
        <label className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
          <input
            type="checkbox"
            className="size-3.5 accent-primary"
            checked={required}
            onChange={(event) => setRequired(event.target.checked)}
            disabled={disabled}
          />
          必填
        </label>
        <Button
          variant="outline"
          size="sm"
          onClick={submit}
          disabled={disabled}
        >
          <Plus data-icon="inline-start" />
          添加属性
        </Button>
      </div>
      {error !== null && (
        <p role="alert" className="text-[0.6875rem] text-destructive">
          {error}
        </p>
      )}
      {disabled && limitMessage !== null && (
        <p className="text-[0.6875rem] text-muted-foreground">{limitMessage}</p>
      )}
    </div>
  )
}
