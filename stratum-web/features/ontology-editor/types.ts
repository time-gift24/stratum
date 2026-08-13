// Ontology 资源文档 DTO，与 docs/ontology/API.md 逐字段对齐（snake_case 为线协议形状）。
// Ontology / ObjectType / Property / LinkType 的 id 均为 UUIDv7 字符串。

export type OntologyPropertyValueType =
  | "string"
  | "integer"
  | "number"
  | "boolean"
  | "date"
  | "date_time"

export type OntologyLinkCardinality = "one" | "many"

export type OntologyProperty = {
  id: string
  name: string
  display_name: string
  description?: string
  value_type: OntologyPropertyValueType
  required: boolean
}

export type OntologyObjectType = {
  id: string
  name: string
  display_name: string
  description?: string
  properties: readonly OntologyProperty[]
}

export type OntologyLinkType = {
  id: string
  name: string
  display_name: string
  description?: string
  source_object_type_id: string
  target_object_type_id: string
  source_to_target: OntologyLinkCardinality
  target_to_source: OntologyLinkCardinality
}

export type OntologyCanvasPosition = {
  object_type_id: string
  x: number
  y: number
}

export type OntologyCanvas = {
  positions: readonly OntologyCanvasPosition[]
}

export type OntologyDocument = {
  id: string
  name: string
  display_name: string
  description?: string
  object_types: readonly OntologyObjectType[]
  link_types: readonly OntologyLinkType[]
  canvas: OntologyCanvas
}

export type OntologySummary = {
  id: string
  name: string
  display_name: string
  description?: string
  created_at: string
  updated_at: string
}

export type OntologyPagination = {
  page: number
  per_page: number
  total: number
}

export type OntologyListPage = {
  data: readonly OntologySummary[]
  pagination: OntologyPagination
}

export type OntologyNeighborhood = {
  origin_object_type_id: string
  depth: number
  object_types: readonly OntologyObjectType[]
  link_types: readonly OntologyLinkType[]
  canvas: OntologyCanvas
}

// 422 invalid_ontology_schema 的单条违例；path 为 RFC 6901 JSON Pointer。
export type OntologyViolation = {
  code: string
  path: string
  message: string
}
