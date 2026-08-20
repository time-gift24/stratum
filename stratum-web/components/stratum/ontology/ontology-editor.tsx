"use client"

import { useCallback, useMemo, useState } from "react"
import Link from "next/link"
import {
  ArrowLeft,
  CircleAlert,
  Loader2,
  Network,
  Plus,
  Save,
  SquarePen,
  XIcon,
} from "lucide-react"

import { Button, buttonVariants } from "@/components/ui/button"
import { OntologyCanvas } from "@/components/stratum/ontology/ontology-canvas"
import { ConflictDialog } from "@/components/stratum/ontology/conflict-dialog"
import { NeighborhoodView } from "@/components/stratum/ontology/neighborhood-view"
import {
  AddObjectTypeDialog,
  DeleteObjectTypeDialog,
} from "@/components/stratum/ontology/object-type-dialogs"
import {
  ChromePill,
  PillDivider,
  PillIconButton,
  PillLinkButton,
  PrimaryPillButton,
} from "@/components/stratum/ontology/ontology-chrome"
import { LinkTypeDialog } from "@/components/stratum/ontology/link-type-dialog"
import type {
  ObjectTypeNodeActions,
  ObjectTypePropertyActions,
} from "@/components/stratum/ontology/ontology-node"
import type { LinkTypeEdgeActions } from "@/components/stratum/ontology/ontology-edge"
import type { OntologyEditor as OntologyEditorController } from "@/hooks/use-ontology-editor"
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

/**
 * Ontology 画布编辑器主装配：加载相位（loading / missing / error）、
 * 满铺编辑画布、悬浮 chrome（身份 / 选中工具条 / 全局动作，见
 * ontology-chrome.tsx）、保存状态机 UI（dirty / in_flight / 412 / 422 /
 * save_unconfirmed）、崩溃恢复草稿、neighborhood 只读视图与本地聚焦。
 * 所有编辑只写 candidate；保存经 hook 的 PUT + If-Match 流程。
 */

export function OntologyEditor({
  editor,
}: {
  editor: OntologyEditorController
}) {
  const { state } = editor

  return (
    <div className="flex h-full min-h-0 flex-col bg-background">
      {state.phase === "loading" || state.phase === "idle" ? (
        <div className="flex h-full flex-col items-center justify-center gap-2 text-sm text-muted-foreground">
          <span className="animate-spin motion-reduce:animate-none">
            <Loader2 aria-hidden className="size-4" />
          </span>
          正在加载 Ontology…
        </div>
      ) : state.phase === "missing" ? (
        <div className="flex h-full flex-col items-center justify-center gap-3">
          <p className="text-sm">该 Ontology 不存在或已被删除。</p>
          <Link
            href="/ontologies"
            className={buttonVariants({ variant: "outline", size: "sm" })}
          >
            <ArrowLeft data-icon="inline-start" />
            返回列表
          </Link>
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

/**
 * 保存失败的稳定 code → 中文文案。契约规定 message 只是安全可读文本、
 * 不是控制流契约，因此已知 code 一律走本地映射；未知 code 回退服务端
 * message。409 ontology_entity_id_conflict：子实体 ID 在全部现存
 * Ontology 中按类型全局唯一，冲突说明本地 candidate 已过期（硬删除
 * 无 tombstone，客户端不复用已删除 ID）。
 */
const SAVE_ERROR_TEXT: Readonly<Record<string, string>> = {
  save_unconfirmed: "保存结果未确认，请检查远端状态后重试。",
  ontology_entity_id_conflict:
    "保存失败：实体 ID 已被另一个 Ontology 使用，本地草稿可能已过期，请重新加载后再编辑。",
  ontology_name_conflict:
    "保存失败：名称已被其他 Ontology 使用，请改名后重试。",
  ontology_payload_too_large: "保存失败：文档超过 2 MiB 上限。",
  ontology_store_unavailable: "保存失败：存储服务暂不可用，请稍后重试。",
}

function groupViolations(
  pointerDocument: OntologyEditorController["state"]["candidate"],
  displayedDocument: OntologyEditorController["state"]["candidate"],
  violations: readonly OntologyViolation[] | null
): GroupedViolations {
  if (
    pointerDocument === null ||
    displayedDocument === null ||
    violations === null ||
    violations.length === 0
  )
    return EMPTY_GROUPED_VIOLATIONS
  const objectViolations = new Map<string, string[]>()
  const propertyViolations = new Map<string, string[]>()
  const linkViolations = new Map<string, string[]>()
  const globalViolations: OntologyViolation[] = []
  const displayedObjectIds = new Set(
    displayedDocument.object_types.map(({ id }) => id)
  )
  const displayedPropertyIds = new Set(
    displayedDocument.object_types.flatMap((objectType) =>
      objectType.properties.map((property) => `${objectType.id}/${property.id}`)
    )
  )
  const displayedLinkIds = new Set(
    displayedDocument.link_types.map(({ id }) => id)
  )
  for (const { violation, target } of mapViolations(
    pointerDocument,
    violations
  )) {
    const push = (map: Map<string, string[]>, key: string) => {
      const bucket = map.get(key)
      if (bucket === undefined) map.set(key, [violation.message])
      else bucket.push(violation.message)
    }
    const propertyKey =
      target.objectTypeId && target.propertyId
        ? `${target.objectTypeId}/${target.propertyId}`
        : null
    if (
      target.kind === "property" &&
      propertyKey !== null &&
      displayedPropertyIds.has(propertyKey)
    ) {
      push(propertyViolations, propertyKey)
    } else if (
      (target.kind === "objectType" || target.kind === "canvas") &&
      target.objectTypeId &&
      displayedObjectIds.has(target.objectTypeId)
    ) {
      push(objectViolations, target.objectTypeId)
    } else if (
      target.kind === "linkType" &&
      target.linkTypeId &&
      displayedLinkIds.has(target.linkTypeId)
    ) {
      push(linkViolations, target.linkTypeId)
    } else {
      globalViolations.push(violation)
    }
  }
  return {
    objectViolations,
    propertyViolations,
    linkViolations,
    globalViolations,
  }
}

function ReadyEditor({ editor }: { editor: OntologyEditorController }) {
  const { state, dirty, save } = editor
  const candidate = state.candidate

  const [view, setView] = useState<"edit" | "neighborhood">("edit")
  const [focus, setFocus] = useState<{
    originId: string
    depth: number
  } | null>(null)
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
  const [clientViolations, setClientViolations] = useState<
    readonly OntologyViolation[] | null
  >(null)

  const limits = ONTOLOGY_MVP_LIMITS
  const objectTypes = candidate?.object_types
  const linkTypes = candidate?.link_types
  const totalProperties = useMemo(
    () =>
      (objectTypes ?? []).reduce(
        (sum, objectType) => sum + objectType.properties.length,
        0
      ),
    [objectTypes]
  )

  // 本地聚焦：由 candidate 实时计算，未保存编辑立即可见；origin 被删后自动失效
  const focusNeighborhood = useMemo(
    () =>
      objectTypes !== undefined && linkTypes !== undefined && focus !== null
        ? computeLocalNeighborhood(
            { object_types: objectTypes, link_types: linkTypes },
            focus.originId,
            focus.depth
          )
        : null,
    [objectTypes, linkTypes, focus]
  )
  const activeFocus = focusNeighborhood !== null ? focus : null

  const violations = useMemo(
    () =>
      groupViolations(
        state.violationDocument ?? candidate,
        candidate,
        state.violations
      ),
    [candidate, state.violationDocument, state.violations]
  )

  // 节点属性行内增删改：hook 方法本身引用稳定，聚合对象借此保持稳定，
  // 避免画布 toNodes 的 useMemo 每轮重建
  const propertyActions = useMemo<ObjectTypePropertyActions>(
    () => ({
      getAddPropertyDisabledReason: (objectType) =>
        objectType.properties.length >=
        ONTOLOGY_MVP_LIMITS.maxPropertiesPerObjectType
          ? `每个 Object Type 的属性数量已达上限（${ONTOLOGY_MVP_LIMITS.maxPropertiesPerObjectType}）`
          : totalProperties >= ONTOLOGY_MVP_LIMITS.maxTotalProperties
            ? `Ontology 总属性数量已达上限（${ONTOLOGY_MVP_LIMITS.maxTotalProperties}）`
            : null,
      onAddProperty: editor.addProperty,
      onUpdateProperty: editor.updateProperty,
      onRemoveProperty: editor.removeProperty,
    }),
    [
      editor.addProperty,
      editor.updateProperty,
      editor.removeProperty,
      totalProperties,
    ]
  )

  // 节点级动作：详情更新 / 请求删除（打开确认对话框）/ 聚焦邻域。
  // 引用稳定（candidate 变化时才重建，与文档派生同步），避免画布无谓重渲染
  const objectActions = useMemo<ObjectTypeNodeActions>(
    () => ({
      onUpdate: editor.updateObjectType,
      onRequestDelete: (objectType) =>
        setDeleteTarget({
          objectType,
          referencingLinks:
            candidate?.link_types.filter(
              (linkType) =>
                linkType.source_object_type_id === objectType.id ||
                linkType.target_object_type_id === objectType.id
            ) ?? [],
        }),
      onFocus: (originId, depth) => setFocus({ originId, depth }),
    }),
    [candidate, editor.updateObjectType]
  )

  const updateLinkType = editor.updateLinkType
  const removeLinkType = editor.removeLinkType
  const edgeActions = useMemo<LinkTypeEdgeActions>(
    () => ({
      onUpdate: updateLinkType,
      onDelete: (linkType) => removeLinkType(linkType.id),
    }),
    [updateLinkType, removeLinkType]
  )

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

  const saveDisabled =
    !dirty || inFlight || state.conflict !== null || view !== "edit"

  return (
    <>
      {/* 主区域：画布满铺；控制全部收进悬浮 pill（ontology-chrome.tsx） */}
      <div className="relative min-h-0 flex-1">
        {view === "neighborhood" ? (
          <NeighborhoodView
            ontologyId={candidate.id}
            objectTypes={state.acknowledged?.document.object_types ?? []}
          />
        ) : (
          <OntologyCanvas
            document={candidate}
            focus={activeFocus !== null ? focusNeighborhood : null}
            objectViolations={violations.objectViolations}
            propertyViolations={violations.propertyViolations}
            linkViolations={violations.linkViolations}
            propertyActions={propertyActions}
            objectActions={objectActions}
            edgeActions={edgeActions}
            onConnectNodes={handleConnect}
            onNodeDragStop={editor.setPosition}
          />
        )}

        {/* 顶部悬浮 pill 群：左 = 返回 + 标题 + 保存状态；
            右 = 视图切换 / 新增 pill + 独立的保存主操作（有脏数据时实心） */}
        <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex items-start justify-between gap-2 px-3 pt-20">
          <ChromePill className="max-w-[60vw]">
            <PillLinkButton label="返回列表" href="/ontologies">
              <ArrowLeft aria-hidden className="size-4" />
            </PillLinkButton>
            <PillDivider />
            <span className="max-w-56 truncate px-1.5 text-xs font-medium">
              {candidate.display_name}
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
              <span
                role="status"
                className="pr-1.5 text-xs text-muted-foreground"
              >
                未保存
              </span>
            ) : null}
          </ChromePill>
          <div className="flex items-start gap-2">
            <ChromePill>
              <PillIconButton
                label="编辑画布"
                active={view === "edit"}
                onClick={() => setView("edit")}
              >
                <SquarePen aria-hidden className="size-4" />
              </PillIconButton>
              <PillIconButton
                label="邻域视图"
                active={view === "neighborhood"}
                onClick={() => setView("neighborhood")}
              >
                <Network aria-hidden className="size-4" />
              </PillIconButton>
              {view === "edit" ? (
                <>
                  <PillDivider />
                  <PillIconButton
                    label="新增 Object Type"
                    onClick={openAddDialog}
                  >
                    <Plus aria-hidden className="size-4" />
                  </PillIconButton>
                </>
              ) : null}
            </ChromePill>
            {view === "edit" ? (
              <PrimaryPillButton
                label="保存"
                loading={inFlight}
                disabled={saveDisabled}
                onClick={() => void handleSave()}
              >
                <Save aria-hidden className="size-4" />
              </PrimaryPillButton>
            ) : null}
          </div>
        </div>

        {/* 状态横幅 + 聚焦指示：顶部居中浮层（避开 pill 群与站点导航），有事才出现 */}
        <div className="pointer-events-none absolute inset-x-0 top-32 z-10 flex flex-col items-center gap-2 px-3">
          {state.draftAvailable !== null && (
            <div className="pointer-events-auto flex flex-wrap items-center justify-center gap-2 rounded-xl border border-border bg-popover px-3 py-2 text-xs dark:bg-popover/95 dark:shadow-[0_8px_30px] dark:shadow-black/10 dark:backdrop-blur">
              <span>发现未保存的草稿（上次编辑未完成保存）。</span>
              <Button variant="outline" size="xs" onClick={editor.restoreDraft}>
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
              className="pointer-events-auto flex max-w-lg flex-wrap items-center justify-center gap-2 rounded-xl border border-destructive/40 bg-popover px-3 py-2 text-xs text-destructive dark:bg-popover/95 dark:shadow-[0_8px_30px] dark:shadow-black/10 dark:backdrop-blur"
            >
              <CircleAlert aria-hidden className="size-3.5" />
              <span>
                {SAVE_ERROR_TEXT[state.saveError.code] ??
                  `保存失败：${state.saveError.message}`}
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
              className="pointer-events-auto max-w-lg rounded-xl border border-destructive/40 bg-popover px-3 py-2 text-xs text-destructive dark:bg-popover/95 dark:shadow-[0_8px_30px] dark:shadow-black/10 dark:backdrop-blur"
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
              className="pointer-events-auto max-w-lg rounded-xl border border-destructive/40 bg-popover px-3 py-2 text-xs text-destructive dark:bg-popover/95 dark:shadow-[0_8px_30px] dark:shadow-black/10 dark:backdrop-blur"
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
              className="pointer-events-auto flex items-center gap-2 rounded-xl border border-border bg-popover px-3 py-2 text-xs dark:bg-popover/95 dark:shadow-[0_8px_30px] dark:shadow-black/10 dark:backdrop-blur"
            >
              <span>{notice}</span>
              <Button variant="ghost" size="xs" onClick={() => setNotice(null)}>
                知道了
              </Button>
            </div>
          )}
          {view === "edit" && activeFocus !== null && (
            <div className="pointer-events-auto flex items-center gap-1 rounded-full border border-border bg-popover py-1 pr-1 pl-3 text-xs dark:bg-popover/95 dark:shadow-[0_8px_30px] dark:shadow-black/10 dark:backdrop-blur">
              <span>
                聚焦 {findObjectType(activeFocus.originId)?.display_name ?? ""}
                （深度 {activeFocus.depth}，基于本地草稿）
              </span>
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label="退出聚焦"
                className="rounded-full"
                onClick={() => setFocus(null)}
              >
                <XIcon />
              </Button>
            </div>
          )}
        </div>
      </div>

      {/* 对话框 */}
      <AddObjectTypeDialog
        open={addOpen}
        onOpenChange={setAddOpen}
        onSubmit={(input) => {
          editor.addObjectType(input)
          setAddOpen(false)
        }}
      />
      <DeleteObjectTypeDialog
        target={deleteTarget}
        onCancel={() => setDeleteTarget(null)}
        onConfirm={() => {
          if (deleteTarget !== null)
            editor.removeObjectType(deleteTarget.objectType.id)
          setDeleteTarget(null)
        }}
      />
      <LinkTypeDialog
        pending={pendingLink}
        source={
          pendingLink !== null
            ? findObjectType(pendingLink.sourceId)
            : undefined
        }
        target={
          pendingLink !== null
            ? findObjectType(pendingLink.targetId)
            : undefined
        }
        onCancel={() => setPendingLink(null)}
        onSubmit={(input) => {
          if (pendingLink === null) return
          editor.addLinkType({
            ...input,
            source_object_type_id: pendingLink.sourceId,
            target_object_type_id: pendingLink.targetId,
          })
          setPendingLink(null)
        }}
      />
      <ConflictDialog
        open={state.conflict !== null}
        onReconcile={editor.reconcile}
      />
    </>
  )
}
