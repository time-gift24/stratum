"use client"

import { useCallback, useMemo, useState } from "react"
import Link from "next/link"
import { ArrowLeft, CircleAlert, Loader2, Plus } from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  OntologyCanvas,
  type CanvasSelection,
} from "@/components/stratum/ontology/ontology-canvas"
import { ConflictDialog } from "@/components/stratum/ontology/conflict-dialog"
import { NeighborhoodView } from "@/components/stratum/ontology/neighborhood-view"
import {
  AddObjectTypeDialog,
  DeleteObjectTypeDialog,
} from "@/components/stratum/ontology/object-type-dialogs"
import {
  ObjectTypePanel,
  type PropertyInput,
} from "@/components/stratum/ontology/object-type-panel"
import {
  LinkTypeDialog,
  LinkTypePanel,
} from "@/components/stratum/ontology/link-type-dialog"
import type { ObjectTypePropertyActions } from "@/components/stratum/ontology/ontology-node"
import {
  useOntologyEditor,
  type OntologyEditor,
} from "@/hooks/use-ontology-editor"
import { computeLocalNeighborhood } from "@/features/ontology-editor/neighborhood"
import { mapViolations } from "@/features/ontology-editor/violations"
import {
  ONTOLOGY_MVP_LIMITS,
  validateOntologyDocument,
} from "@/features/ontology-editor/validation"
import type {
  OntologyLinkType,
  OntologyObjectType,
  OntologyViolation,
} from "@/features/ontology-editor/types"
import { cn } from "@/lib/utils"

/**
 * Ontology 画布编辑器主装配：加载相位（loading / missing / error）、
 * 编辑画布、编辑面板、保存状态机 UI（dirty / in_flight / 412 / 422 /
 * save_unconfirmed）、崩溃恢复草稿、neighborhood 只读视图与本地聚焦。
 * 所有编辑只写 candidate；保存经 hook 的 PUT + If-Match 流程。
 */

export function OntologyEditor({ ontologyId }: { ontologyId: string }) {
  const editor = useOntologyEditor(ontologyId)
  const { state } = editor

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      {state.phase === "loading" || state.phase === "idle" ? (
        <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
          <Loader2 aria-hidden className="size-4 animate-spin" />
          正在加载 Ontology…
        </div>
      ) : state.phase === "missing" ? (
        <div className="flex h-full flex-col items-center justify-center gap-3">
          <p className="text-sm">该 Ontology 不存在或已被删除。</p>
          <Button
            variant="outline"
            size="sm"
            render={<Link href="/ontologies" />}
          >
            <ArrowLeft data-icon="inline-start" />
            返回列表
          </Button>
        </div>
      ) : state.phase === "error" ? (
        <div className="flex h-full flex-col items-center justify-center gap-3">
          <p role="alert" className="text-sm text-destructive">
            加载失败：{state.error?.message ?? "未知错误"}
          </p>
          <Button variant="outline" size="sm" onClick={editor.reload}>
            重试
          </Button>
        </div>
      ) : (
        <ReadyEditor editor={editor} />
      )}
    </div>
  )
}

/** 422 / 客户端校验违例按映射目标分组：节点、属性、边内联，其余进全局区 */
type GroupedViolations = {
  objectViolations: ReadonlyMap<string, readonly string[]>
  propertyViolations: ReadonlyMap<string, readonly string[]>
  linkViolations: ReadonlyMap<string, readonly string[]>
  globalViolations: readonly OntologyViolation[]
}

// 无违例时共享同一冻结对象，保持 prop 引用稳定，避免画布无谓重渲染
const EMPTY_GROUPED_VIOLATIONS: GroupedViolations = Object.freeze({
  objectViolations: new Map<string, readonly string[]>(),
  propertyViolations: new Map<string, readonly string[]>(),
  linkViolations: new Map<string, readonly string[]>(),
  globalViolations: Object.freeze([]) as readonly OntologyViolation[],
})

function groupViolations(
  document: OntologyEditor["state"]["candidate"],
  violations: readonly OntologyViolation[] | null
): GroupedViolations {
  if (document === null || violations === null || violations.length === 0)
    return EMPTY_GROUPED_VIOLATIONS
  const objectViolations = new Map<string, string[]>()
  const propertyViolations = new Map<string, string[]>()
  const linkViolations = new Map<string, string[]>()
  const globalViolations: OntologyViolation[] = []
  for (const { violation, target } of mapViolations(document, violations)) {
    const push = (map: Map<string, string[]>, key: string) => {
      const bucket = map.get(key)
      if (bucket === undefined) map.set(key, [violation.message])
      else bucket.push(violation.message)
    }
    if (target.kind === "property" && target.objectTypeId && target.propertyId) {
      push(propertyViolations, `${target.objectTypeId}/${target.propertyId}`)
    } else if (
      (target.kind === "objectType" || target.kind === "canvas") &&
      target.objectTypeId
    ) {
      push(objectViolations, target.objectTypeId)
    } else if (target.kind === "linkType" && target.linkTypeId) {
      push(linkViolations, target.linkTypeId)
    } else {
      globalViolations.push(violation)
    }
  }
  return { objectViolations, propertyViolations, linkViolations, globalViolations }
}

function propertyMessagesFor(
  propertyViolations: ReadonlyMap<string, readonly string[]>,
  objectTypeId: string
): ReadonlyMap<string, readonly string[]> {
  const prefix = `${objectTypeId}/`
  const result = new Map<string, readonly string[]>()
  for (const [key, messages] of propertyViolations) {
    if (key.startsWith(prefix)) result.set(key.slice(prefix.length), messages)
  }
  return result
}

function ReadyEditor({ editor }: { editor: OntologyEditor }) {
  const { state, dirty, save } = editor
  const candidate = state.candidate

  const [selection, setSelection] = useState<CanvasSelection>(null)
  const [view, setView] = useState<"edit" | "neighborhood">("edit")
  const [focus, setFocus] = useState<{ originId: string; depth: number } | null>(
    null
  )
  const [focusDepth, setFocusDepth] = useState(1)
  const [addOpen, setAddOpen] = useState(false)
  const [deleteTarget, setDeleteTarget] = useState<{
    objectType: OntologyObjectType
    referencingLinks: readonly OntologyLinkType[]
  } | null>(null)
  const [pendingLink, setPendingLink] = useState<{
    sourceId: string
    targetId: string
  } | null>(null)
  const [notice, setNotice] = useState<string | null>(null)
  const [clientViolations, setClientViolations] =
    useState<readonly OntologyViolation[] | null>(null)

  const limits = ONTOLOGY_MVP_LIMITS
  const totalProperties = useMemo(
    () =>
      (candidate?.object_types ?? []).reduce(
        (sum, objectType) => sum + objectType.properties.length,
        0
      ),
    [candidate]
  )

  // 本地聚焦：由 candidate 实时计算，未保存编辑立即可见；origin 被删后自动失效
  const focusNeighborhood = useMemo(
    () =>
      candidate !== null && focus !== null
        ? computeLocalNeighborhood(candidate, focus.originId, focus.depth)
        : null,
    [candidate, focus]
  )
  const activeFocus = focusNeighborhood !== null ? focus : null

  const violations = useMemo(
    () => groupViolations(candidate, state.violations),
    [candidate, state.violations]
  )

  // 节点属性行内增删改：hook 方法本身引用稳定，聚合对象借此保持稳定，
  // 避免画布 toNodes 的 useMemo 每轮重建
  const propertyActions = useMemo<ObjectTypePropertyActions>(
    () => ({
      onAddProperty: editor.addProperty,
      onUpdateProperty: editor.updateProperty,
      onRemoveProperty: editor.removeProperty,
    }),
    [editor.addProperty, editor.updateProperty, editor.removeProperty]
  )

  const selectedObjectType =
    candidate !== null && selection?.kind === "objectType"
      ? candidate.object_types.find(
          (objectType) => objectType.id === selection.id
        )
      : undefined
  const selectedLinkType =
    candidate !== null && selection?.kind === "linkType"
      ? candidate.link_types.find((linkType) => linkType.id === selection.id)
      : undefined

  const findObjectType = useCallback(
    (id: string) =>
      candidate?.object_types.find((objectType) => objectType.id === id),
    [candidate]
  )

  const handleSave = useCallback(async () => {
    if (candidate === null) return
    setNotice(null)
    // 保存前客户端先行校验：不过则不发送必然 422 的请求
    const found = validateOntologyDocument(candidate)
    if (found.length > 0) {
      setClientViolations(found)
      return
    }
    setClientViolations(null)
    await save()
  }, [candidate, save])

  if (candidate === null) return null

  const inFlight = state.inFlight !== null
  const objectTypeLimitReached =
    candidate.object_types.length >= limits.maxObjectTypes
  const linkLimitReached = candidate.link_types.length >= limits.maxLinkTypes

  const openAddDialog = () => {
    if (objectTypeLimitReached) {
      setNotice(
        `Object Type 数量已达上限（${limits.maxObjectTypes}），无法继续新增`
      )
      return
    }
    setNotice(null)
    setAddOpen(true)
  }

  const handleConnect = (sourceId: string, targetId: string) => {
    if (linkLimitReached) {
      setNotice(
        `Link Type 数量已达上限（${limits.maxLinkTypes}），无法继续新增`
      )
      return
    }
    setNotice(null)
    setPendingLink({ sourceId, targetId })
  }

  const requestDeleteObjectType = (objectType: OntologyObjectType) => {
    setDeleteTarget({
      objectType,
      referencingLinks: candidate.link_types.filter(
        (linkType) =>
          linkType.source_object_type_id === objectType.id ||
          linkType.target_object_type_id === objectType.id
      ),
    })
  }

  const saveDisabled =
    !dirty || inFlight || state.conflict !== null || view !== "edit"

  return (
    <>
      {/* 工具栏：返回、标题、视图切换、保存状态与动作 */}
      <header className="flex flex-wrap items-center gap-2 border-b border-border px-4 py-2">
        <Button
          variant="ghost"
          size="sm"
          render={<Link href="/ontologies" />}
        >
          <ArrowLeft data-icon="inline-start" />
          返回列表
        </Button>
        <div className="min-w-0">
          <h1 className="truncate text-sm font-medium">
            {candidate.display_name}
          </h1>
          <p className="truncate font-mono text-[0.6875rem] text-muted-foreground">
            {candidate.name}
          </p>
        </div>
        <div className="ml-auto flex items-center gap-2">
          <span aria-live="polite" className="text-xs text-muted-foreground">
            {inFlight ? (
              "保存中…"
            ) : dirty ? (
              <span className="text-foreground">未保存</span>
            ) : (
              "已保存"
            )}
          </span>
          <div
            role="group"
            aria-label="视图切换"
            className="flex rounded-full border border-border p-0.5"
          >
            <button
              type="button"
              aria-pressed={view === "edit"}
              onClick={() => setView("edit")}
              className={cn(
                "rounded-full px-2.5 py-1 text-xs transition-colors",
                view === "edit"
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              编辑画布
            </button>
            <button
              type="button"
              aria-pressed={view === "neighborhood"}
              onClick={() => setView("neighborhood")}
              className={cn(
                "rounded-full px-2.5 py-1 text-xs transition-colors",
                view === "neighborhood"
                  ? "bg-accent text-accent-foreground"
                  : "text-muted-foreground hover:text-foreground"
              )}
            >
              邻域视图
            </button>
          </div>
          {view === "edit" && (
            <Button variant="outline" size="sm" onClick={openAddDialog}>
              <Plus data-icon="inline-start" />
              新增 Object Type
            </Button>
          )}
          <Button
            size="sm"
            disabled={saveDisabled}
            onClick={() => void handleSave()}
          >
            {inFlight ? "保存中…" : "保存"}
          </Button>
        </div>
      </header>

      {/* 状态横幅区：草稿恢复 / 保存错误 / 全局违例 / 限制与客户端校验提示 */}
      <div className="flex flex-col">
        {state.draftAvailable !== null && (
          <div className="flex flex-wrap items-center gap-2 border-b border-border bg-muted/50 px-4 py-2 text-xs">
            <span>发现未保存的草稿（上次编辑未完成保存）。</span>
            <Button
              variant="outline"
              size="xs"
              onClick={editor.restoreDraft}
            >
              恢复草稿
            </Button>
            <Button variant="ghost" size="xs" onClick={editor.discardDraft}>
              丢弃
            </Button>
          </div>
        )}
        {state.saveError !== null && (
          <div
            role="alert"
            className="flex flex-wrap items-center gap-2 border-b border-destructive/40 bg-destructive/10 px-4 py-2 text-xs text-destructive"
          >
            <CircleAlert aria-hidden className="size-3.5" />
            <span>
              {state.saveError.code === "save_unconfirmed"
                ? "保存结果未确认，请检查远端状态后重试。"
                : `保存失败：${state.saveError.message}`}
            </span>
            <Button
              variant="outline"
              size="xs"
              disabled={inFlight}
              onClick={() => void handleSave()}
            >
              重试保存
            </Button>
          </div>
        )}
        {violations.globalViolations.length > 0 && (
          <div
            role="alert"
            className="border-b border-destructive/40 bg-destructive/10 px-4 py-2 text-xs text-destructive"
          >
            <p className="font-medium">
              保存被拒绝（422），以下问题未定位到具体节点：
            </p>
            <ul className="mt-1 flex flex-col gap-0.5">
              {violations.globalViolations.map((violation) => (
                <li key={`${violation.path}:${violation.code}`}>
                  <span className="font-mono">{violation.path}</span>：
                  {violation.message}
                </li>
              ))}
            </ul>
          </div>
        )}
        {clientViolations !== null && clientViolations.length > 0 && (
          <div
            role="alert"
            className="border-b border-destructive/40 bg-destructive/10 px-4 py-2 text-xs text-destructive"
          >
            <p className="font-medium">
              本地校验未通过，已取消本次保存（不会产生必然被拒绝的请求）：
            </p>
            <ul className="mt-1 flex flex-col gap-0.5">
              {clientViolations.map((violation) => (
                <li key={`${violation.path}:${violation.code}`}>
                  <span className="font-mono">{violation.path}</span>：
                  {violation.message}
                </li>
              ))}
            </ul>
          </div>
        )}
        {notice !== null && (
          <div
            role="status"
            className="flex items-center gap-2 border-b border-border bg-muted/50 px-4 py-2 text-xs"
          >
            <span>{notice}</span>
            <Button variant="ghost" size="xs" onClick={() => setNotice(null)}>
              知道了
            </Button>
          </div>
        )}
      </div>

      {/* 主区域：编辑画布 / 邻域只读视图 */}
      <div className="relative min-h-0 flex-1">
        {view === "neighborhood" ? (
          <NeighborhoodView
            ontologyId={candidate.id}
            objectTypes={state.acknowledged?.document.object_types ?? []}
          />
        ) : (
          <>
            <OntologyCanvas
              document={candidate}
              focus={activeFocus !== null ? focusNeighborhood : null}
              objectViolations={violations.objectViolations}
              linkViolations={violations.linkViolations}
              propertyActions={propertyActions}
              onSelectionChange={setSelection}
              onConnectNodes={handleConnect}
              onNodeDragStop={editor.setPosition}
            />

            {activeFocus !== null && (
              <div className="absolute top-3 left-1/2 flex -translate-x-1/2 items-center gap-2 rounded-full border border-border bg-popover px-3 py-1 text-xs shadow-[0_8px_30px] shadow-black/10">
                <span>
                  聚焦：
                  {findObjectType(activeFocus.originId)?.display_name ?? ""}
                  （深度 {activeFocus.depth}，基于本地草稿）
                </span>
                <Button
                  variant="ghost"
                  size="xs"
                  onClick={() => setFocus(null)}
                >
                  退出聚焦
                </Button>
              </div>
            )}

            {selectedObjectType !== undefined && (
              <div className="absolute top-3 right-3 bottom-3 w-[28rem]">
                <ObjectTypePanel
                    objectType={selectedObjectType}
                    messages={
                      violations.objectViolations.get(selectedObjectType.id) ??
                      []
                    }
                    propertyMessages={propertyMessagesFor(
                      violations.propertyViolations,
                      selectedObjectType.id
                    )}
                    canAddProperty={
                      selectedObjectType.properties.length <
                        limits.maxPropertiesPerObjectType &&
                      totalProperties < limits.maxTotalProperties
                    }
                    propertyLimitMessage={
                      selectedObjectType.properties.length >=
                      limits.maxPropertiesPerObjectType
                        ? `每个 Object Type 的属性数量已达上限（${limits.maxPropertiesPerObjectType}）`
                        : totalProperties >= limits.maxTotalProperties
                          ? `Ontology 总属性数量已达上限（${limits.maxTotalProperties}）`
                          : null
                    }
                    focusDepth={focusDepth}
                    onFocusDepthChange={setFocusDepth}
                    onFocus={() =>
                      setFocus({
                        originId: selectedObjectType.id,
                        depth: focusDepth,
                      })
                    }
                    onUpdate={editor.updateObjectType}
                    onDelete={() =>
                      requestDeleteObjectType(selectedObjectType)
                    }
                    onAddProperty={(input: PropertyInput) =>
                      editor.addProperty(selectedObjectType.id, input)
                    }
                    onUpdateProperty={(property) =>
                      editor.updateProperty(selectedObjectType.id, property)
                    }
                    onRemoveProperty={(propertyId) =>
                      editor.removeProperty(selectedObjectType.id, propertyId)
                    }
                    onClose={() => setSelection(null)}
                  />
              </div>
            )}
            {selectedObjectType === undefined &&
              selectedLinkType !== undefined && (
              <div className="absolute top-3 right-3 bottom-3 w-80">
                  <LinkTypePanel
                    linkType={selectedLinkType}
                    source={findObjectType(
                      selectedLinkType.source_object_type_id
                    )}
                    target={findObjectType(
                      selectedLinkType.target_object_type_id
                    )}
                    messages={
                      violations.linkViolations.get(selectedLinkType.id) ?? []
                    }
                    onUpdate={editor.updateLinkType}
                    onDelete={() => {
                      editor.removeLinkType(selectedLinkType.id)
                      setSelection(null)
                    }}
                    onClose={() => setSelection(null)}
                  />
              </div>
            )}
          </>
        )}
      </div>

      {/* 对话框 */}
      <AddObjectTypeDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onSubmit={(input) => {
          const id = editor.addObjectType(input)
          setAddOpen(false)
          setSelection({ kind: "objectType", id })
        }}
      />
      <DeleteObjectTypeDialog
        target={deleteTarget}
        onCancel={() => setDeleteTarget(null)}
        onConfirm={() => {
          if (deleteTarget !== null)
            editor.removeObjectType(deleteTarget.objectType.id)
          setDeleteTarget(null)
          setSelection(null)
        }}
      />
      <LinkTypeDialog
        pending={pendingLink}
        source={
          pendingLink !== null ? findObjectType(pendingLink.sourceId) : undefined
        }
        target={
          pendingLink !== null ? findObjectType(pendingLink.targetId) : undefined
        }
        onCancel={() => setPendingLink(null)}
        onSubmit={(input) => {
          if (pendingLink === null) return
          const id = editor.addLinkType({
            ...input,
            source_object_type_id: pendingLink.sourceId,
            target_object_type_id: pendingLink.targetId,
          })
          setPendingLink(null)
          setSelection({ kind: "linkType", id })
        }}
      />
      <ConflictDialog
        open={state.conflict !== null}
        onReconcile={editor.reconcile}
      />
    </>
  )
}
