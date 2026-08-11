"use client"

import { useState } from "react"
import { Trash2Icon, XIcon } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog"
import { Input } from "@/components/ui/input"
import {
  CommitInput,
  FieldRow,
} from "@/components/stratum/ontology/form-controls"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import type {
  OntologyLinkCardinality,
  OntologyLinkType,
  OntologyObjectType,
} from "@/features/ontology-editor/types"
import { isValidOntologyName } from "@/features/ontology-editor/validation"
import { cn } from "@/lib/utils"

/**
 * Link Type 交互：连线（onConnect）后弹出创建对话框（name + 双向
 * cardinality）；选中边后的编辑面板支持改名、调 cardinality 与删除。
 */

const CARDINALITY_OPTIONS: readonly {
  value: OntologyLinkCardinality
  label: string
}[] = [
  { value: "one", label: "one（零或一）" },
  { value: "many", label: "many（零或多）" },
]

function CardinalitySelect({
  ariaLabel,
  value,
  onChange,
}: {
  ariaLabel: string
  value: OntologyLinkCardinality
  onChange(next: OntologyLinkCardinality): void
}) {
  return (
    <Select
      value={value}
      onValueChange={(next) => onChange(next as OntologyLinkCardinality)}
    >
      <SelectTrigger aria-label={ariaLabel} className="w-full">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        {CARDINALITY_OPTIONS.map((option) => (
          <SelectItem key={option.value} value={option.value}>
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  )
}

export function LinkTypeDialog({
  pending,
  source,
  target,
  onCancel,
  onSubmit,
}: {
  pending: { sourceId: string; targetId: string } | null
  source: OntologyObjectType | undefined
  target: OntologyObjectType | undefined
  onCancel(): void
  onSubmit(input: {
    name: string
    display_name: string
    description?: string
    source_to_target: OntologyLinkCardinality
    target_to_source: OntologyLinkCardinality
  }): void
}) {
  const [name, setName] = useState("")
  const [displayName, setDisplayName] = useState("")
  const [sourceToTarget, setSourceToTarget] =
    useState<OntologyLinkCardinality>("many")
  const [targetToSource, setTargetToSource] =
    useState<OntologyLinkCardinality>("one")
  const [error, setError] = useState<string | null>(null)

  const reset = () => {
    setName("")
    setDisplayName("")
    setSourceToTarget("many")
    setTargetToSource("one")
    setError(null)
  }

  const submit = () => {
    if (!isValidOntologyName(name)) {
      setError("名称需匹配 ^[a-z][a-z0-9_]{0,63}$")
      return
    }
    if (displayName.trim() === "") {
      setError("显示名不能为空")
      return
    }
    onSubmit({
      name,
      display_name: displayName,
      source_to_target: sourceToTarget,
      target_to_source: targetToSource,
    })
    reset()
  }

  return (
    <Dialog
      open={pending !== null}
      onOpenChange={(open) => {
        if (!open) {
          reset()
          onCancel()
        }
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>新建 Link Type</DialogTitle>
          <DialogDescription>
            {source?.display_name ?? "源"} → {target?.display_name ?? "目标"}
            ，请选择双向 cardinality。
          </DialogDescription>
        </DialogHeader>
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault()
            submit()
          }}
        >
          <FieldRow label="名称（name）">
            <Input
              autoFocus
              aria-label="Link Type 名称"
              placeholder="owns_ticket"
              value={name}
              onChange={(event) => setName(event.target.value)}
              className="font-mono"
            />
          </FieldRow>
          <FieldRow label="显示名（display_name）">
            <Input
              aria-label="Link Type 显示名"
              placeholder="拥有工单"
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
            />
          </FieldRow>
          <FieldRow
            label={`源 → 目标（每个「${source?.display_name ?? "源"}」对应多少「${target?.display_name ?? "目标"}」）`}
          >
            <CardinalitySelect
              ariaLabel="源到目标 cardinality"
              value={sourceToTarget}
              onChange={setSourceToTarget}
            />
          </FieldRow>
          <FieldRow
            label={`目标 → 源（每个「${target?.display_name ?? "目标"}」对应多少「${source?.display_name ?? "源"}」）`}
          >
            <CardinalitySelect
              ariaLabel="目标到源 cardinality"
              value={targetToSource}
              onChange={setTargetToSource}
            />
          </FieldRow>
          {error !== null && (
            <p role="alert" className="text-xs text-destructive">
              {error}
            </p>
          )}
          <DialogFooter>
            <Button type="button" variant="outline" onClick={onCancel}>
              取消
            </Button>
            <Button type="submit">创建连线</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function LinkTypePanel({
  linkType,
  source,
  target,
  messages,
  onUpdate,
  onDelete,
  onClose,
}: {
  linkType: OntologyLinkType
  source: OntologyObjectType | undefined
  target: OntologyObjectType | undefined
  messages: readonly string[]
  onUpdate(next: OntologyLinkType): void
  onDelete(): void
  onClose(): void
}) {
  return (
    <aside
      aria-label={`Link Type ${linkType.display_name} 编辑面板`}
      className="flex h-full flex-col gap-3 overflow-y-auto rounded-xl border border-border bg-popover p-3 text-popover-foreground shadow-[0_8px_30px] shadow-black/10"
    >
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h2 className="truncate text-sm font-medium">
            {linkType.display_name}
          </h2>
          <p className="truncate font-mono text-[0.6875rem] text-muted-foreground">
            {linkType.name}
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

      <p className="text-xs text-muted-foreground">
        {source?.display_name ?? "未知源"} → {target?.display_name ?? "未知目标"}
      </p>

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
          validate={(next) => (next.trim() === "" ? "显示名不能为空" : null)}
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

      <div className="mt-auto border-t border-border pt-3">
        <Button
          variant="destructive"
          size="sm"
          className={cn("w-full")}
          onClick={onDelete}
        >
          <Trash2Icon data-icon="inline-start" />
          删除该 Link Type
        </Button>
      </div>
    </aside>
  )
}
