import type { OntologyDocument } from "@/features/ontology-editor/types"

// 无位置节点的确定性网格布局（设计决策 D7）：同一文档渲染结果确定、不抖动。
// 稳定顺序 = 文档数组序：未存位置的节点按其在 object_types 中的数组下标落入
// 固定网格；已存 canvas.positions 的节点优先使用存储坐标。
// 选用「数组下标」而非「未定位节点中的序号」：拖拽某节点落位后，其余未定位
// 节点的网格坐标不受影响，避免级联跳动。

export type CanvasPoint = { x: number; y: number }

export type GridLayoutOptions = {
  columns?: number
  columnWidth?: number
  rowHeight?: number
  originX?: number
  originY?: number
}

export const DEFAULT_GRID_LAYOUT = {
  columns: 3,
  columnWidth: 320,
  rowHeight: 220,
  originX: 0,
  originY: 0,
} as const satisfies Required<GridLayoutOptions>

export function gridPositionForIndex(
  index: number,
  options?: GridLayoutOptions
): CanvasPoint {
  const { columns, columnWidth, rowHeight, originX, originY } = {
    ...DEFAULT_GRID_LAYOUT,
    ...options,
  }
  const column = index % columns
  const row = Math.floor(index / columns)
  return {
    x: originX + column * columnWidth,
    y: originY + row * rowHeight,
  }
}

/** 解析整份文档每个 object type 的画布坐标：存储位置优先，缺失走网格。 */
export function resolveNodePositions(
  document: Pick<OntologyDocument, "object_types" | "canvas">,
  options?: GridLayoutOptions
): ReadonlyMap<string, CanvasPoint> {
  const stored = new Map<string, CanvasPoint>(
    document.canvas.positions.map((position) => [
      position.object_type_id,
      { x: position.x, y: position.y },
    ])
  )
  const resolved = new Map<string, CanvasPoint>()
  document.object_types.forEach((objectType, index) => {
    resolved.set(
      objectType.id,
      stored.get(objectType.id) ?? gridPositionForIndex(index, options)
    )
  })
  return resolved
}
