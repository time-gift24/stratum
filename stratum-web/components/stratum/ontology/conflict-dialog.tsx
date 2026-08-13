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

/**
 * 412 ontology_precondition_failed 调和对话框：远端已被他人/另一标签页更新。
 * 契约禁止静默重试，必须由用户显式选择保留本地或采用远端——对话框不提供
 * 关闭按钮、不响应外部关闭，选完才消失（state.conflict 清空）。
 */

export function ConflictDialog({
  open,
  onReconcile,
}: {
  open: boolean
  onReconcile(resolution: "local" | "remote"): Promise<boolean>
}) {
  const [pending, setPending] = useState<"local" | "remote" | null>(null)
  const [failed, setFailed] = useState(false)

  const choose = async (resolution: "local" | "remote") => {
    setPending(resolution)
    setFailed(false)
    const ok = await onReconcile(resolution)
    setPending(null)
    if (!ok) setFailed(true)
  }

  return (
    <Dialog open={open} onOpenChange={() => {}} modal>
      <DialogContent showCloseButton={false}>
        <DialogHeader>
          <DialogTitle>保存冲突：远端已有更新</DialogTitle>
          <DialogDescription>
            此 Ontology 在你编辑期间被其他来源修改，你的本地更改尚未保存。
            请选择如何处理——两种选择都会丢弃其中一方的未保存更改。
          </DialogDescription>
        </DialogHeader>
        <ul className="flex flex-col gap-2 text-xs">
          <li className="rounded-lg border border-border px-3 py-2">
            <p className="font-medium">保留本地版本</p>
            <p className="text-muted-foreground">
              以你的本地草稿为准，再次点击「保存」时将覆盖远端版本，远端的新
              更改会丢失。
            </p>
          </li>
          <li className="rounded-lg border border-border px-3 py-2">
            <p className="font-medium">采用远端版本</p>
            <p className="text-muted-foreground">
              丢弃你本地全部未保存的更改，载入远端最新版本。
            </p>
          </li>
        </ul>
        {failed && (
          <p role="alert" className="text-xs text-destructive">
            重读远端状态失败，请重试。
          </p>
        )}
        <DialogFooter>
          <Button
            variant="outline"
            disabled={pending !== null}
            onClick={() => void choose("remote")}
          >
            {pending === "remote" ? "处理中…" : "采用远端版本"}
          </Button>
          <Button
            disabled={pending !== null}
            onClick={() => void choose("local")}
          >
            {pending === "local" ? "处理中…" : "保留本地版本"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
