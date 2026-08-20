import type {
  AgentDefinitionInput,
  AgentDefinitionView,
  ProviderKind,
  ProviderView,
} from "@/lib/stratum/api"

export type AgentDraft = {
  agentName: string
  agentVersion: string
  model: string
  parameters: Record<string, unknown>
  tools: string[]
  prompt: string
}

export type ProviderDraft = {
  provider: ProviderKind
  apiKey: string
}

export type FormPhase = "loaded" | "dirty" | "saving" | "invalid" | "conflict"

export type ManagementFormState<T> = {
  phase: FormPhase
  dirty: boolean
  acknowledged: T
  draft: T
  etag: string
  message: string | null
  violations: Readonly<Record<string, string>>
  blockers: readonly { resource_type: string; name: string; message?: string }[]
}

export type AgentEditorRecord = {
  resource: AgentDefinitionView
  etag: string
}

export type ProviderEditorRecord = {
  resource: ProviderView
  etag: string
}

export type AgentSaveInput = AgentDefinitionInput
