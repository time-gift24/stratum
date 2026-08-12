import type {
  AgentDefinitionInput,
  AgentDefinitionView,
  ManagedModelView,
  ProviderView,
} from "@/lib/stratum/api"
import { parse, stringify, TomlError } from "smol-toml"
import type {
  AgentDraft,
  ModelDraft,
  ProviderDraft,
} from "@/features/studio-management/types"

export function agentViewToDraft(view: AgentDefinitionView): AgentDraft {
  return {
    agentName: view.agent_name,
    model: view.model,
    parameters: structuredClone(view.model_parameters),
    tools: [...view.tools],
    prompt: view.prompt,
  }
}

export function agentDraftToInput(draft: AgentDraft): AgentDefinitionInput {
  return {
    agent_name: draft.agentName.trim(),
    model: draft.model,
    model_parameters: structuredClone(draft.parameters),
    tools: draft.tools.map((tool) => tool.trim()).filter(Boolean),
    prompt: draft.prompt.trim(),
  }
}

export function providerViewToDraft(view: ProviderView): ProviderDraft {
  return { provider: view.provider, apiKey: "" }
}

export function modelViewToDraft(view: ManagedModelView): ModelDraft {
  return { provider: view.provider, modelName: view.name }
}

export function encodeAgentToml(draft: AgentDraft): string {
  return stringify({
    model: draft.model,
    tools: draft.tools,
    prompt: draft.prompt,
    ...(Object.keys(draft.parameters).length === 0
      ? {}
      : { model_parameters: draft.parameters }),
  })
}

export type RawAgentParseResult =
  { ok: true; draft: AgentDraft } | { ok: false; line: number; message: string }

const AGENT_KEYS = new Set(["model", "model_parameters", "tools", "prompt"])

const isPlainRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" &&
  value !== null &&
  !Array.isArray(value) &&
  Object.getPrototypeOf(value) === Object.prototype

const isJsonValue = (value: unknown): boolean => {
  if (typeof value === "string" || typeof value === "boolean") return true
  if (typeof value === "number") return Number.isFinite(value)
  if (Array.isArray(value)) return value.every(isJsonValue)
  if (isPlainRecord(value)) return Object.values(value).every(isJsonValue)
  return false
}

export const isTomlCompatibleParameters = (
  value: unknown
): value is Record<string, unknown> =>
  isPlainRecord(value) && Object.values(value).every(isJsonValue)

const keyLine = (source: string, key: string): number => {
  const index = source
    .split("\n")
    .findIndex((line) => line.trimStart().startsWith(key))
  return index < 0 ? 1 : index + 1
}

export function parseAgentToml(source: string): RawAgentParseResult {
  try {
    const document = parse(source)
    const unknown = Object.keys(document).find((key) => !AGENT_KEYS.has(key))
    if (unknown)
      return {
        ok: false,
        line: keyLine(source, unknown),
        message: `未知字段 ${unknown}`,
      }

    const { model, tools, prompt } = document
    const parameters = document.model_parameters ?? {}
    if (
      typeof model !== "string" ||
      !Array.isArray(tools) ||
      !tools.every((tool) => typeof tool === "string") ||
      typeof prompt !== "string" ||
      !isTomlCompatibleParameters(parameters)
    ) {
      return { ok: false, line: 1, message: "字段类型不符合 Agent 配置" }
    }
    return {
      ok: true,
      draft: { agentName: "", model, tools, prompt, parameters },
    }
  } catch (error) {
    if (
      typeof error === "object" &&
      error !== null &&
      "line" in error &&
      "message" in error
    ) {
      return error as { ok: false; line: number; message: string }
    }
    if (error instanceof TomlError)
      return { ok: false, line: error.line, message: error.message }
    return {
      ok: false,
      line: 1,
      message: error instanceof Error ? error.message : "无法解析 TOML",
    }
  }
}

export function encodeProviderRaw(view: ProviderView): string {
  return stringify({
    provider: view.provider,
    credential_configured: view.credential_configured,
    models_count: view.models_count,
  })
}

export function encodeModelSchema(view: ManagedModelView): string {
  return JSON.stringify(view.parameter_schema, null, 2)
}
