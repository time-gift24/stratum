import { thinkingLevels } from "@/lib/stratum/model-config"

export type ParameterControl =
  | {
      kind: "select"
      key: string
      label: string
      options: readonly { value: string; label: string }[]
    }
  | {
      kind: "boolean"
      key: string
      label: string
    }
  | {
      kind: "number"
      key: string
      label: string
      minimum?: number
      maximum?: number
      integer: boolean
    }
  | { kind: "text"; key: string; label: string }
  | { kind: "thinking"; key: "thinking"; label: string; options: readonly { value: string; label: string }[] }

export type ParameterControlsResult = {
  controls: readonly ParameterControl[]
  requiresRawFallback: boolean
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value)

export function parameterControls(schema: unknown): ParameterControlsResult {
  if (!isRecord(schema) || !isRecord(schema.properties))
    return { controls: [], requiresRawFallback: true }

  const controls: ParameterControl[] = []
  let requiresRawFallback = false
  for (const [key, property] of Object.entries(schema.properties)) {
    if (key === "thinking") {
      const levels = thinkingLevels(schema)
      if (levels.length > 0) {
        controls.push({
          kind: "thinking",
          key,
          label: "思考",
          options: levels.map((level) => ({ value: level.id, label: level.name })),
        })
      } else requiresRawFallback = true
      continue
    }
    if (!isRecord(property)) {
      requiresRawFallback = true
      continue
    }
    const label = typeof property.title === "string" ? property.title : key
    if (Array.isArray(property.enum) && property.enum.every((value) => typeof value === "string")) {
      controls.push({
        kind: "select",
        key,
        label,
        options: property.enum.map((value) => ({ value, label: value })),
      })
    } else if (property.type === "boolean") {
      controls.push({ kind: "boolean", key, label })
    } else if (property.type === "number" || property.type === "integer") {
      controls.push({
        kind: "number",
        key,
        label,
        minimum: typeof property.minimum === "number" ? property.minimum : undefined,
        maximum: typeof property.maximum === "number" ? property.maximum : undefined,
        integer: property.type === "integer",
      })
    } else if (property.type === "string") {
      controls.push({ kind: "text", key, label })
    } else {
      requiresRawFallback = true
    }
  }
  return { controls, requiresRawFallback }
}
