import type {
  OntologyProperty,
  OntologyPropertyValueType,
} from "./types"
import { isValidOntologyName } from "./validation"

/**
 * 属性草稿的共享规则：命名提示、值类型全集、行内校验与自动命名。
 * 画布节点（ontology-node.tsx）与编辑面板（object-type-panel.tsx）
 * 共用同一份，避免两处实现漂移。
 */

export const PROPERTY_NAME_HINT =
  "需匹配 ^[a-z][a-z0-9_]{0,63}$（小写字母开头，可含数字与下划线）"

export const PROPERTY_VALUE_TYPES: readonly OntologyPropertyValueType[] = [
  "string",
  "integer",
  "number",
  "boolean",
  "date",
  "date_time",
]

/** 属性 name 行内校验：合法返回 null，否则返回提示文案 */
export function validatePropertyName(next: string): string | null {
  return isValidOntologyName(next) ? null : PROPERTY_NAME_HINT
}

/** 显示名行内校验：非空即可 */
export function validatePropertyDisplayName(next: string): string | null {
  return next.trim() === "" ? "显示名不能为空" : null
}

/** 新属性的自动命名：field_n，从 properties.length + 1 起取不冲突的序号 */
export function nextPropertyName(
  properties: readonly OntologyProperty[]
): string {
  const taken = new Set(properties.map((property) => property.name))
  let index = properties.length + 1
  while (taken.has(`field_${index}`)) index += 1
  return `field_${index}`
}
