"use client"

import {
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react"

import { ApiError, type StratumApi } from "@/lib/stratum/api"
import {
  finishCreatedOntologyHandoff,
  readCreatedOntologyHandoff,
} from "@/lib/stratum/ontology-navigation-handoff"
import { resolveOntologyApi } from "@/lib/stratum/ontology-api"
import { createUuidV7 } from "@/features/ontology-editor/ids"
import {
  browserOntologyDraftStore,
  type OntologyDraftStore,
} from "@/features/ontology-editor/recovery"
import {
  canClearOntologyDraft,
  initialOntologyEditorState,
  isOntologyEditorDirty,
  ontologyDocumentsEqual,
  ontologyEditorReducer,
  type OntologyEditorState,
} from "@/features/ontology-editor/reducer"
import { attemptOntologySave } from "@/features/ontology-editor/save"
import type {
  OntologyLinkCardinality,
  OntologyLinkType,
  OntologyObjectType,
  OntologyProperty,
  OntologyPropertyValueType,
} from "@/features/ontology-editor/types"

const DRAFT_SAVE_DEBOUNCE_MS = 250

export type UseOntologyEditorOptions = {
  api?: StratumApi
  // 显式传 null 可禁用草稿持久化（默认取浏览器 IndexedDB）
  draftStore?: OntologyDraftStore | null
}

export type OntologyEditor = {
  state: OntologyEditorState
  dirty: boolean
  reload(): void
  save(): Promise<boolean>
  reconcile(resolution: "local" | "remote"): Promise<boolean>
  restoreDraft(): void
  discardDraft(): void
  addObjectType(input: {
    name: string
    display_name: string
    description?: string
  }): string
  updateObjectType(objectType: OntologyObjectType): void
  removeObjectType(objectTypeId: string): void
  addProperty(
    objectTypeId: string,
    input: {
      name: string
      display_name: string
      description?: string
      value_type: OntologyPropertyValueType
      required: boolean
    }
  ): string
  updateProperty(objectTypeId: string, property: OntologyProperty): void
  removeProperty(objectTypeId: string, propertyId: string): void
  addLinkType(input: {
    name: string
    display_name: string
    description?: string
    source_object_type_id: string
    target_object_type_id: string
    source_to_target: OntologyLinkCardinality
    target_to_source: OntologyLinkCardinality
  }): string
  updateLinkType(linkType: OntologyLinkType): void
  removeLinkType(linkTypeId: string): void
  setPosition(objectTypeId: string, x: number, y: number): void
}

export function useOntologyEditor(
  ontologyId: string | null,
  options?: UseOntologyEditorOptions
): OntologyEditor {
  const [state, dispatch] = useReducer(
    ontologyEditorReducer,
    initialOntologyEditorState
  )
  const [reloadVersion, setReloadVersion] = useState(0)

  const apiOption = options?.api
  const draftStoreOption = options?.draftStore
  const api = useMemo(() => resolveOntologyApi(apiOption), [apiOption])
  const draftStore = useMemo(
    () =>
      draftStoreOption !== undefined
        ? draftStoreOption
        : browserOntologyDraftStore(),
    [draftStoreOption]
  )

  const stateRef = useRef(state)
  useEffect(() => {
    stateRef.current = state
  }, [state])

  // 加载：GET 与 IndexedDB 草稿读取并发 → acknowledged + candidate 深拷贝 → 草稿处置
  useEffect(() => {
    if (ontologyId === null) return
    let cancelled = false

    dispatch({ type: "load_started", ontologyId })
    void (async () => {
      // create 成功后的紧接客户端导航直接消费 POST 返回的文档与 ETag；
      // 无 handoff（包括完整刷新）才发 GET。
      const createdResource = readCreatedOntologyHandoff(ontologyId)
      const resourcePromise =
        createdResource === null
          ? api.getOntology(ontologyId)
          : Promise.resolve(createdResource)
      // 草稿读取与 GET 同时发出；先挂 noop catch，避免 GET 先失败时草稿
      // Promise 成为 unhandled rejection（真正 await 时仍会抛错）
      const draftPromise = draftStore?.loadDraft(ontologyId) ?? null
      draftPromise?.catch(() => {})
      let resource
      try {
        resource = await resourcePromise
      } catch (error) {
        if (cancelled) return
        dispatch({ type: "load_failed", ontologyId, error: toApiError(error) })
        return
      }

      if (cancelled) return
      dispatch({
        type: "load_succeeded",
        ontologyId,
        document: resource.document,
        etag: resource.etag,
      })
      if (createdResource !== null) finishCreatedOntologyHandoff(ontologyId)

      if (draftPromise === null) return
      let draft
      try {
        draft = await draftPromise
      } catch {
        // 草稿是可选的崩溃恢复通道：IndexedDB 故障不得覆盖已成功的 GET。
        // 不标记 checked，因此也不会删除这条未能读取的草稿；重载时可重试。
        return
      }
      if (cancelled) return
      if (draft === null) {
        dispatch({ type: "draft_checked" })
        return
      }
      if (ontologyDocumentsEqual(draft.candidate, resource.document)) {
        // 草稿与 acknowledged 一致：无需提示，交由统一清理副作用删除
        dispatch({ type: "draft_checked" })
        return
      }
      dispatch({ type: "draft_found", draft })
    })()

    return () => {
      cancelled = true
    }
  }, [api, draftStore, ontologyId, reloadVersion])

  const dirty = isOntologyEditorDirty(state)
  // 复用上方已完成的文档比较，避免最大 candidate 在同一 render 被 canonicalize 两次。
  const canClearDraft = canClearOntologyDraft(state, dirty)

  // 草稿持久化：candidate 变化（dirty）时防抖写入；干净时清除。
  // 依赖收窄到实际读取的原语，避免无关 dispatch 重置防抖计时器。
  const phase = state.phase
  const candidate = state.candidate
  const baseEtag = state.acknowledged?.etag
  useEffect(() => {
    if (draftStore === null || ontologyId === null) return
    if (phase !== "ready") return

    if (dirty) {
      if (candidate === null || baseEtag === undefined) return
      const timer = setTimeout(() => {
        // 有意吞掉错误：草稿持久化失败不影响编辑主流程，草稿仅用于崩溃恢复
        void draftStore
          .saveDraft({
            ontology_id: ontologyId,
            base_etag: baseEtag,
            candidate,
          })
          .catch(() => {})
      }, DRAFT_SAVE_DEBOUNCE_MS)
      return () => clearTimeout(timer)
    }

    if (canClearDraft)
      // 有意吞掉错误：草稿持久化失败不影响编辑主流程，草稿仅用于崩溃恢复
      void draftStore.clearDraft(ontologyId).catch(() => {})
    return undefined
  }, [draftStore, ontologyId, phase, candidate, baseEtag, dirty, canClearDraft])

  const reload = useCallback(() => {
    setReloadVersion((version) => version + 1)
  }, [])

  const save = useCallback(async (): Promise<boolean> => {
    const snapshot = stateRef.current
    if (
      snapshot.phase !== "ready" ||
      snapshot.ontologyId === null ||
      snapshot.candidate === null ||
      snapshot.acknowledged === null ||
      snapshot.inFlight !== null
    )
      return false
    if (!isOntologyEditorDirty(snapshot)) return true

    dispatch({ type: "save_started" })
    const result = await attemptOntologySave(
      { api, dispatch },
      {
        ontologyId: snapshot.ontologyId,
        document: snapshot.candidate,
        baseEtag: snapshot.acknowledged.etag,
      }
    )
    return result.outcome === "saved"
  }, [api])

  const reconcile = useCallback(
    async (resolution: "local" | "remote"): Promise<boolean> => {
      const snapshot = stateRef.current
      if (snapshot.conflict === null || snapshot.ontologyId === null)
        return false

      try {
        // 调和前重读一次，确保基于最新远端与 ETag
        const remote = await api.getOntology(snapshot.ontologyId)
        dispatch({
          type: "conflict_resolved",
          resolution,
          remote: { document: remote.document, etag: remote.etag },
        })
        return true
      } catch {
        return false
      }
    },
    [api]
  )

  const restoreDraft = useCallback(() => {
    dispatch({ type: "draft_restored" })
  }, [])

  const discardDraft = useCallback(() => {
    const currentOntologyId = stateRef.current.ontologyId
    if (draftStore !== null && currentOntologyId !== null)
      // 有意吞掉错误：草稿持久化失败不影响编辑主流程，草稿仅用于崩溃恢复
      void draftStore.clearDraft(currentOntologyId).catch(() => {})
    dispatch({ type: "draft_discarded" })
  }, [draftStore])

  const addObjectType = useCallback(
    (input: { name: string; display_name: string; description?: string }) => {
      const id = createUuidV7()
      dispatch({
        type: "object_type_added",
        objectType: { id, ...input, properties: [] },
      })
      return id
    },
    []
  )

  const updateObjectType = useCallback((objectType: OntologyObjectType) => {
    dispatch({ type: "object_type_updated", objectType })
  }, [])

  const removeObjectType = useCallback((objectTypeId: string) => {
    dispatch({ type: "object_type_removed", objectTypeId })
  }, [])

  const addProperty = useCallback(
    (
      objectTypeId: string,
      input: {
        name: string
        display_name: string
        description?: string
        value_type: OntologyPropertyValueType
        required: boolean
      }
    ) => {
      const id = createUuidV7()
      dispatch({
        type: "property_added",
        objectTypeId,
        property: { id, ...input },
      })
      return id
    },
    []
  )

  const updateProperty = useCallback(
    (objectTypeId: string, property: OntologyProperty) => {
      dispatch({ type: "property_updated", objectTypeId, property })
    },
    []
  )

  const removeProperty = useCallback(
    (objectTypeId: string, propertyId: string) => {
      dispatch({ type: "property_removed", objectTypeId, propertyId })
    },
    []
  )

  const addLinkType = useCallback(
    (input: {
      name: string
      display_name: string
      description?: string
      source_object_type_id: string
      target_object_type_id: string
      source_to_target: OntologyLinkCardinality
      target_to_source: OntologyLinkCardinality
    }) => {
      const id = createUuidV7()
      dispatch({ type: "link_type_added", linkType: { id, ...input } })
      return id
    },
    []
  )

  const updateLinkType = useCallback((linkType: OntologyLinkType) => {
    dispatch({ type: "link_type_updated", linkType })
  }, [])

  const removeLinkType = useCallback((linkTypeId: string) => {
    dispatch({ type: "link_type_removed", linkTypeId })
  }, [])

  const setPosition = useCallback(
    (objectTypeId: string, x: number, y: number) => {
      dispatch({ type: "position_set", objectTypeId, x, y })
    },
    []
  )

  return {
    state,
    dirty,
    reload,
    save,
    reconcile,
    restoreDraft,
    discardDraft,
    addObjectType,
    updateObjectType,
    removeObjectType,
    addProperty,
    updateProperty,
    removeProperty,
    addLinkType,
    updateLinkType,
    removeLinkType,
    setPosition,
  }
}

function toApiError(error: unknown): ApiError {
  if (error instanceof ApiError) return error
  return new ApiError(
    "connection_error",
    0,
    error instanceof Error ? error.message : "connection failed"
  )
}
