import type { Dispatch } from "react"

import type { ManagementFormState } from "@/features/studio-management/types"
import { ApiError } from "@/lib/stratum/api"
import type { FieldViolation, ResourceBlocker } from "@/lib/stratum/api"

export type FormAction<T> =
  | { type: "edit"; draft: T; forceDirty?: boolean }
  | { type: "save" }
  | { type: "test" }
  | { type: "invalid"; message: string; violations?: readonly FieldViolation[] }
  | { type: "conflict"; message: string }
  | { type: "blocked"; message: string; blockers: readonly ResourceBlocker[] }
  | { type: "acknowledge"; value: T; etag: string; message?: string }
  | {
      type: "settle"
      message?: string
      restorePhase?: "loaded" | "dirty" | "invalid" | "conflict"
    }
  | { type: "refresh"; value: T; etag: string }
  | { type: "reload"; value: T; etag: string }

export function initialFormState<T>(
  value: T,
  etag = ""
): ManagementFormState<T> {
  return {
    phase: "loaded",
    dirty: false,
    acknowledged: value,
    draft: value,
    etag,
    message: null,
    violations: {},
    blockers: [],
  }
}

const violationMap = (
  violations: readonly FieldViolation[] = []
): Readonly<Record<string, string>> =>
  Object.fromEntries(violations.map((item) => [item.field, item.message]))

function draftsEqual(left: unknown, right: unknown): boolean {
  if (Object.is(left, right)) return true
  if (left === null || right === null) return false
  if (typeof left !== "object" || typeof right !== "object") return false

  if (Array.isArray(left) || Array.isArray(right)) {
    return (
      Array.isArray(left) &&
      Array.isArray(right) &&
      left.length === right.length &&
      left.every((value, index) => draftsEqual(value, right[index]))
    )
  }

  const leftRecord = left as Readonly<Record<string, unknown>>
  const rightRecord = right as Readonly<Record<string, unknown>>
  const leftKeys = Object.keys(leftRecord)
  const rightKeys = Object.keys(rightRecord)
  return (
    leftKeys.length === rightKeys.length &&
    leftKeys.every(
      (key) =>
        Object.hasOwn(rightRecord, key) &&
        draftsEqual(leftRecord[key], rightRecord[key])
    )
  )
}

export function formReducer<T>(
  state: ManagementFormState<T>,
  action: FormAction<T>
): ManagementFormState<T> {
  switch (action.type) {
    case "edit": {
      const dirty =
        action.forceDirty || !draftsEqual(action.draft, state.acknowledged)
      return {
        ...state,
        dirty,
        phase: dirty ? "dirty" : "loaded",
        draft: action.draft,
        message: null,
        violations: {},
        blockers: [],
      }
    }
    case "save":
      return { ...state, phase: "saving", message: null, blockers: [] }
    case "test":
      return { ...state, phase: "testing", message: null }
    case "invalid":
      return {
        ...state,
        phase: "invalid",
        message: action.message,
        violations: violationMap(action.violations),
      }
    case "conflict":
      return { ...state, phase: "conflict", message: action.message }
    case "blocked":
      return {
        ...state,
        phase: "invalid",
        message: action.message,
        blockers: action.blockers,
      }
    case "acknowledge":
      return {
        phase: "loaded",
        dirty: false,
        acknowledged: action.value,
        draft: action.value,
        etag: action.etag,
        message: action.message ?? "已保存。变更会用于之后新建的 Agent。",
        violations: {},
        blockers: [],
      }
    case "settle":
      return {
        ...state,
        phase: action.restorePhase ?? "loaded",
        message: action.message ?? null,
      }
    case "refresh":
      return state.dirty ? state : initialFormState(action.value, action.etag)
    case "reload":
      return initialFormState(action.value, action.etag)
  }
}

/**
 * 管理 API 错误 → 表单 phase：412 → conflict、409 → blocked、
 * 400/422 → invalid（带字段级 violations）、其他 → invalid（fallback 文案）。
 */
export function dispatchApiError<T>(
  dispatch: Dispatch<FormAction<T>>,
  caught: unknown,
  messages: { conflict: string; fallback: string }
): void {
  if (caught instanceof ApiError && caught.status === 412) {
    dispatch({ type: "conflict", message: messages.conflict })
  } else if (caught instanceof ApiError && caught.status === 409) {
    dispatch({
      type: "blocked",
      message: caught.message,
      blockers: caught.details.blockers ?? [],
    })
  } else if (
    caught instanceof ApiError &&
    (caught.status === 400 || caught.status === 422)
  ) {
    dispatch({
      type: "invalid",
      message: caught.message,
      violations: caught.details.violations,
    })
  } else {
    dispatch({
      type: "invalid",
      message: caught instanceof Error ? caught.message : messages.fallback,
    })
  }
}
