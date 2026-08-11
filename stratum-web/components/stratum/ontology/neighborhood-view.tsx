"use client"

import { useMemo, useState } from "react"
import { Loader2 } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { NeighborhoodCanvas } from "@/components/stratum/ontology/ontology-canvas"
import { ApiError } from "@/lib/stratum/api"
import { resolveOntologyApi } from "@/lib/stratum/ontology-api"
import { MAX_NEIGHBORHOOD_DEPTH } from "@/features/ontology-editor/neighborhood"
import type {
  OntologyNeighborhood,
  OntologyObjectType,
} from "@/features/ontology-editor/types"

/**
 * Neighborhood 只读聚焦视图（持久化图）：选择起点 object type 与 depth
 * （0–5，默认 1）→ GET neighborhood 端点 → 只读渲染返回子图。
 * 不提供任何编辑入口，不触碰 candidate。404 object_type_not_found 内联提示。
 */

type ViewStatus =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; result: OntologyNeighborhood }
  | { kind: "error"; error: ApiError }

export function NeighborhoodView({
  ontologyId,
  objectTypes,
}: {
  ontologyId: string
  objectTypes: readonly OntologyObjectType[]
}) {
  const api = useMemo(() => resolveOntologyApi(undefined), [])
  const [originId, setOriginId] = useState("")
  const [depth, setDepth] = useState(1)
  const [status, setStatus] = useState<ViewStatus>({ kind: "idle" })

  const run = async () => {
    if (originId === "") return
    setStatus({ kind: "loading" })
    try {
      const result = await api.getObjectTypeNeighborhood(
        ontologyId,
        originId,
        depth
      )
      setStatus({ kind: "ready", result })
    } catch (error) {
      setStatus({
        kind: "error",
        error:
          error instanceof ApiError
            ? error
            : new ApiError("connection_error", 0, "connection failed"),
      })
    }
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2">
        <span className="text-xs text-muted-foreground">
          只读邻域（持久化图，不含未保存更改）
        </span>
        <Select
          value={originId === "" ? null : originId}
          onValueChange={(next) => setOriginId(next ?? "")}
        >
          <SelectTrigger aria-label="邻域起点 Object Type" className="w-56">
            <SelectValue placeholder="选择起点 Object Type…" />
          </SelectTrigger>
          <SelectContent>
            {objectTypes.map((objectType) => (
              <SelectItem key={objectType.id} value={objectType.id}>
                {objectType.display_name}（{objectType.name}）
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Select
          value={depth}
          onValueChange={(next) => setDepth(next ?? 1)}
        >
          <SelectTrigger aria-label="邻域深度" className="w-24">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            {Array.from({ length: MAX_NEIGHBORHOOD_DEPTH + 1 }, (_, value) => (
              <SelectItem key={value} value={value}>
                深度 {value}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <Button
          size="sm"
          disabled={originId === "" || status.kind === "loading"}
          onClick={() => void run()}
        >
          {status.kind === "loading" ? (
            <>
              <Loader2 data-icon="inline-start" className="animate-spin" />
              查询中…
            </>
          ) : (
            "查看邻域"
          )}
        </Button>
      </div>

      <div className="min-h-0 flex-1">
        {status.kind === "idle" && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            选择起点与深度后查看持久化图的邻域。
          </div>
        )}
        {status.kind === "loading" && (
          <div className="flex h-full items-center justify-center text-sm text-muted-foreground">
            正在加载邻域…
          </div>
        )}
        {status.kind === "error" && (
          <div className="flex h-full flex-col items-center justify-center gap-2">
            <p role="alert" className="text-sm text-destructive">
              {status.error.code === "object_type_not_found"
                ? "该 Object Type 在远端不存在（可能已被他人删除），请选择其他起点。"
                : `邻域加载失败：${status.error.message}`}
            </p>
            {status.error.code !== "object_type_not_found" && (
              <Button
                variant="outline"
                size="sm"
                onClick={() => void run()}
              >
                重试
              </Button>
            )}
          </div>
        )}
        {status.kind === "ready" && (
          <div className="flex h-full min-h-0 flex-col">
            <p className="border-b border-border px-4 py-1.5 text-xs text-muted-foreground">
              命中 {status.result.object_types.length} 个 Object Type、
              {status.result.link_types.length} 条 Link Type（深度{" "}
              {status.result.depth}，只读）
            </p>
            <div className="min-h-0 flex-1">
              <NeighborhoodCanvas neighborhood={status.result} />
            </div>
          </div>
        )}
      </div>
    </div>
  )
}
