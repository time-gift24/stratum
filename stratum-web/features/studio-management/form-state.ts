import type {
  FormPhase,
  ManagementFormState,
} from "@/features/studio-management/types"
import type { FieldViolation, ResourceBlocker } from "@/lib/stratum/api"

export type FormAction<T> =
  | { type: "edit"; draft: T }
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
  | { type: "reload"; value: T; etag: string }

export function initialFormState<T>(
  value: T,
  etag = ""
): ManagementFormState<T> {
  return {
    phase: "loaded",
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

export function formReducer<T>(
  state: ManagementFormState<T>,
  action: FormAction<T>
): ManagementFormState<T> {
  switch (action.type) {
    case "edit":
      return {
        ...state,
        phase: "dirty",
        draft: action.draft,
        message: null,
        violations: {},
        blockers: [],
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
    case "reload":
      return initialFormState(action.value, action.etag)
  }
}

export function isDirtyPhase(phase: FormPhase): boolean {
  return (
    phase === "dirty" ||
    phase === "saving" ||
    phase === "invalid" ||
    phase === "conflict"
  )
}
