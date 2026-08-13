import { describe, expect, it } from "vitest"

import type { OntologyDocument } from "@/features/ontology-editor/types"
import {
  mapViolations,
  mapViolationTarget,
} from "@/features/ontology-editor/violations"

const document: OntologyDocument = {
  id: "onto-1",
  name: "support_domain",
  display_name: "Support domain",
  object_types: [
    {
      id: "ot-1",
      name: "customer",
      display_name: "Customer",
      properties: [
        {
          id: "p-1",
          name: "email",
          display_name: "Email",
          value_type: "string",
          required: true,
        },
        {
          id: "p-2",
          name: "age",
          display_name: "Age",
          value_type: "integer",
          required: false,
        },
      ],
    },
    { id: "ot-2", name: "ticket", display_name: "Ticket", properties: [] },
  ],
  link_types: [
    {
      id: "lt-1",
      name: "owns_ticket",
      display_name: "Owns ticket",
      source_object_type_id: "ot-1",
      target_object_type_id: "ot-2",
      source_to_target: "many",
      target_to_source: "one",
    },
  ],
  canvas: { positions: [{ object_type_id: "ot-1", x: 120.5, y: -48 }] },
}

describe("mapViolationTarget", () => {
  it("maps object type field paths to the object type", () => {
    expect(mapViolationTarget(document, "/object_types/1/name")).toEqual({
      kind: "objectType",
      objectTypeId: "ot-2",
    })
    expect(mapViolationTarget(document, "/object_types/0")).toEqual({
      kind: "objectType",
      objectTypeId: "ot-1",
    })
  })

  it("maps property paths to the owning object type and property", () => {
    expect(
      mapViolationTarget(document, "/object_types/0/properties/1/name")
    ).toEqual({ kind: "property", objectTypeId: "ot-1", propertyId: "p-2" })
  })

  it("falls back to the object type when the property index is out of range", () => {
    expect(
      mapViolationTarget(document, "/object_types/0/properties/9/name")
    ).toEqual({ kind: "objectType", objectTypeId: "ot-1" })
  })

  it("maps link type paths to the link type", () => {
    expect(
      mapViolationTarget(document, "/link_types/0/source_object_type_id")
    ).toEqual({ kind: "linkType", linkTypeId: "lt-1" })
  })

  it("maps canvas position paths to the positioned object type", () => {
    expect(mapViolationTarget(document, "/canvas/positions/0/x")).toEqual({
      kind: "canvas",
      objectTypeId: "ot-1",
    })
  })

  it("maps the document root and document-level paths to document", () => {
    expect(mapViolationTarget(document, "")).toEqual({ kind: "document" })
    expect(mapViolationTarget(document, "/name")).toEqual({ kind: "document" })
    expect(mapViolationTarget(document, "/link_types")).toEqual({
      kind: "document",
    })
  })

  it("maps out-of-range indexes to document", () => {
    expect(mapViolationTarget(document, "/object_types/7/name")).toEqual({
      kind: "document",
    })
    expect(mapViolationTarget(document, "/link_types/3/name")).toEqual({
      kind: "document",
    })
    expect(mapViolationTarget(document, "/canvas/positions/4/y")).toEqual({
      kind: "document",
    })
  })

  it("maps non-numeric array segments to document", () => {
    expect(mapViolationTarget(document, "/object_types/first/name")).toEqual({
      kind: "document",
    })
    expect(mapViolationTarget(document, "/object_types/-/name")).toEqual({
      kind: "document",
    })
  })

  it("handles escaped segments that decode to unknown fields", () => {
    // ~1 解码为 /、~0 解码为 ~；解码后仍不是已知集合名 → document
    expect(mapViolationTarget(document, "/object~1types/0/name")).toEqual({
      kind: "document",
    })
    expect(mapViolationTarget(document, "/~0/name")).toEqual({
      kind: "document",
    })
  })

  it("maps invalid pointers to document", () => {
    expect(mapViolationTarget(document, "object_types/0/name")).toEqual({
      kind: "document",
    })
    expect(mapViolationTarget(document, "/object_types/0~2/name")).toEqual({
      kind: "document",
    })
  })
})

describe("mapViolations", () => {
  it("maps every violation and preserves response order", () => {
    const mapped = mapViolations(document, [
      { code: "a", path: "/object_types/0/name", message: "m1" },
      { code: "b", path: "/canvas/positions/0/x", message: "m2" },
      { code: "c", path: "/link_types", message: "m3" },
    ])

    expect(mapped.map((entry) => entry.violation.code)).toEqual([
      "a",
      "b",
      "c",
    ])
    expect(mapped.map((entry) => entry.target.kind)).toEqual([
      "objectType",
      "canvas",
      "document",
    ])
  })
})
