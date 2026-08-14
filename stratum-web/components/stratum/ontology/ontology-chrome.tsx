"use client"

import Link from "next/link"
import {
  ArrowLeft,
  Crosshair,
  ListPlus,
  Loader2,
  Network,
  PenLine,
  Plus,
  Save,
  SquarePen,
  Trash2Icon,
  XIcon,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Popover,
  PopoverContent,
  PopoverDescription,
  PopoverHeader,
  PopoverTitle,
  PopoverTrigger,
} from "@/components/ui/popover"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  CommitInput,
  CommitTextarea,
  FieldRow,
} from "@/components/stratum/ontology/form-controls"
import { CardinalitySelect } from "@/components/stratum/ontology/link-type-dialog"
import type { ObjectTypePropertyDraft } from "@/components/stratum/ontology/ontology-node"
import { MAX_NEIGHBORHOOD_DEPTH } from "@/features/ontology-editor/neighborhood"
import {
  nextPropertyName,
  validatePropertyDisplayName,
  validatePropertyName,
} from "@/features/ontology-editor/property"
import { isValidOntologyName } from "@/features/ontology-editor/validation"
import type {
  OntologyLinkType,
  OntologyObjectType,
} from "@/features/ontology-editor/types"
import { cn } from "@/lib/utils"

/**
 * Ontology 画布的悬浮 chrome——方向契约：
 * 画布满铺，没有头部栏。所有控制收进浮在画布上的圆角 pill：
 * 左上 = 返回 + 标题 + 保存状态；右上 = 选中工具条（有选中时）+ 全局动作
 * （视图切换 / 新增 / 保存）。文字只保留标题与真实保存状态，其余一律图标
 * + tooltip；需要输入的编辑（改名、描述、cardinality、聚焦深度）收进图标
 * 弹出的 Popover，不再使用右侧常驻面板。
 */

/** 悬浮 pill 容器：popover 底 + 细边 + 投影，与画布节点同一浮层语言 */
function CanvasPill({
  className,
  children,
}: {
  className?: string
  children: React.ReactNode
}) {
  return (
    <div
      className={cn(
        "pointer-events-auto flex items-center gap-0.5 rounded-full border border-border bg-popover/95 p-1 shadow-[0_8px_30px] shadow-black/10 backdrop-blur",
        className
      )}
    >
      {children}
    </div>
  )
}

/** 图标动作：ghost 圆钮 + tooltip（tooltip 即无障碍名） */
function IconAction({
  label,
  active,
  disabled,
  onClick,
  children,
}: {
  label: string
  active?: boolean
  disabled?: boolean
  onClick?: () => void
  children: React.ReactNode
}) {
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger
          render={
            <Button
              type="button"
              variant="ghost"
              size="icon-sm"
              aria-label={label}
              aria-pressed={active}
              disabled={disabled}
              onClick={onClick}
              className={cn(
                "rounded-full",
                active && "bg-accent text-accent-foreground"
              )}
            />
          }
        >
          {children}
        </TooltipTrigger>
        <TooltipContent side="bottom">{label}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

/** 图标弹层动作：图标钮 + tooltip，点击弹出 Popover 表单 */
function IconPopover({
  label,
  children,
  content,
}: {
  label: string
  children: React.ReactNode
  content: React.ReactNode
}) {
  return (
    <Popover>
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger
            render={
              <PopoverTrigger
                render={
                  <Button
                    type="button"
                    variant="ghost"
                    size="icon-sm"
                    aria-label={label}
                    className="rounded-full"
                  />
                }
              />
            }
          >
            {children}
          </TooltipTrigger>
          <TooltipContent side="bottom">{label}</TooltipContent>
        </Tooltip>
      </TooltipProvider>
      <PopoverContent align="end" className="w-80">
        {content}
      </PopoverContent>
    </Popover>
  )
}

const pillIcon = "size-4"

/** 左上：返回 + Ontology 标题 + 真实保存状态（已保存时不占视觉） */
export function CanvasIdentityPill({
  displayName,
  dirty,
  inFlight,
}: {
  displayName: string
  dirty: boolean
  inFlight: boolean
}) {
  return (
    <CanvasPill className="max-w-[70vw]">
      <TooltipProvider>
        <Tooltip>
          <TooltipTrigger
            render={
              <Link
                href="/ontologies"
                aria-label="返回列表"
                className="flex size-8 items-center justify-center rounded-full text-muted-foreground outline-none transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:ring-2 focus-visible:ring-ring"
              />
            }
          >
            <ArrowLeft aria-hidden className={pillIcon} />
          </TooltipTrigger>
          <TooltipContent side="bottom">返回列表</TooltipContent>
        </Tooltip>
      </TooltipProvider>
      <span aria-hidden className="mx-1 h-4 w-px bg-border" />
      <span className="max-w-48 truncate px-1.5 text-xs font-medium">
        {displayName}
      </span>
      {inFlight ? (
        <span className="flex items-center gap-1.5 pr-1.5 text-xs text-muted-foreground">
          <Loader2
            aria-hidden
            className="size-3.5 animate-spin motion-reduce:animate-none"
          />
          保存中
        </span>
      ) : dirty ? (
        <span role="status" className="pr-1.5 text-xs text-muted-foreground">
          未保存
        </span>
      ) : null}
    </CanvasPill>
  )
}

/** 右上全局动作：视图切换（编辑/邻域）+ 新增 Object Type + 保存 */
export function CanvasGlobalPill({
  view,
  onViewChange,
  onAddObjectType,
  saveDisabled,
  inFlight,
  onSave,
}: {
  view: "edit" | "neighborhood"
  onViewChange(view: "edit" | "neighborhood"): void
  onAddObjectType(): void
  saveDisabled: boolean
  inFlight: boolean
  onSave(): void
}) {
  return (
    <CanvasPill>
      <IconAction
        label="编辑画布"
        active={view === "edit"}
        onClick={() => onViewChange("edit")}
      >
        <SquarePen aria-hidden className={pillIcon} />
      </IconAction>
      <IconAction
        label="邻域视图"
        active={view === "neighborhood"}
        onClick={() => onViewChange("neighborhood")}
      >
        <Network aria-hidden className={pillIcon} />
      </IconAction>
      {view === "edit" ? (
        <>
          <span aria-hidden className="mx-1 h-4 w-px bg-border" />
          <IconAction label="新增 Object Type" onClick={onAddObjectType}>
            <Plus aria-hidden className={pillIcon} />
          </IconAction>
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    type="button"
                    size="icon-sm"
                    aria-label="保存"
                    disabled={saveDisabled}
                    onClick={onSave}
                    className="rounded-full"
                  />
                }
              >
                {inFlight ? (
                  <Loader2
                    aria-hidden
                    className={cn(
                      pillIcon,
                      "animate-spin motion-reduce:animate-none"
                    )}
                  />
                ) : (
                  <Save aria-hidden className={pillIcon} />
                )}
              </TooltipTrigger>
              <TooltipContent side="bottom">保存</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </>
      ) : null}
    </CanvasPill>
  )
}

function ViolationList({ messages }: { messages: readonly string[] }) {
  if (messages.length === 0) return null
  return (
    <div
      role="alert"
      className="rounded-lg border border-destructive/40 bg-destructive/10 px-2 py-1.5 text-xs text-destructive"
    >
      {messages.map((message) => (
        <p key={message}>{message}</p>
      ))}
    </div>
  )
}

/** 选中 Object Type 的悬浮工具条：加属性 / 编辑详情 / 聚焦邻域 / 删除 / 取消选中 */
export function ObjectTypeSelectionPill({
  objectType,
  messages,
  propertyMessages,
  addPropertyDisabledReason,
  focusDepth,
  onFocusDepthChange,
  onFocus,
  onUpdate,
  onDelete,
  onAddProperty,
  onClearSelection,
}: {
  objectType: OntologyObjectType
  messages: readonly string[]
  propertyMessages: ReadonlyMap<string, readonly string[]>
  addPropertyDisabledReason: string | null
  focusDepth: number
  onFocusDepthChange(depth: number): void
  onFocus(): void
  onUpdate(next: OntologyObjectType): void
  onDelete(): void
  onAddProperty(input: ObjectTypePropertyDraft): void
  onClearSelection(): void
}) {
  const allMessages = [
    ...messages,
    ...objectType.properties.flatMap((property) =>
      (propertyMessages.get(property.id) ?? []).map(
        (message) => `${property.name}：${message}`
      )
    ),
  ]
  const addProperty = () => {
    if (addPropertyDisabledReason !== null) return
    const name = nextPropertyName(objectType.properties)
    onAddProperty({
      name,
      display_name: name,
      value_type: "string",
      required: false,
    })
  }

  return (
    <CanvasPill>
      <span className="max-w-32 truncate px-1.5 text-xs font-medium">
        {objectType.display_name}
      </span>
      <span aria-hidden className="mx-1 h-4 w-px bg-border" />
      <IconAction
        label={addPropertyDisabledReason ?? "添加属性"}
        disabled={addPropertyDisabledReason !== null}
        onClick={addProperty}
      >
        <ListPlus aria-hidden className={pillIcon} />
      </IconAction>
      <IconPopover
        label="编辑详情"
        content={
          <>
            <PopoverHeader>
              <PopoverTitle>{objectType.display_name}</PopoverTitle>
              <PopoverDescription className="font-mono">
                {objectType.name}
              </PopoverDescription>
            </PopoverHeader>
            <ViolationList messages={allMessages} />
            <FieldRow label="显示名（display_name）">
              <CommitInput
                ariaLabel="Object Type 显示名"
                value={objectType.display_name}
                validate={validatePropertyDisplayName}
                onCommit={(displayName) =>
                  onUpdate({ ...objectType, display_name: displayName })
                }
              />
            </FieldRow>
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
          </>
        }
      >
        <PenLine aria-hidden className={pillIcon} />
      </IconPopover>
      <IconPopover
        label="聚焦邻域"
        content={
          <>
            <PopoverHeader>
              <PopoverTitle>聚焦邻域</PopoverTitle>
              <PopoverDescription>
                只显示「{objectType.display_name}」及其指定深度内的邻居。
              </PopoverDescription>
            </PopoverHeader>
            <FieldRow label="深度">
              <Select
                value={focusDepth}
                onValueChange={(next) => onFocusDepthChange(next ?? 1)}
              >
                <SelectTrigger aria-label="聚焦深度" className="w-full">
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
            </FieldRow>
            <Button type="button" size="sm" onClick={onFocus}>
              聚焦
            </Button>
          </>
        }
      >
        <Crosshair aria-hidden className={pillIcon} />
      </IconPopover>
      <IconAction label="删除该 Object Type" onClick={onDelete}>
        <Trash2Icon aria-hidden className={pillIcon} />
      </IconAction>
      <span aria-hidden className="mx-1 h-4 w-px bg-border" />
      <IconAction label="取消选中" onClick={onClearSelection}>
        <XIcon aria-hidden className={pillIcon} />
      </IconAction>
    </CanvasPill>
  )
}

/** 选中 Link Type 的悬浮工具条：编辑（名称/显示名/双向 cardinality）/ 删除 / 取消选中 */
export function LinkTypeSelectionPill({
  linkType,
  source,
  target,
  messages,
  onUpdate,
  onDelete,
  onClearSelection,
}: {
  linkType: OntologyLinkType
  source: OntologyObjectType | undefined
  target: OntologyObjectType | undefined
  messages: readonly string[]
  onUpdate(next: OntologyLinkType): void
  onDelete(): void
  onClearSelection(): void
}) {
  return (
    <CanvasPill>
      <span className="max-w-32 truncate px-1.5 text-xs font-medium">
        {linkType.display_name}
      </span>
      <span aria-hidden className="mx-1 h-4 w-px bg-border" />
      <IconPopover
        label="编辑 Link Type"
        content={
          <>
            <PopoverHeader>
              <PopoverTitle>{linkType.display_name}</PopoverTitle>
              <PopoverDescription>
                {source?.display_name ?? "未知源"} →{" "}
                {target?.display_name ?? "未知目标"}
              </PopoverDescription>
            </PopoverHeader>
            <ViolationList messages={messages} />
            <FieldRow label="名称（name）">
              <CommitInput
                mono
                ariaLabel="Link Type 名称"
                value={linkType.name}
                validate={(next) =>
                  isValidOntologyName(next)
                    ? null
                    : "需匹配 ^[a-z][a-z0-9_]{0,63}$"
                }
                onCommit={(name) => onUpdate({ ...linkType, name })}
              />
            </FieldRow>
            <FieldRow label="显示名（display_name）">
              <CommitInput
                ariaLabel="Link Type 显示名"
                value={linkType.display_name}
                validate={(next) =>
                  next.trim() === "" ? "显示名不能为空" : null
                }
                onCommit={(displayName) =>
                  onUpdate({ ...linkType, display_name: displayName })
                }
              />
            </FieldRow>
            <FieldRow
              label={`源 → 目标（每个「${source?.display_name ?? "源"}」对应多少「${target?.display_name ?? "目标"}」）`}
            >
              <CardinalitySelect
                ariaLabel="源到目标 cardinality"
                value={linkType.source_to_target}
                onChange={(sourceToTarget) =>
                  onUpdate({ ...linkType, source_to_target: sourceToTarget })
                }
              />
            </FieldRow>
            <FieldRow
              label={`目标 → 源（每个「${target?.display_name ?? "目标"}」对应多少「${source?.display_name ?? "源"}」）`}
            >
              <CardinalitySelect
                ariaLabel="目标到源 cardinality"
                value={linkType.target_to_source}
                onChange={(targetToSource) =>
                  onUpdate({ ...linkType, target_to_source: targetToSource })
                }
              />
            </FieldRow>
          </>
        }
      >
        <PenLine aria-hidden className={pillIcon} />
      </IconPopover>
      <IconAction label="删除该 Link Type" onClick={onDelete}>
        <Trash2Icon aria-hidden className={pillIcon} />
      </IconAction>
      <span aria-hidden className="mx-1 h-4 w-px bg-border" />
      <IconAction label="取消选中" onClick={onClearSelection}>
        <XIcon aria-hidden className={pillIcon} />
      </IconAction>
    </CanvasPill>
  )
}
