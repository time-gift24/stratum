export type ModelConfig = {
  model: string
  parameters: Record<string, unknown>
}

export type ModelDisplayName = {
  provider: string | null
  model: string
}

export type ModelDescriptor = {
  model: string
  parameters_schema: unknown
}

export type AgentTemplateView = {
  agent_name: string
  /** Template author supplied tag; case-sensitive and not sortable. */
  version: string
  model_config: ModelConfig
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value)

export function schemaDefault(schema: unknown): Record<string, unknown> {
  if (!isRecord(schema) || !isRecord(schema.default)) return {}

  return structuredClone(schema.default)
}

export function configForTemplate(template: AgentTemplateView): ModelConfig {
  return structuredClone(template.model_config)
}

export function configForModel(descriptor: ModelDescriptor): ModelConfig {
  return {
    model: descriptor.model,
    parameters: schemaDefault(descriptor.parameters_schema),
  }
}

export function modelDisplayName(modelId: string): ModelDisplayName {
  const separator = modelId.indexOf(":")
  if (separator <= 0 || separator === modelId.length - 1)
    return { provider: null, model: modelId }

  const provider = modelId.slice(0, separator)
  return {
    provider: provider.charAt(0).toUpperCase() + provider.slice(1),
    model: modelId.slice(separator + 1),
  }
}

export type ThinkingLevel = { id: string; name: string }

const displayLevelName = (id: string): string =>
  id === "disabled" ? "关闭" : id.charAt(0).toUpperCase() + id.slice(1)

/**
 * 从模型的 parameters_schema 解析可用的 thinking 等级：
 * `properties.thinking.oneOf` 里的 disabled 项 + enabled 项的 reasoning_effort enum。
 * schema 无 thinking 配置时返回空数组（UI 应隐藏 Thinking 控件）。
 */
export function thinkingLevels(schema: unknown): readonly ThinkingLevel[] {
  if (!isRecord(schema) || !isRecord(schema.properties)) return []

  const thinking = schema.properties.thinking
  if (!isRecord(thinking) || !Array.isArray(thinking.oneOf)) return []

  const levels: ThinkingLevel[] = []
  for (const option of thinking.oneOf) {
    if (!isRecord(option) || !isRecord(option.properties)) continue

    const type = option.properties.type
    if (!isRecord(type)) continue
    if (type.const === "disabled") {
      levels.push({ id: "disabled", name: displayLevelName("disabled") })
      continue
    }
    if (type.const !== "enabled") continue

    const reasoningEffort = option.properties.reasoning_effort
    if (!isRecord(reasoningEffort) || !Array.isArray(reasoningEffort.enum))
      continue
    for (const value of reasoningEffort.enum) {
      if (typeof value === "string")
        levels.push({ id: value, name: displayLevelName(value) })
    }
  }

  return levels
}

/** 读取当前 parameters 里生效的 thinking 等级 id；未配置时返回 null。 */
export function currentThinkingLevel(
  parameters: Record<string, unknown>
): string | null {
  const thinking = parameters.thinking
  if (!isRecord(thinking)) return null

  if (thinking.type === "disabled") return "disabled"
  if (
    thinking.type === "enabled" &&
    typeof thinking.reasoning_effort === "string"
  )
    return thinking.reasoning_effort
  return null
}

export function withThinkingLevel(
  parameters: Record<string, unknown>,
  level: string
): Record<string, unknown> {
  return {
    ...parameters,
    thinking:
      level === "disabled"
        ? { type: "disabled" }
        : { type: "enabled", reasoning_effort: level },
  }
}
