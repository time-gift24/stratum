import { ApiError } from "@/lib/stratum/api"
import type { OntologyDraft } from "@/features/ontology-editor/recovery"
import type {
  OntologyDocument,
  OntologyLinkType,
  OntologyObjectType,
  OntologyProperty,
  OntologyViolation,
} from "@/features/ontology-editor/types"

// 保存状态机（docs/ontology/API.md「Frontend save state」）：
// acknowledged = 服务端确认的最近文档与 ETag；candidate = 画布展示的可变本地文档；
// inFlight = 当前 PUT 尝试的不可变快照与其 base ETag。
export type AcknowledgedOntology = {
  document: OntologyDocument
  etag: string
}

export type InFlightSave = {
  document: OntologyDocument
  baseEtag: string
}

export type OntologyEditorPhase =
  "idle" | "loading" | "ready" | "missing" | "error"

export type OntologyEditorState = {
  ontologyId: string | null
  phase: OntologyEditorPhase
  error: ApiError | null
  acknowledged: AcknowledgedOntology | null
  candidate: OntologyDocument | null
  inFlight: InFlightSave | null
  // 412：已重读的远端最新资源，等待用户显式调和
  conflict: { remote: AcknowledgedOntology } | null
  // 422：按响应原序保留（path、code 排序由服务端保证）
  violations: readonly OntologyViolation[] | null
  // 422 path 必须按实际提交的快照解析，不能按已继续编辑的 candidate 解析
  violationDocument: OntologyDocument | null
  // 超时/响应丢失且重读确认未提交、或其它保存失败
  saveError: ApiError | null
  // 当前加载轮次的本地草稿已读取完毕；load_started 会将其重置
  draftChecked: boolean
  draftAvailable: OntologyDraft | null
}

export type OntologyEditorAction =
  | { type: "load_started"; ontologyId: string }
  | {
      type: "load_succeeded"
      ontologyId: string
      document: OntologyDocument
      etag: string
    }
  | { type: "load_failed"; ontologyId: string; error: ApiError }
  | { type: "draft_checked" }
  | { type: "draft_found"; draft: OntologyDraft }
  | { type: "draft_restored" }
  | { type: "draft_discarded" }
  | { type: "object_type_added"; objectType: OntologyObjectType }
  | { type: "object_type_updated"; objectType: OntologyObjectType }
  | { type: "object_type_removed"; objectTypeId: string }
  | { type: "property_added"; objectTypeId: string; property: OntologyProperty }
  | {
      type: "property_updated"
      objectTypeId: string
      property: OntologyProperty
    }
  | { type: "property_removed"; objectTypeId: string; propertyId: string }
  | { type: "link_type_added"; linkType: OntologyLinkType }
  | { type: "link_type_updated"; linkType: OntologyLinkType }
  | { type: "link_type_removed"; linkTypeId: string }
  | { type: "position_set"; objectTypeId: string; x: number; y: number }
  | { type: "save_started" }
  | { type: "save_succeeded"; etag: string }
  | { type: "save_conflict"; remote: AcknowledgedOntology }
  | { type: "save_invalid"; violations: readonly OntologyViolation[] }
  | { type: "save_failed"; error: ApiError }
  | {
      type: "conflict_resolved"
      resolution: "local" | "remote"
      remote: AcknowledgedOntology
    }

export const initialOntologyEditorState: OntologyEditorState = {
  ontologyId: null,
  phase: "idle",
  error: null,
  acknowledged: null,
  candidate: null,
  inFlight: null,
  conflict: null,
  violations: null,
  violationDocument: null,
  saveError: null,
  draftChecked: false,
  draftAvailable: null,
}

// 键序无关的深度相等：dirty 判定与「远端是否等于 in_flight」都依赖它。
// 先做引用与集合大小的快路径，避免每次 dispatch 都对两份文档做全量 canonicalize。
export function ontologyDocumentsEqual(
  left: OntologyDocument,
  right: OntologyDocument
): boolean {
  if (left === right) return true
  if (
    left.object_types.length !== right.object_types.length ||
    left.link_types.length !== right.link_types.length ||
    left.canvas.positions.length !== right.canvas.positions.length
  )
    return false
  return canonicalize(left) === canonicalize(right)
}

function canonicalize(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(canonicalize).join(",")}]`
  if (typeof value === "object" && value !== null) {
    const entries = Object.entries(value)
      .filter(([, entryValue]) => entryValue !== undefined)
      .sort(([leftKey], [rightKey]) => (leftKey < rightKey ? -1 : 1))
    const body = entries
      .map(
        ([key, entryValue]) =>
          `${JSON.stringify(key)}:${canonicalize(entryValue)}`
      )
      .join(",")
    return `{${body}}`
  }
  return JSON.stringify(value) ?? "null"
}

export function isOntologyEditorDirty(state: OntologyEditorState): boolean {
  return (
    state.candidate !== null &&
    state.acknowledged !== null &&
    !ontologyDocumentsEqual(state.candidate, state.acknowledged.document)
  )
}

// 只有当前加载轮次已检查草稿、没有待用户处置的候选且编辑器干净时，
// 才可删除 IndexedDB 记录。这同时避免同 ontology 重载沿用上轮检查结果。
export function canClearOntologyDraft(
  state: OntologyEditorState,
  dirty = isOntologyEditorDirty(state)
): boolean {
  return (
    state.phase === "ready" &&
    state.draftChecked &&
    state.draftAvailable === null &&
    !dirty
  )
}

export function ontologyEditorReducer(
  state: OntologyEditorState,
  action: OntologyEditorAction
): OntologyEditorState {
  switch (action.type) {
    case "load_started":
      return {
        ...initialOntologyEditorState,
        ontologyId: action.ontologyId,
        phase: "loading",
      }
    case "load_succeeded":
      if (state.ontologyId !== action.ontologyId) return state
      return {
        ...initialOntologyEditorState,
        ontologyId: action.ontologyId,
        phase: "ready",
        acknowledged: { document: action.document, etag: action.etag },
        candidate: structuredClone(action.document),
      }
    case "load_failed":
      if (state.ontologyId !== action.ontologyId) return state
      return {
        ...state,
        phase: action.error.status === 404 ? "missing" : "error",
        error: action.error,
      }
    case "draft_checked":
      if (state.phase !== "ready") return state
      return { ...state, draftChecked: true }
    case "draft_found":
      if (
        state.phase !== "ready" ||
        action.draft.ontology_id !== state.ontologyId
      )
        return state
      return { ...state, draftChecked: true, draftAvailable: action.draft }
    case "draft_restored":
      if (state.draftAvailable === null) return state
      return {
        ...state,
        candidate: structuredClone(state.draftAvailable.candidate),
        draftAvailable: null,
        violations: null,
        violationDocument: null,
      }
    case "draft_discarded":
      return { ...state, draftAvailable: null }

    case "object_type_added":
      return updateCandidate(state, (document) => ({
        ...document,
        object_types: [...document.object_types, action.objectType],
      }))
    case "object_type_updated":
      return updateCandidate(state, (document) => ({
        ...document,
        object_types: document.object_types.map((objectType) =>
          objectType.id === action.objectType.id
            ? action.objectType
            : objectType
        ),
      }))
    case "object_type_removed":
      // 级联：引用它的 link type 与其画布位置必须一并移除（契约要求）
      return updateCandidate(state, (document) => ({
        ...document,
        object_types: document.object_types.filter(
          (objectType) => objectType.id !== action.objectTypeId
        ),
        link_types: document.link_types.filter(
          (linkType) =>
            linkType.source_object_type_id !== action.objectTypeId &&
            linkType.target_object_type_id !== action.objectTypeId
        ),
        canvas: {
          positions: document.canvas.positions.filter(
            (position) => position.object_type_id !== action.objectTypeId
          ),
        },
      }))
    case "property_added":
      return updateObjectType(state, action.objectTypeId, (objectType) => ({
        ...objectType,
        properties: [...objectType.properties, action.property],
      }))
    case "property_updated":
      return updateObjectType(state, action.objectTypeId, (objectType) => ({
        ...objectType,
        properties: objectType.properties.map((property) =>
          property.id === action.property.id ? action.property : property
        ),
      }))
    case "property_removed":
      return updateObjectType(state, action.objectTypeId, (objectType) => ({
        ...objectType,
        properties: objectType.properties.filter(
          (property) => property.id !== action.propertyId
        ),
      }))
    case "link_type_added":
      return updateCandidate(state, (document) => ({
        ...document,
        link_types: [...document.link_types, action.linkType],
      }))
    case "link_type_updated":
      return updateCandidate(state, (document) => ({
        ...document,
        link_types: document.link_types.map((linkType) =>
          linkType.id === action.linkType.id ? action.linkType : linkType
        ),
      }))
    case "link_type_removed":
      return updateCandidate(state, (document) => ({
        ...document,
        link_types: document.link_types.filter(
          (linkType) => linkType.id !== action.linkTypeId
        ),
      }))
    case "position_set":
      return updateCandidate(state, (document) => {
        // 位置必须引用同文档内存在的 object type
        if (
          !document.object_types.some(
            (objectType) => objectType.id === action.objectTypeId
          )
        )
          return document
        const next = {
          object_type_id: action.objectTypeId,
          x: action.x,
          y: action.y,
        }
        const exists = document.canvas.positions.some(
          (position) => position.object_type_id === action.objectTypeId
        )
        return {
          ...document,
          canvas: {
            positions: exists
              ? document.canvas.positions.map((position) =>
                  position.object_type_id === action.objectTypeId
                    ? next
                    : position
                )
              : [...document.canvas.positions, next],
          },
        }
      })

    case "save_started":
      if (
        state.phase !== "ready" ||
        state.candidate === null ||
        state.acknowledged === null ||
        state.inFlight !== null
      )
        return state
      return {
        ...state,
        inFlight: {
          document: state.candidate,
          baseEtag: state.acknowledged.etag,
        },
        conflict: null,
        violations: null,
        violationDocument: null,
        saveError: null,
      }
    case "save_succeeded":
      // 仅确认 in_flight 快照；candidate 若已前进则原样保留（dirty 维持 true）
      if (state.inFlight === null) return state
      return {
        ...state,
        acknowledged: { document: state.inFlight.document, etag: action.etag },
        inFlight: null,
        conflict: null,
        violations: null,
        violationDocument: null,
        saveError: null,
      }
    case "save_conflict":
      // 412：candidate 保持原样，等待用户显式调和，绝不静默重试
      return {
        ...state,
        inFlight: null,
        conflict: { remote: action.remote },
      }
    case "save_invalid":
      // 422：candidate 保持原样，path 始终以这次实际提交的快照映射。
      if (state.inFlight === null) return state
      return {
        ...state,
        inFlight: null,
        violations: action.violations,
        violationDocument: state.inFlight.document,
      }
    case "save_failed":
      return { ...state, inFlight: null, saveError: action.error }
    case "conflict_resolved":
      if (state.conflict === null) return state
      return {
        ...state,
        acknowledged: action.remote,
        candidate:
          action.resolution === "remote"
            ? structuredClone(action.remote.document)
            : state.candidate,
        inFlight: null,
        conflict: null,
        violations: null,
        violationDocument: null,
        saveError: null,
      }
  }
}

function updateCandidate(
  state: OntologyEditorState,
  update: (document: OntologyDocument) => OntologyDocument
): OntologyEditorState {
  if (state.candidate === null) return state
  const candidate = update(state.candidate)
  // 编辑使旧的 422 violations 定位失效，随 candidate 变化一并清除
  return candidate === state.candidate
    ? state
    : { ...state, candidate, violations: null, violationDocument: null }
}

function updateObjectType(
  state: OntologyEditorState,
  objectTypeId: string,
  update: (objectType: OntologyObjectType) => OntologyObjectType
): OntologyEditorState {
  return updateCandidate(state, (document) => {
    if (
      !document.object_types.some(
        (objectType) => objectType.id === objectTypeId
      )
    )
      return document
    return {
      ...document,
      object_types: document.object_types.map((objectType) =>
        objectType.id === objectTypeId ? update(objectType) : objectType
      ),
    }
  })
}
