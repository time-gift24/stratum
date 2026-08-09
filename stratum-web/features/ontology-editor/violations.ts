import { parseJsonPointer } from "@/features/ontology-editor/pointer"
import type {
  OntologyDocument,
  OntologyViolation,
} from "@/features/ontology-editor/types"

// 422 violation 的 JSON Pointer path 映射结果；无法定位到具体实体时为 document。
export type ViolationTarget = {
  kind: "objectType" | "property" | "linkType" | "canvas" | "document"
  objectTypeId?: string
  propertyId?: string
  linkTypeId?: string
}

export type MappedViolation = {
  violation: OntologyViolation
  target: ViolationTarget
}

const DOCUMENT_TARGET: ViolationTarget = { kind: "document" }

function asArrayIndex(segment: string | undefined): number | null {
  if (segment === undefined || !/^(0|[1-9][0-9]*)$/.test(segment)) return null
  return Number(segment)
}

export function mapViolationTarget(
  document: OntologyDocument,
  path: string
): ViolationTarget {
  const segments = parseJsonPointer(path)
  if (segments === null || segments.length === 0) return DOCUMENT_TARGET

  switch (segments[0]) {
    case "object_types": {
      const objectType = document.object_types[asArrayIndex(segments[1]) ?? -1]
      if (objectType === undefined) return DOCUMENT_TARGET
      if (segments[2] !== "properties")
        return { kind: "objectType", objectTypeId: objectType.id }
      const property = objectType.properties[asArrayIndex(segments[3]) ?? -1]
      if (property === undefined)
        return { kind: "objectType", objectTypeId: objectType.id }
      return {
        kind: "property",
        objectTypeId: objectType.id,
        propertyId: property.id,
      }
    }
    case "link_types": {
      const linkType = document.link_types[asArrayIndex(segments[1]) ?? -1]
      if (linkType === undefined) return DOCUMENT_TARGET
      return { kind: "linkType", linkTypeId: linkType.id }
    }
    case "canvas": {
      if (segments[1] !== "positions") return { kind: "canvas" }
      const index = asArrayIndex(segments[2])
      if (index === null) return { kind: "canvas" }
      const position = document.canvas.positions[index]
      if (position === undefined) return DOCUMENT_TARGET
      return { kind: "canvas", objectTypeId: position.object_type_id }
    }
    default:
      return DOCUMENT_TARGET
  }
}

export function mapViolations(
  document: OntologyDocument,
  violations: readonly OntologyViolation[]
): readonly MappedViolation[] {
  // 保持响应原序（服务端已按 path、code 排序），不在此重排
  return violations.map((violation) => ({
    violation,
    target: mapViolationTarget(document, violation.path),
  }))
}
