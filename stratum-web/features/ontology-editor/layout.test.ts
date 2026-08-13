import { describe, expect, it } from "vitest"

import {
  DEFAULT_GRID_LAYOUT,
  gridPositionForIndex,
  resolveNodePositions,
} from "@/features/ontology-editor/layout"
import type {
  OntologyDocument,
  OntologyObjectType,
} from "@/features/ontology-editor/types"

function objectType(id: string, name: string): OntologyObjectType {
  return { id, name, display_name: name, properties: [] }
}

function document(
  objectTypes: readonly OntologyObjectType[],
  positions: readonly { object_type_id: string; x: number; y: number }[]
): OntologyDocument {
  return {
    id: "onto",
    name: "onto",
    display_name: "Onto",
    object_types: objectTypes,
    link_types: [],
    canvas: { positions },
  }
}

describe("gridPositionForIndex", () => {
  it("按列优先的网格坐标排列", () => {
    const { columnWidth, rowHeight } = DEFAULT_GRID_LAYOUT
    expect(gridPositionForIndex(0)).toEqual({ x: 0, y: 0 })
    expect(gridPositionForIndex(1)).toEqual({ x: columnWidth, y: 0 })
    expect(gridPositionForIndex(2)).toEqual({ x: columnWidth * 2, y: 0 })
    expect(gridPositionForIndex(3)).toEqual({ x: 0, y: rowHeight })
    expect(gridPositionForIndex(4)).toEqual({ x: columnWidth, y: rowHeight })
  })

  it("支持自定义网格参数与原点", () => {
    expect(
      gridPositionForIndex(3, {
        columns: 2,
        columnWidth: 100,
        rowHeight: 50,
        originX: 10,
        originY: 20,
      })
    ).toEqual({ x: 110, y: 70 })
  })
})

describe("resolveNodePositions", () => {
  it("已存 canvas.positions 的节点使用存储坐标", () => {
    const doc = document(
      [objectType("a", "a"), objectType("b", "b")],
      [{ object_type_id: "b", x: 120.5, y: -48 }]
    )
    const positions = resolveNodePositions(doc)
    expect(positions.get("b")).toEqual({ x: 120.5, y: -48 })
  })

  it("未定位节点按文档数组序落网格，同一文档结果确定", () => {
    const doc = document(
      [objectType("a", "a"), objectType("b", "b"), objectType("c", "c")],
      []
    )
    const first = resolveNodePositions(doc)
    const second = resolveNodePositions(doc)
    expect(first.get("a")).toEqual(gridPositionForIndex(0))
    expect(first.get("b")).toEqual(gridPositionForIndex(1))
    expect(first.get("c")).toEqual(gridPositionForIndex(2))
    expect([...first.entries()]).toEqual([...second.entries()])
  })

  it("某节点落位后其余未定位节点坐标不变", () => {
    const before = resolveNodePositions(
      document(
        [objectType("a", "a"), objectType("b", "b"), objectType("c", "c")],
        []
      )
    )
    const after = resolveNodePositions(
      document(
        [objectType("a", "a"), objectType("b", "b"), objectType("c", "c")],
        [{ object_type_id: "a", x: 999, y: 999 }]
      )
    )
    expect(after.get("b")).toEqual(before.get("b"))
    expect(after.get("c")).toEqual(before.get("c"))
  })

  it("忽略指向不存在 object type 的存储位置", () => {
    const doc = document(
      [objectType("a", "a")],
      [{ object_type_id: "ghost", x: 5, y: 5 }]
    )
    const positions = resolveNodePositions(doc)
    expect(positions.size).toBe(1)
    expect(positions.get("a")).toEqual(gridPositionForIndex(0))
  })
})
