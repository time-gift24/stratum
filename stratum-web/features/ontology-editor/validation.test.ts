import { describe, expect, it } from "vitest"

import type {
  OntologyDocument,
  OntologyObjectType,
} from "@/features/ontology-editor/types"
import {
  isValidOntologyName,
  ONTOLOGY_MVP_LIMITS,
  validateOntologyDocument,
  validateOntologyLimits,
  validateOntologyNames,
} from "@/features/ontology-editor/validation"

function makeDocument(
  overrides: Partial<OntologyDocument> = {}
): OntologyDocument {
  return {
    id: "onto-1",
    name: "support_domain",
    display_name: "Support domain",
    object_types: [],
    link_types: [],
    canvas: { positions: [] },
    ...overrides,
  }
}

function makeObjectTypes(
  count: number,
  propertiesPerType = 0
): OntologyObjectType[] {
  return Array.from({ length: count }, (_, index) => ({
    id: `ot-${index}`,
    name: `ot_${index}`,
    display_name: `OT ${index}`,
    properties: Array.from({ length: propertiesPerType }, (_, p) => ({
      id: `ot-${index}-p-${p}`,
      name: `p_${p}`,
      display_name: `P ${p}`,
      value_type: "string" as const,
      required: false,
    })),
  }))
}

describe("isValidOntologyName", () => {
  it("accepts contract-valid names", () => {
    expect(isValidOntologyName("a")).toBe(true)
    expect(isValidOntologyName("support_domain")).toBe(true)
    expect(isValidOntologyName("a".padEnd(64, "0"))).toBe(true)
  })

  it("rejects invalid names", () => {
    expect(isValidOntologyName("")).toBe(false)
    expect(isValidOntologyName("1abc")).toBe(false)
    expect(isValidOntologyName("Abc")).toBe(false)
    expect(isValidOntologyName("a-b")).toBe(false)
    expect(isValidOntologyName("a b")).toBe(false)
    expect(isValidOntologyName("a".padEnd(65, "0"))).toBe(false)
  })
})

describe("validateOntologyNames", () => {
  it("flags invalid names with entity-level paths", () => {
    const document = makeDocument({
      name: "Bad",
      object_types: [
        {
          id: "ot-1",
          name: "1bad",
          display_name: "Bad",
          properties: [
            {
              id: "p-1",
              name: "no-space ok?",
              display_name: "P",
              value_type: "string",
              required: false,
            },
          ],
        },
      ],
      link_types: [
        {
          id: "lt-1",
          name: "ok_name",
          display_name: "OK",
          source_object_type_id: "ot-1",
          target_object_type_id: "ot-1",
          source_to_target: "one",
          target_to_source: "one",
        },
      ],
    })

    const violations = validateOntologyNames(document)
    expect(violations.map((v) => v.path)).toEqual([
      "/name",
      "/object_types/0/name",
      "/object_types/0/properties/0/name",
    ])
    expect(violations.every((v) => v.code === "invalid_name")).toBe(true)
  })

  it("accepts a valid document", () => {
    expect(validateOntologyNames(makeDocument())).toEqual([])
  })
})

describe("validateOntologyLimits", () => {
  it("accepts an empty document", () => {
    expect(validateOntologyLimits(makeDocument())).toEqual([])
  })

  it("accepts documents exactly at each limit", () => {
    const document = makeDocument({
      object_types: makeObjectTypes(ONTOLOGY_MVP_LIMITS.maxObjectTypes),
    })
    expect(validateOntologyLimits(document)).toEqual([])
  })

  it("flags more than 500 object types", () => {
    const document = makeDocument({
      object_types: makeObjectTypes(ONTOLOGY_MVP_LIMITS.maxObjectTypes + 1),
    })
    expect(validateOntologyLimits(document)).toEqual([
      expect.objectContaining({
        code: "too_many_object_types",
        path: "/object_types",
      }),
    ])
  })

  it("flags more than 100 properties on one object type", () => {
    const document = makeDocument({
      object_types: makeObjectTypes(
        1,
        ONTOLOGY_MVP_LIMITS.maxPropertiesPerObjectType + 1
      ),
    })
    expect(validateOntologyLimits(document)).toEqual([
      expect.objectContaining({
        code: "too_many_properties",
        path: "/object_types/0/properties",
      }),
    ])
  })

  it("flags more than 10000 total properties across object types", () => {
    // 101 types × 100 properties = 10100 > 10000，单 type 均未超限
    const document = makeDocument({
      object_types: makeObjectTypes(
        101,
        ONTOLOGY_MVP_LIMITS.maxPropertiesPerObjectType
      ),
    })
    expect(validateOntologyLimits(document)).toEqual([
      expect.objectContaining({
        code: "too_many_properties_total",
        path: "/object_types",
      }),
    ])
  })

  it("flags more than 2000 link types", () => {
    const document = makeDocument({
      object_types: makeObjectTypes(1),
      link_types: Array.from(
        { length: ONTOLOGY_MVP_LIMITS.maxLinkTypes + 1 },
        (_, index) => ({
          id: `lt-${index}`,
          name: `lt_${index}`,
          display_name: `LT ${index}`,
          source_object_type_id: "ot-0",
          target_object_type_id: "ot-0",
          source_to_target: "one" as const,
          target_to_source: "one" as const,
        })
      ),
    })
    expect(validateOntologyLimits(document)).toEqual([
      expect.objectContaining({
        code: "too_many_link_types",
        path: "/link_types",
      }),
    ])
  })

  it("flags more than 500 canvas positions", () => {
    const document = makeDocument({
      object_types: makeObjectTypes(1),
      canvas: {
        positions: Array.from(
          { length: ONTOLOGY_MVP_LIMITS.maxCanvasPositions + 1 },
          (_, index) => ({ object_type_id: "ot-0", x: index, y: index })
        ),
      },
    })
    expect(validateOntologyLimits(document)).toEqual([
      expect.objectContaining({
        code: "too_many_canvas_positions",
        path: "/canvas/positions",
      }),
    ])
  })
})

describe("validateOntologyDocument", () => {
  it("merges name and limit violations sorted by path then code", () => {
    const document = makeDocument({
      name: "Bad",
      object_types: makeObjectTypes(ONTOLOGY_MVP_LIMITS.maxObjectTypes + 1),
    })
    const violations = validateOntologyDocument(document)
    expect(violations.map((v) => `${v.path}#${v.code}`)).toEqual([
      "/name#invalid_name",
      "/object_types#too_many_object_types",
    ])
  })
})
