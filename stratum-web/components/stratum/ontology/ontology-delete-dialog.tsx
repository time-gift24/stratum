"use client"

import { useState } from "react"

import type { OntologySummary } from "@/features/ontology-editor/types"
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

/**
 * 删除本体确认对话框（ontology 为 null 时关闭）。
 * 契约要求 If-Match 携带当前 ETag：确认后先 GET 读取最新 ETag 再 DELETE。
 * 取消不发起任何请求；412 提示资源已被他人修改并刷新列表；404 视为已删除。
 */

export type OntologyDeleteDialogProps = {
  api: StratumApi
  ontology: OntologySummary | null
  onClose(): void
  // 列表内容可能已变化（删除成功 / 已被修改 / 已不存在），调用方刷新当前页
  onListChanged(): void
}

export function OntologyDeleteDialog({
  api,
  ontology,
  onClose,
  onListChanged,
}: OntologyDeleteDialogProps) {
  const [deleting, setDeleting] = useState(false)
  const [message, setMessage] = useState<{
    kind: "conflict" | "error"
    text: string
  } | null>(null)

  const handleOpenChange = (nextOpen: boolean) => {
    if (nextOpen || deleting) return
    setMessage(null)
    onClose()
  }

  const handleConfirm = () => {
    if (ontology === null || deleting) return
    setDeleting(true)
    setMessage(null)
    void (async () => {
      try {
        const resource = await api.getOntology(ontology.id)
        await api.deleteOntology(ontology.id, resource.etag)
        setDeleting(false)
        onClose()
        onListChanged()
      } catch (error) {
        setDeleting(false)
        if (error instanceof ApiError && error.status === 412) {
          // ETag 已过期：提示并刷新列表，由用户基于最新列表重新决定
          setMessage({
            kind: "conflict",
            text: "该本体已被他人修改，列表已刷新，请确认后再操作。",
          })
          onListChanged()
          return
        }
        if (error instanceof ApiError && error.status === 404) {
          // 已不存在：等同于删除成功，刷新列表即可
          onClose()
          onListChanged()
          return
        }
        setMessage({
          kind: "error",
          text:
            error instanceof ApiError
              ? `删除失败：${error.message}`
              : "删除失败：无法连接到 Stratum 后端",
        })
      }
    })()
  }

  return (
    <Dialog open={ontology !== null} onOpenChange={handleOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>删除本体</DialogTitle>
          <DialogDescription>
            {ontology === null ? null : (
              <>
                将永久删除「{ontology.display_name}」（
                <span className="font-mono">{ontology.name}</span>
                ），此操作不可撤销。
              </>
            )}
          </DialogDescription>
        </DialogHeader>
        {message !== null ? (
          <p
            role="alert"
            className={
              message.kind === "conflict"
                ? "text-xs text-muted-foreground"
                : "text-xs text-destructive"
            }
          >
            {message.text}
          </p>
        ) : null}
        <DialogFooter>
          <Button
            variant="outline"
            onClick={() => handleOpenChange(false)}
            disabled={deleting}
          >
            取消
          </Button>
          <Button
            variant="destructive"
            onClick={handleConfirm}
            disabled={deleting}
          >
            {deleting ? "删除中…" : "删除"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
