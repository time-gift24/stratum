"use client"

import { useState } from "react"

import {
  currentThinkingLevel,
  withThinkingLevel,
} from "@/lib/stratum/model-config"
import { parameterControls } from "@/features/studio-management/schema-controls"
import { isTomlCompatibleParameters } from "@/features/studio-management/transforms"
import {
  Field,
  StudioInput,
  StudioTextarea,
  controlClass,
} from "@/components/stratum/studio/primitives"

export function ParameterFields({
  schema,
  parameters,
  onChange,
  onInvalidEdit,
  onValidityChange,
}: {
  schema: unknown
  parameters: Record<string, unknown>
  onChange: (parameters: Record<string, unknown>) => void
  onInvalidEdit?: () => void
  onValidityChange?: (valid: boolean) => void
}) {
  const parsed = parameterControls(schema)

  if (parsed.controls.length === 0 || parsed.requiresRawFallback) {
    return (
      <RawParameterFields
        parameters={parameters}
        onChange={onChange}
        onInvalidEdit={onInvalidEdit}
        onValidityChange={onValidityChange}
      />
    )
  }

  return (
    <div className="grid gap-5 sm:grid-cols-2">
      {parsed.controls.map((control) => {
        if (control.kind === "thinking") {
          const current =
            currentThinkingLevel(parameters) ?? control.options[0]?.value ?? ""
          return (
            <Field key={control.key} label={control.label}>
              <select
                className={`${controlClass} h-9 rounded-md border px-3 text-sm outline-none focus-visible:ring-2`}
                value={current}
                onChange={(event) =>
                  onChange(withThinkingLevel(parameters, event.target.value))
                }
              >
                {control.options.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </Field>
          )
        }
        if (control.kind === "select") {
          return (
            <Field key={control.key} label={control.label}>
              <select
                className={`${controlClass} h-9 rounded-md border px-3 text-sm outline-none focus-visible:ring-2`}
                value={
                  typeof parameters[control.key] === "string"
                    ? String(parameters[control.key])
                    : ""
                }
                onChange={(event) =>
                  onChange({ ...parameters, [control.key]: event.target.value })
                }
              >
                {control.options.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </Field>
          )
        }
        if (control.kind === "boolean") {
          return (
            <label
              key={control.key}
              className="flex min-h-11 items-center gap-3 text-sm font-medium"
            >
              <input
                type="checkbox"
                className="size-4 accent-primary"
                checked={parameters[control.key] === true}
                onChange={(event) =>
                  onChange({
                    ...parameters,
                    [control.key]: event.target.checked,
                  })
                }
              />
              {control.label}
            </label>
          )
        }
        return (
          <Field key={control.key} label={control.label}>
            <StudioInput
              type={control.kind === "number" ? "number" : "text"}
              min={control.kind === "number" ? control.minimum : undefined}
              max={control.kind === "number" ? control.maximum : undefined}
              step={
                control.kind === "number" && !control.integer
                  ? "any"
                  : undefined
              }
              value={String(parameters[control.key] ?? "")}
              onChange={(event) =>
                onChange({
                  ...parameters,
                  [control.key]:
                    control.kind === "number"
                      ? event.target.valueAsNumber
                      : event.target.value,
                })
              }
            />
          </Field>
        )
      })}
    </div>
  )
}

function RawParameterFields({
  parameters,
  onChange,
  onInvalidEdit,
  onValidityChange,
}: {
  parameters: Record<string, unknown>
  onChange: (parameters: Record<string, unknown>) => void
  onInvalidEdit?: () => void
  onValidityChange?: (valid: boolean) => void
}) {
  const [source, setSource] = useState(() =>
    JSON.stringify(parameters, null, 2)
  )
  const [error, setError] = useState<string | null>(null)

  return (
    <Field
      label="Model parameters"
      error={error ?? undefined}
      hint="该模型包含暂不支持的 schema 形状，请使用 JSON 参数。"
    >
      <StudioTextarea
        rows={8}
        className="font-mono text-sm"
        value={source}
        onChange={(event) => {
          const next = event.target.value
          setSource(next)
          try {
            const value: unknown = JSON.parse(next)
            if (!isTomlCompatibleParameters(value))
              throw new Error(
                "参数必须是 TOML 可表示的 JSON object，不能包含 null 或非有限数字"
              )
            setError(null)
            onValidityChange?.(true)
            onChange(value as Record<string, unknown>)
          } catch (caught) {
            const message =
              caught instanceof Error ? caught.message : "JSON 无法解析"
            setError(message)
            onValidityChange?.(false)
            onInvalidEdit?.()
          }
        }}
      />
    </Field>
  )
}
