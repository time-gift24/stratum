export const knowledgeStatuses = [
  "当前事实",
  "强制不变量",
  "失败模式",
  "恢复路径",
  "潜在风险",
  "明确延期",
  "非目标",
] as const

export type KnowledgeStatus = (typeof knowledgeStatuses)[number]

export type EvidenceKind = "实现" | "测试" | "规范" | "运维"

export interface EvidenceRef {
  readonly kind: EvidenceKind
  readonly path: string
  readonly symbol: string
  readonly context: string
  readonly note: string
}

export interface NarrativeCase {
  readonly title: string
  readonly summary: string
  readonly steps: readonly string[]
  readonly eventRows?: readonly EventRow[]
}

export interface EventRow {
  readonly seq: string
  readonly type: string
  readonly scope: string
  readonly fact: string
  readonly tone: "fact" | "internal" | "failure" | "recovery"
}

export interface RiskNote {
  readonly level: "已登记" | "审阅提示"
  readonly title: string
  readonly trigger: string
  readonly impact: string
  readonly owner: string
  readonly verification: string
}

export interface ConceptBoundaries {
  readonly deferred: string
  readonly nonGoal: string
}

export interface ConceptEntry {
  readonly id: string
  readonly term: string
  readonly english?: string
  readonly definition: string
  readonly statuses: readonly KnowledgeStatus[]
  readonly avoid: readonly string[]
  readonly normal: NarrativeCase
  readonly failure: NarrativeCase
  readonly recovery: NarrativeCase
  readonly risks: readonly RiskNote[]
  readonly boundaries: ConceptBoundaries
  readonly invariant: string
  readonly evidence: readonly EvidenceRef[]
}

export type VisualKind =
  | "first-principles"
  | "identity-map"
  | "iteration-ledger"
  | "hook-journal"
  | "crash-recovery"
  | "compaction-modes"

export interface ManualChapter {
  readonly id: string
  readonly navLabel: string
  readonly title: string
  readonly thesis: string
  readonly visual: VisualKind
  readonly concepts: readonly ConceptEntry[]
}

export interface ContextManual {
  readonly title: string
  readonly description: string
  readonly checkedAtCommit: string
  readonly chapters: readonly ManualChapter[]
}

export const todoStatuses = [
  "待开始",
  "进行中",
  "受阻",
  "待讨论",
  "明确延期",
  "已完成",
  "已取代",
] as const

export type TodoStatus = (typeof todoStatuses)[number]

export interface TodoItem {
  readonly id: string
  readonly title: string
  readonly status: TodoStatus
  readonly acceptance?: string
  readonly note?: string
}

export interface TodoInitiative {
  readonly id: string
  readonly area: "Agent DIY" | "Workflow" | "平台基础" | "治理历史"
  readonly title: string
  readonly status: TodoStatus
  readonly goal: string
  readonly dependencies: readonly string[]
  readonly items: readonly TodoItem[]
  readonly acceptance: readonly string[]
}

export interface TodoCoordination {
  readonly parallelTracks: readonly string[]
  readonly serialRules: readonly string[]
}

export interface TodoLedger {
  readonly title: string
  readonly principles: readonly string[]
  readonly coordination: TodoCoordination
  readonly initiatives: readonly TodoInitiative[]
  readonly deferredBoundaries: readonly string[]
}
