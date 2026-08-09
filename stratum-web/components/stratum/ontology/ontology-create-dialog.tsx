"use client"

import { useState } from "react"
import { useRouter } from "next/navigation"

import { isValidOntologyName } from "@/features/ontology-editor/validation"
import { ApiError, type StratumApi } from "@/lib/stratum/api"
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

/**
 * 新建本体对话框。name 客户端先行校验（不匹配正则不发请求）；
 * 409 ontology_name_conflict 内联到名称字段，表单内容保留；
 * 201 成功后跳转编辑器页 /ontologies/[id]（编辑器自行加载资源与 ETag）。
 */

export type OntologyCreateDialogProps = {
  api: StratumApi
  open: boolean
  onOpenChange(open: boolean): void
}

export function OntologyCreateDialog({
  api,
  open,
  onOpenChange,
}: OntologyCreateDialogProps) {
  const router = useRouter()
  const [name, setName] = useState("")
  const [displayName, setDisplayName] = useState("")
  const [description, setDescription] = useState("")
  const [nameError, setNameError] = useState<string | null>(null)
  const [displayNameError, setDisplayNameError] = useState<string | null>(null)
  const [formError, setFormError] = useState<string | null>(null)
  const [submitting, setSubmitting] = useState(false)

  const reset = () => {
    setName("")
    setDisplayName("")
    setDescription("")
    setNameError(null)
    setDisplayNameError(null)
    setFormError(null)
    setSubmitting(false)
  }

  const handleOpenChange = (nextOpen: boolean) => {
    if (!nextOpen) reset()
    onOpenChange(nextOpen)
  }

  const handleSubmit = (event: React.FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (submitting) return

    const trimmedName = name.trim()
    const trimmedDisplayName = displayName.trim()
    const trimmedDescription = description.trim()

    // 客户端先行校验：任何一项不合法都不发起请求
    const nextNameError = isValidOntologyName(trimmedName)
      ? null
      : "名称需以小写字母开头，仅含小写字母、数字、下划线，最长 64 字符"
    const nextDisplayNameError =
      trimmedDisplayName.length === 0
        ? "请输入显示名"
        : trimmedDisplayName.length > 200
          ? "显示名最长 200 字符"
          : null
    setNameError(nextNameError)
    setDisplayNameError(nextDisplayNameError)
    setFormError(null)
    if (nextNameError !== null || nextDisplayNameError !== null) return

    setSubmitting(true)
    void api
      .createOntology({
        name: trimmedName,
        displayName: trimmedDisplayName,
        ...(trimmedDescription === ""
          ? {}
          : { description: trimmedDescription }),
      })
      .then((resource) => {
        handleOpenChange(false)
        router.push(`/ontologies/${resource.document.id}`)
      })
      .catch((error: unknown) => {
        setSubmitting(false)
        if (
          error instanceof ApiError &&
          error.code === "ontology_name_conflict"
        ) {
          setNameError("该名称已被占用，请换一个名称")
          return
        }
        setFormError(
          error instanceof ApiError
            ? `创建失败：${error.message}`
            : "创建失败：无法连接到 Stratum 后端"
        )
      })
  }

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>新建本体</DialogTitle>
          <DialogDescription>
            创建后进入画布编辑器，定义对象类型与关系。
          </DialogDescription>
        </DialogHeader>
        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="ontology-create-name"
              className="text-xs font-medium"
            >
              名称
            </label>
            <Input
              id="ontology-create-name"
              value={name}
              onChange={(event) => {
                setName(event.target.value)
                setNameError(null)
              }}
              placeholder="support_domain"
              autoComplete="off"
              aria-invalid={nameError !== null}
              aria-describedby={
                nameError !== null ? "ontology-create-name-error" : undefined
              }
              className="font-mono"
            />
            {nameError !== null ? (
              <p
                id="ontology-create-name-error"
                className="text-xs text-destructive"
              >
                {nameError}
              </p>
            ) : (
              <p className="text-xs text-muted-foreground">
                英文标识：小写字母开头，仅含小写字母、数字、下划线。
              </p>
            )}
          </div>
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="ontology-create-display-name"
              className="text-xs font-medium"
            >
              显示名
            </label>
            <Input
              id="ontology-create-display-name"
              value={displayName}
              onChange={(event) => {
                setDisplayName(event.target.value)
                setDisplayNameError(null)
              }}
              placeholder="客服域"
              aria-invalid={displayNameError !== null}
              aria-describedby={
                displayNameError !== null
                  ? "ontology-create-display-name-error"
                  : undefined
              }
            />
            {displayNameError !== null ? (
              <p
                id="ontology-create-display-name-error"
                className="text-xs text-destructive"
              >
                {displayNameError}
              </p>
            ) : null}
          </div>
          <div className="flex flex-col gap-1.5">
            <label
              htmlFor="ontology-create-description"
              className="text-xs font-medium"
            >
              描述（可选）
            </label>
            <Textarea
              id="ontology-create-description"
              value={description}
              onChange={(event) => setDescription(event.target.value)}
              placeholder="这个本体描述的业务领域"
            />
          </div>
          {formError !== null ? (
            <p role="alert" className="text-xs text-destructive">
              {formError}
            </p>
          ) : null}
          <DialogFooter>
            <Button
              type="button"
              variant="outline"
              onClick={() => handleOpenChange(false)}
              disabled={submitting}
            >
              取消
            </Button>
            <Button type="submit" disabled={submitting}>
              {submitting ? "创建中…" : "创建"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
