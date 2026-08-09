import type { OntologyDocument } from "@/features/ontology-editor/types"

// 编辑器内聚焦：由本地 candidate 计算邻域（与 docs/ontology/API.md 的
// neighborhood 语义一致——双向遍历 + induced link 子图），使未保存编辑可见，
// 不依赖远端端点。纯函数，可单测。

export const MAX_NEIGHBORHOOD_DEPTH = 5

export type LocalNeighborhood = {
  objectTypeIds: ReadonlySet<string>
  linkTypeIds: ReadonlySet<string>
}

/** BFS 双向遍历，返回 depth 跳内可达的 object type 集合与 induced link 子图。 */
export function computeLocalNeighborhood(
  document: Pick<OntologyDocument, "object_types" | "link_types">,
  originObjectTypeId: string,
  depth: number
): LocalNeighborhood | null {
  if (
    !document.object_types.some(
      (objectType) => objectType.id === originObjectTypeId
    )
  )
    return null

  const hops = Math.max(
    0,
    Math.min(MAX_NEIGHBORHOOD_DEPTH, Math.floor(depth))
  )

  const adjacency = new Map<string, Set<string>>()
  for (const linkType of document.link_types) {
    addEdge(adjacency, linkType.source_object_type_id, linkType.target_object_type_id)
    addEdge(adjacency, linkType.target_object_type_id, linkType.source_object_type_id)
  }

  const reached = new Set<string>([originObjectTypeId])
  let frontier: readonly string[] = [originObjectTypeId]
  for (let hop = 0; hop < hops; hop += 1) {
    const next: string[] = []
    for (const id of frontier) {
      for (const neighbor of adjacency.get(id) ?? []) {
        if (!reached.has(neighbor)) {
          reached.add(neighbor)
          next.push(neighbor)
        }
      }
    }
    frontier = next
  }

  const linkTypeIds = new Set<string>(
    document.link_types
      .filter(
        (linkType) =>
          reached.has(linkType.source_object_type_id) &&
          reached.has(linkType.target_object_type_id)
      )
      .map((linkType) => linkType.id)
  )

  return { objectTypeIds: reached, linkTypeIds }
}

function addEdge(
  adjacency: Map<string, Set<string>>,
  from: string,
  to: string
): void {
  const bucket = adjacency.get(from)
  if (bucket === undefined) {
    adjacency.set(from, new Set([to]))
  } else {
    bucket.add(to)
  }
}
