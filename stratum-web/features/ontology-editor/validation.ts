import type {
  OntologyDocument,
  OntologyViolation,
} from "@/features/ontology-editor/types"

// 契约约定（docs/ontology/API.md Conventions / MVP limits）的客户端先行校验。
export const ONTOLOGY_NAME_PATTERN = /^[a-z][a-z0-9_]{0,63}$/

export const ONTOLOGY_MVP_LIMITS = {
  maxObjectTypes: 500,
  maxPropertiesPerObjectType: 100,
  maxTotalProperties: 10_000,
  maxLinkTypes: 2_000,
  maxCanvasPositions: 500,
} as const

export function isValidOntologyName(name: string): boolean {
  return ONTOLOGY_NAME_PATTERN.test(name)
}

function violation(
  code: string,
  path: string,
  message: string
): OntologyViolation {
  return { code, path, message }
}

// name 形状校验：ontology 自身、object type、property、link type。
export function validateOntologyNames(
  document: OntologyDocument
): OntologyViolation[] {
  const violations: OntologyViolation[] = []

  if (!isValidOntologyName(document.name))
    violations.push(
      violation("invalid_name", "/name", "name must match ^[a-z][a-z0-9_]{0,63}$")
    )

  document.object_types.forEach((objectType, objectTypeIndex) => {
    if (!isValidOntologyName(objectType.name))
      violations.push(
        violation(
          "invalid_name",
          `/object_types/${objectTypeIndex}/name`,
          "object type name must match ^[a-z][a-z0-9_]{0,63}$"
        )
      )
    objectType.properties.forEach((property, propertyIndex) => {
      if (!isValidOntologyName(property.name))
        violations.push(
          violation(
            "invalid_name",
            `/object_types/${objectTypeIndex}/properties/${propertyIndex}/name`,
            "property name must match ^[a-z][a-z0-9_]{0,63}$"
          )
        )
    })
  })

  document.link_types.forEach((linkType, linkTypeIndex) => {
    if (!isValidOntologyName(linkType.name))
      violations.push(
        violation(
          "invalid_name",
          `/link_types/${linkTypeIndex}/name`,
          "link type name must match ^[a-z][a-z0-9_]{0,63}$"
        )
      )
  })

  return violations
}

// MVP 上限：超限编辑在客户端阻止，避免必然被 422 拒绝的 candidate。
export function validateOntologyLimits(
  document: OntologyDocument
): OntologyViolation[] {
  const violations: OntologyViolation[] = []
  const limits = ONTOLOGY_MVP_LIMITS

  if (document.object_types.length > limits.maxObjectTypes)
    violations.push(
      violation(
        "too_many_object_types",
        "/object_types",
        `object types must not exceed ${limits.maxObjectTypes}`
      )
    )

  let totalProperties = 0
  document.object_types.forEach((objectType, objectTypeIndex) => {
    totalProperties += objectType.properties.length
    if (objectType.properties.length > limits.maxPropertiesPerObjectType)
      violations.push(
        violation(
          "too_many_properties",
          `/object_types/${objectTypeIndex}/properties`,
          `properties per object type must not exceed ${limits.maxPropertiesPerObjectType}`
        )
      )
  })
  if (totalProperties > limits.maxTotalProperties)
    violations.push(
      violation(
        "too_many_properties_total",
        "/object_types",
        `total properties must not exceed ${limits.maxTotalProperties}`
      )
    )

  if (document.link_types.length > limits.maxLinkTypes)
    violations.push(
      violation(
        "too_many_link_types",
        "/link_types",
        `link types must not exceed ${limits.maxLinkTypes}`
      )
    )

  if (document.canvas.positions.length > limits.maxCanvasPositions)
    violations.push(
      violation(
        "too_many_canvas_positions",
        "/canvas/positions",
        `canvas positions must not exceed ${limits.maxCanvasPositions}`
      )
    )

  return violations
}

// 合并校验并按契约顺序（path、code）排序返回。
export function validateOntologyDocument(
  document: OntologyDocument
): OntologyViolation[] {
  return [
    ...validateOntologyNames(document),
    ...validateOntologyLimits(document),
  ].sort(
    (left, right) =>
      left.path.localeCompare(right.path) || left.code.localeCompare(right.code)
  )
}
