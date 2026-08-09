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
import { Textarea } from "@/components/ui/textarea"
import { FieldRow } from "@/components/stratum/ontology/form-controls"
import type {
  OntologyLinkType,
  OntologyObjectType,
} from "@/features/ontology-editor/types"
import { isValidOntologyName } from "@/features/ontology-editor/validation"

/**
 * Object Type 对话框：新增（name 内联正则校验）与删除确认
 * （被 Link Type 引用时列出并提示将一并移除）。
 */

export function AddObjectTypeDialog({
  open,
  onOpenChange,
  onSubmit,
}: {
  open: boolean
  onOpenChange(open: boolean): void
  onSubmit(input: {
    name: string
    display_name: string
    description?: string
  }): void
}) {
  const [name, setName] = useState("")
  const [displayName, setDisplayName] = useState("")
  const [description, setDescription] = useState("")
  const [error, setError] = useState<string | null>(null)

  const reset = () => {
    setName("")
    setDisplayName("")
    setDescription("")
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
      ...(description.trim() === "" ? {} : { description }),
    })
    reset()
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) reset()
        onOpenChange(next)
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>新增 Object Type</DialogTitle>
          <DialogDescription>
            新建的 Object Type 只写入本地草稿，点击「保存」后才会提交。
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
              aria-label="Object Type 名称"
              placeholder="customer"
              value={name}
              onChange={(event) => setName(event.target.value)}
              className="font-mono"
            />
          </FieldRow>
          <FieldRow label="显示名（display_name）">
            <Input
              aria-label="Object Type 显示名"
              placeholder="客户"
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
            />
          </FieldRow>
          <FieldRow label="描述（可选）">
            <Textarea
              aria-label="Object Type 描述"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
            />
          </FieldRow>
          {error !== null && (
            <p role="alert" className="text-xs text-destructive">
              {error}
            </p>
          )}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => onOpenChange(false)}
            >
              取消
            </Button>
            <Button type="submit">创建</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}

export function DeleteObjectTypeDialog({
  target,
  onCancel,
  onConfirm,
}: {
  target: {
    objectType: OntologyObjectType
    referencingLinks: readonly OntologyLinkType[]
  } | null
  onCancel(): void
  onConfirm(): void
}) {
  const referencing = target?.referencingLinks ?? []
  return (
    <Dialog
      open={target !== null}
      onOpenChange={(open) => {
        if (!open) onCancel()
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            删除 Object Type「{target?.objectType.display_name}」？
          </DialogTitle>
          <DialogDescription>
            {referencing.length > 0
              ? `该类型仍被 ${referencing.length} 条 Link Type 引用，删除后将一并移除这些关联及其画布位置。`
              : "删除后将同时移除其画布位置。该操作只影响本地草稿，保存后生效。"}
          </DialogDescription>
        </DialogHeader>
        {referencing.length > 0 && (
          <ul className="max-h-40 overflow-y-auto rounded-lg border border-border px-3 py-2 text-xs">
            {referencing.map((linkType) => (
              <li key={linkType.id} className="py-0.5">
                {linkType.display_name}
                <span className="ml-1 font-mono text-muted-foreground">
                  {linkType.name}
                </span>
              </li>
            ))}
          </ul>
        )}
        {referencing.length > 0 && (
          <p className="text-xs text-muted-foreground">
            将一并移除 {referencing.length} 条关联。
          </p>
        )}
        <DialogFooter>
          <Button variant="outline" onClick={onCancel}>
            取消
          </Button>
          <Button variant="destructive" onClick={onConfirm}>
            删除
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
