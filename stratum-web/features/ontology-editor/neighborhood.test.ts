import { describe, expect, it } from "vitest"

import {
  computeLocalNeighborhood,
  MAX_NEIGHBORHOOD_DEPTH,
} from "@/features/ontology-editor/neighborhood"
import type {
  OntologyDocument,
  OntologyLinkType,
  OntologyObjectType,
} from "@/features/ontology-editor/types"

function objectType(id: string): OntologyObjectType {
  return { id, name: id, display_name: id, properties: [] }
}

function linkType(
  id: string,
  source: string,
  target: string
): OntologyLinkType {
  return {
    id,
    name: id,
    display_name: id,
    source_object_type_id: source,
    target_object_type_id: target,
    source_to_target: "many",
    target_to_source: "one",
  }
}

// 图：a -l1-> b -l2-> c -l3-> d，另有自环 a -self-> a
function chain(): OntologyDocument {
  return {
    id: "onto",
    name: "onto",
    display_name: "Onto",
    object_types: ["a", "b", "c", "d"].map(objectType),
    link_types: [
      linkType("l1", "a", "b"),
      linkType("l2", "b", "c"),
      linkType("l3", "c", "d"),
      linkType("self", "a", "a"),
    ],
    canvas: { positions: [] },
  }
}

describe("computeLocalNeighborhood", () => {
  it("origin 不存在时返回 null", () => {
    expect(computeLocalNeighborhood(chain(), "ghost", 1)).toBeNull()
  })

  it("depth 0 只含 origin 与其自环", () => {
    const result = computeLocalNeighborhood(chain(), "a", 0)
    expect(result).not.toBeNull()
    expect([...(result?.objectTypeIds ?? [])].sort()).toEqual(["a"])
    expect([...(result?.linkTypeIds ?? [])]).toEqual(["self"])
  })

  it("depth 1 双向可达一跳邻居", () => {
    const fromB = computeLocalNeighborhood(chain(), "b", 1)
    expect([...(fromB?.objectTypeIds ?? [])].sort()).toEqual(["a", "b", "c"])
    expect([...(fromB?.linkTypeIds ?? [])].sort()).toEqual(["l1", "l2", "self"])
  })

  it("depth 2 覆盖两跳，induced 子图只含两端都在集合内的 link", () => {
    const fromA = computeLocalNeighborhood(chain(), "a", 2)
    expect([...(fromA?.objectTypeIds ?? [])].sort()).toEqual(["a", "b", "c"])
    // l3 的终点 d 不在集合内，不属于 induced 子图
    expect([...(fromA?.linkTypeIds ?? [])].sort()).toEqual(["l1", "l2", "self"])
  })

  it("depth 超过上限时按上限截断", () => {
    const result = computeLocalNeighborhood(chain(), "a", 99)
    expect([...(result?.objectTypeIds ?? [])].sort()).toEqual([
      "a",
      "b",
      "c",
      "d",
    ])
    expect(MAX_NEIGHBORHOOD_DEPTH).toBe(5)
  })

  it("负数 depth 按 0 处理", () => {
    const result = computeLocalNeighborhood(chain(), "a", -2)
    expect([...(result?.objectTypeIds ?? [])]).toEqual(["a"])
  })

  it("未保存的新节点与连线参与计算（本地 candidate 语义）", () => {
    const doc = chain()
    const extended: OntologyDocument = {
      ...doc,
      object_types: [...doc.object_types, objectType("e")],
      link_types: [...doc.link_types, linkType("l4", "a", "e")],
    }
    const result = computeLocalNeighborhood(extended, "a", 1)
    expect(result?.objectTypeIds.has("e")).toBe(true)
    expect(result?.linkTypeIds.has("l4")).toBe(true)
  })
})
