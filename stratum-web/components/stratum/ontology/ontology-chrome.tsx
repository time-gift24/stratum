"use client"

import Link from "next/link"
import { Loader2 } from "lucide-react"

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
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip"
import {
  CommitInput,
  FieldRow,
} from "@/components/stratum/ontology/form-controls"
import { CardinalitySelect } from "@/components/stratum/ontology/link-type-dialog"
import { isValidOntologyName } from "@/features/ontology-editor/validation"
import type {
  OntologyLinkType,
  OntologyObjectType,
} from "@/features/ontology-editor/types"
import { cn } from "@/lib/utils"

/**
 * Ontology 画布的悬浮 chrome 原语——方向契约（对齐暗色节点编辑器参考）：
 * 画布满铺。顶部：左右两枚悬浮 pill——左 = 返回 + 标题 + 保存状态，
 * 右 = 视图切换 / 新增（图标 + 分隔线收进一枚 pill）+ 独立的主操作 pill
 * （保存：有脏数据时实心 primary，图标+文字，全页最显眼）。
 * 节点/边的操作不长在任何侧栏——直接长在卡片上：节点头部文本双击即
 * 行内编辑，聚焦是图标 + 深度直选（选中即聚焦，无弹窗）；CardIconButton /
 * CardIconPopover 是卡片内小图标动作（nodrag，tooltip 向上），边的编辑
 * 弹层（LinkTypeEditAction）是唯一保留的 Popover 表单。
 * 组装方式：编辑器用 pill 族组合顶部栏；节点/边组件用 card 族组合
 * 卡片内动作；本文件不含整段装配。不用任何常驻面板或侧边按钮列。
 */

const pillIcon = "size-4"

// ---------------------------------------------------------------------------
// 顶部 pill 族
// ---------------------------------------------------------------------------

/** 顶部悬浮 pill 容器：深色圆角全胶囊 + 细边 + 投影 + backdrop-blur */
export function ChromePill({
  className,
  children,
}: {
  className?: string
  children: React.ReactNode
}) {
  return (
    <div
      className={cn(
        "pointer-events-auto flex h-10 items-center gap-0.5 rounded-full border border-border bg-card/95 px-1.5 shadow-xl backdrop-blur",
        className
      )}
    >
      {children}
    </div>
  )
}

export function PillDivider() {
  return <span aria-hidden className="mx-1 h-4 w-px bg-border" />
}

/** pill 内图标钮：ghost 圆钮 + tooltip（side=bottom，tooltip 即无障碍名） */
export function PillIconButton({
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
                "rounded-full text-muted-foreground hover:bg-muted/60 hover:text-foreground",
                active &&
                  "bg-primary/15 text-primary hover:bg-primary/15 hover:text-primary"
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

/** pill 内链接（返回等导航）：与 PillIconButton 同一几何 */
export function PillLinkButton({
  label,
  href,
  children,
}: {
  label: string
  href: string
  children: React.ReactNode
}) {
  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger
          render={
            <Link
              href={href}
              aria-label={label}
              className="flex size-8 items-center justify-center rounded-full text-muted-foreground outline-none transition-colors hover:bg-muted/60 hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
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

/**
 * 主操作 pill（保存）：独立实心胶囊，图标 + 文字。
 * 可行动（有脏数据）时实心 primary——参考里白色 Share 钮的视觉权重；
 * 不可行动时退回安静 pill，不抢注意力。
 */
export function PrimaryPillButton({
  label,
  loading,
  disabled,
  onClick,
  children,
}: {
  label: string
  loading?: boolean
  disabled?: boolean
  onClick?: () => void
  children: React.ReactNode
}) {
  const actionable = !disabled && !loading
  return (
    <Button
      type="button"
      aria-label={label}
      disabled={disabled || loading}
      onClick={onClick}
      className={cn(
        "pointer-events-auto h-10 gap-1.5 rounded-full px-4 text-sm font-medium shadow-xl",
        actionable
          ? "bg-primary text-primary-foreground hover:bg-primary/90"
          : "border border-border bg-card/95 text-muted-foreground backdrop-blur"
      )}
    >
      {loading ? (
        <Loader2
          aria-hidden
          className={cn(pillIcon, "animate-spin motion-reduce:animate-none")}
        />
      ) : (
        children
      )}
      {label}
    </Button>
  )
}

// ---------------------------------------------------------------------------
// 卡片内动作族（节点头 / 边标签）
// ---------------------------------------------------------------------------

type CardTone = "default" | "danger"

const cardToneClass: Record<CardTone, string> = {
  default: "text-muted-foreground hover:bg-muted/60 hover:text-foreground",
  danger: "text-muted-foreground hover:bg-destructive/10 hover:text-destructive",
}

/** 卡片内小图标动作：size-7 + nodrag（不触发节点拖拽）+ tooltip 向上 */
export function CardIconButton({
  label,
  tone = "default",
  disabled,
  onClick,
  children,
}: {
  label: string
  tone?: CardTone
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
              size="icon-xs"
              aria-label={label}
              disabled={disabled}
              onClick={onClick}
              className={cn("nodrag size-7 rounded-md", cardToneClass[tone])}
            />
          }
        >
          {children}
        </TooltipTrigger>
        <TooltipContent side="top">{label}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}

/** 卡片内小图标弹层：点击弹出 Popover 表单（默认向下右对齐，nodrag） */
export function CardIconPopover({
  label,
  side = "bottom",
  align = "end",
  children,
  content,
}: {
  label: string
  side?: "top" | "bottom" | "left" | "right"
  align?: "start" | "center" | "end"
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
                    size="icon-xs"
                    aria-label={label}
                    className={cn(
                      "nodrag size-7 rounded-md",
                      cardToneClass.default
                    )}
                  />
                }
              />
            }
          >
            {children}
          </TooltipTrigger>
          <TooltipContent side="top">{label}</TooltipContent>
        </Tooltip>
      </TooltipProvider>
      <PopoverContent side={side} align={align} className="nodrag w-80">
        {content}
      </PopoverContent>
    </Popover>
  )
}

// ---------------------------------------------------------------------------
// 自包含 Popover 动作（表单与校验逻辑封装在此，图标由调用方传入）
// ---------------------------------------------------------------------------

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

/** Link Type 编辑动作：CardIconPopover + name/显示名/双向 cardinality 表单 */
export function LinkTypeEditAction({
  linkType,
  source,
  target,
  messages,
  onUpdate,
  icon,
}: {
  linkType: OntologyLinkType
  source: OntologyObjectType | undefined
  target: OntologyObjectType | undefined
  messages: readonly string[]
  onUpdate(next: OntologyLinkType): void
  icon: React.ReactNode
}) {
  return (
    <CardIconPopover
      label={`编辑 Link Type（${linkType.display_name}）`}
      side="top"
      align="center"
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
      {icon}
    </CardIconPopover>
  )
}
