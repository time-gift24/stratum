"use client"

import { useState } from "react"

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
import { FieldRow } from "@/components/stratum/ontology/form-controls"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import type {
  OntologyLinkCardinality,
  OntologyObjectType,
} from "@/features/ontology-editor/types"
import { isValidOntologyName } from "@/features/ontology-editor/validation"

/**
 * Link Type 交互：连线（onConnect）后弹出创建对话框（name + 双向
 * cardinality）；选中边后的编辑收进画布右上悬浮工具条的 Popover
 * （ontology-chrome.tsx），不再有右侧常驻面板。
 */

const CARDINALITY_OPTIONS: readonly {
  value: OntologyLinkCardinality
  label: string
}[] = [
  { value: "one", label: "one（零或一）" },
  { value: "many", label: "many（零或多）" },
]

export function CardinalitySelect({
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
