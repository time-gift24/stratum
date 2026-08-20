import type {
  AgentDefinitionInput,
  AgentDefinitionView,
  ProviderView,
} from "@/lib/stratum/api"
import { parse, stringify, TomlError } from "smol-toml"
import type {
  AgentDraft,
  ProviderDraft,
} from "@/features/studio-management/types"

export function agentViewToDraft(view: AgentDefinitionView): AgentDraft {
  return {
    agentName: view.agent_name,
    agentVersion: view.agent_version,
    model: view.model,
    parameters: structuredClone(view.model_parameters),
    tools: [...view.tools],
    prompt: view.prompt,
  }
}

export function agentDraftToInput(draft: AgentDraft): AgentDefinitionInput {
  return {
    agent_name: draft.agentName,
    agent_version: draft.agentVersion,
    model: draft.model,
    model_parameters: structuredClone(draft.parameters),
    tools: [...draft.tools],
    prompt: draft.prompt,
  }
}

export function agentVersionValidationMessage(value: string): string | null {
  if (value.trim() === "") return "版本标签不能为空"
  if (value.trim() !== value) return "版本标签首尾不能有空白"
  if (/\p{Cc}/u.test(value)) return "版本标签不能包含控制字符"
  if (new TextEncoder().encode(value).length > 128)
    return "版本标签不能超过 128 字节"
  return null
}

export function providerViewToDraft(view: ProviderView): ProviderDraft {
  return { provider: view.provider, apiKey: "" }
}

export function encodeAgentToml(draft: AgentDraft): string {
  return stringify({
    agent_version: draft.agentVersion,
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

const AGENT_KEYS = new Set([
  "agent_version",
  "model",
  "model_parameters",
  "tools",
  "prompt",
])

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

    const { agent_version: agentVersion, model, tools, prompt } = document
    const parameters = document.model_parameters ?? {}
    if (
      typeof agentVersion !== "string" ||
      typeof model !== "string" ||
      !Array.isArray(tools) ||
      !tools.every((tool) => typeof tool === "string") ||
      typeof prompt !== "string" ||
      !isTomlCompatibleParameters(parameters)
    ) {
      return { ok: false, line: 1, message: "字段类型不符合 Agent 配置" }
    }
    const versionError = agentVersionValidationMessage(agentVersion)
    if (versionError)
      return {
        ok: false,
        line: keyLine(source, "agent_version"),
        message: versionError,
      }
    if (model.trim() === "" || model.trim() !== model)
      return {
        ok: false,
        line: keyLine(source, "model"),
        message: "Model 不能为空或包含首尾空白",
      }
    if (prompt.trim() === "")
      return {
        ok: false,
        line: keyLine(source, "prompt"),
        message: "System prompt 不能为空",
      }
    if (tools.some((tool) => tool.trim() === "" || tool.trim() !== tool))
      return {
        ok: false,
        line: keyLine(source, "tools"),
        message: "Tool 名称不能为空或包含首尾空白",
      }
    if (new Set(tools).size !== tools.length)
      return {
        ok: false,
        line: keyLine(source, "tools"),
        message: "Tool 名称不能重复",
      }
    return {
      ok: true,
      draft: {
        agentName: "",
        agentVersion,
        model,
        tools,
        prompt,
        parameters,
      },
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
