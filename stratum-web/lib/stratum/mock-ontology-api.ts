// 开发预览用的 Ontology API 内存 mock。仅在
// NEXT_PUBLIC_STRATUM_API_MOCK=ontology 且调用方未显式传入 api 时，
// 由 resolveOntologyApi 启用；未设置环境变量时行为与之前完全一致。
// 语义对齐 docs/ontology/API.md：强 ETag（"rev-N"，PUT 成功即递增）、
// 404 ontology_not_found / object_type_not_found、409 ontology_name_conflict、
// 412 ontology_precondition_failed、邻域按持久化文档做真实双向 BFS。
// 数据只存在于当前浏览器标签页内存，刷新即重置为种子数据。

import {
  ApiError,
  createStratumApi,
  STRATUM_API_BASE_URL,
  type OntologyResource,
  type StratumApi,
} from "@/lib/stratum/api"
import { createUuidV7 } from "@/features/ontology-editor/ids"
import type {
  OntologyDocument,
  OntologyListPage,
  OntologyNeighborhood,
  OntologyObjectType,
  OntologyProperty,
  OntologyPropertyValueType,
  OntologySummary,
} from "@/features/ontology-editor/types"

export type OntologyApi = Pick<
  StratumApi,
  | "listOntologies"
  | "createOntology"
  | "getOntology"
  | "replaceOntology"
  | "deleteOntology"
  | "getObjectTypeNeighborhood"
>

const DEFAULT_DELAY_MS = 300

type StoredOntology = {
  document: OntologyDocument
  revision: number
  createdAt: string
  updatedAt: string
}

const etagOf = (revision: number): string => `"rev-${revision}"`

const cloneDocument = (document: OntologyDocument): OntologyDocument =>
  structuredClone(document)

const sleep = (ms: number): Promise<void> =>
  new Promise((resolve) => setTimeout(resolve, ms))

export function createMockOntologyApi(options?: {
  delayMs?: number
}): OntologyApi {
  const delayMs = options?.delayMs ?? DEFAULT_DELAY_MS
  const store = new Map<string, StoredOntology>()
  for (const seed of seedOntologies()) store.set(seed.document.id, seed)

  const find = (ontologyId: string): StoredOntology => {
    const stored = store.get(ontologyId)
    if (stored === undefined)
      throw new ApiError(
        "ontology_not_found",
        404,
        "ontology was not found"
      )
    return stored
  }

  const assertNameAvailable = (name: string, exceptId?: string): void => {
    for (const stored of store.values()) {
      if (stored.document.name === name && stored.document.id !== exceptId)
        throw new ApiError(
          "ontology_name_conflict",
          409,
          "ontology name is already in use"
        )
    }
  }

  const assertEtag = (stored: StoredOntology, etag: string): void => {
    if (etag !== etagOf(stored.revision))
      throw new ApiError(
        "ontology_precondition_failed",
        412,
        "the supplied etag is no longer current"
      )
  }

  const toSummary = (stored: StoredOntology): OntologySummary => ({
    id: stored.document.id,
    name: stored.document.name,
    display_name: stored.document.display_name,
    ...(stored.document.description === undefined
      ? {}
      : { description: stored.document.description }),
    created_at: stored.createdAt,
    updated_at: stored.updatedAt,
  })

  return {
    listOntologies: async (query): Promise<OntologyListPage> => {
      await sleep(delayMs)
      const page = query?.page ?? 1
      const perPage = query?.perPage ?? 20
      const sort = query?.sort ?? "-updated_at"
      const descending = sort.startsWith("-")
      const field = (
        descending ? sort.slice(1) : sort
      ) as keyof Pick<
        OntologySummary,
        "name" | "display_name" | "created_at" | "updated_at"
      >

      const summaries = Array.from(store.values(), toSummary).sort((a, b) => {
        const order = a[field].localeCompare(b[field])
        if (order !== 0) return descending ? -order : order
        return a.id.localeCompare(b.id)
      })

      const start = (page - 1) * perPage
      return {
        data: summaries.slice(start, start + perPage),
        pagination: { page, per_page: perPage, total: summaries.length },
      }
    },

    createOntology: async (input): Promise<OntologyResource> => {
      await sleep(delayMs)
      assertNameAvailable(input.name)
      const id = createUuidV7()
      const now = new Date().toISOString()
      const document: OntologyDocument = {
        id,
        name: input.name,
        display_name: input.displayName,
        ...(input.description === undefined
          ? {}
          : { description: input.description }),
        object_types: [],
        link_types: [],
        canvas: { positions: [] },
      }
      store.set(id, { document, revision: 1, createdAt: now, updatedAt: now })
      return {
        document: cloneDocument(document),
        etag: etagOf(1),
        location: `/v1/ontologies/${id}`,
      }
    },

    getOntology: async (ontologyId): Promise<OntologyResource> => {
      await sleep(delayMs)
      const stored = find(ontologyId)
      return {
        document: cloneDocument(stored.document),
        etag: etagOf(stored.revision),
        location: null,
      }
    },

    replaceOntology: async (ontologyId, document, etag) => {
      await sleep(delayMs)
      const stored = find(ontologyId)
      assertEtag(stored, etag)
      if (document.id !== ontologyId)
        throw new ApiError(
          "invalid_request",
          400,
          "path and body ontology ids must match"
        )
      assertNameAvailable(document.name, ontologyId)
      store.set(ontologyId, {
        document: cloneDocument(document),
        revision: stored.revision + 1,
        createdAt: stored.createdAt,
        updatedAt: new Date().toISOString(),
      })
      return { etag: etagOf(stored.revision + 1) }
    },

    deleteOntology: async (ontologyId, etag) => {
      await sleep(delayMs)
      const stored = find(ontologyId)
      assertEtag(stored, etag)
      store.delete(ontologyId)
    },

    getObjectTypeNeighborhood: async (
      ontologyId,
      objectTypeId,
      depth
    ): Promise<OntologyNeighborhood> => {
      await sleep(delayMs)
      const stored = find(ontologyId)
      const { document } = stored
      const origin = document.object_types.find(
        (objectType) => objectType.id === objectTypeId
      )
      if (origin === undefined)
        throw new ApiError(
          "object_type_not_found",
          404,
          "object type was not found"
        )

      const maxDepth = Math.max(0, Math.min(5, depth ?? 1))
      // 沿 Link Type 双向 BFS，记录每个 Object Type 的最短跳数
      const distances = new Map<string, number>([[objectTypeId, 0]])
      const queue = [objectTypeId]
      for (let head = 0; head < queue.length; head += 1) {
        const current = queue[head] as string
        const currentDepth = distances.get(current) as number
        if (currentDepth >= maxDepth) continue
        for (const link of document.link_types) {
          const neighbor =
            link.source_object_type_id === current
              ? link.target_object_type_id
              : link.target_object_type_id === current
                ? link.source_object_type_id
                : null
          if (neighbor === null || distances.has(neighbor)) continue
          distances.set(neighbor, currentDepth + 1)
          queue.push(neighbor)
        }
      }

      // 诱导子图：两端都在结果集内的 Link Type 才返回；数组保持文档顺序
      const objectTypes = document.object_types.filter((objectType) =>
        distances.has(objectType.id)
      )
      const linkTypes = document.link_types.filter(
        (link) =>
          distances.has(link.source_object_type_id) &&
          distances.has(link.target_object_type_id)
      )
      return {
        origin_object_type_id: objectTypeId,
        depth: maxDepth,
        object_types: structuredClone(objectTypes),
        link_types: structuredClone(linkTypes),
        canvas: {
          positions: document.canvas.positions.filter((position) =>
            distances.has(position.object_type_id)
          ),
        },
      }
    },
  }
}

// 两个 hook 共用的解析入口：显式传入的 api 永远优先；
// 环境变量开启时返回模块级单例 mock，保证列表页与编辑器共享同一份内存数据。
let sharedMockApi: StratumApi | null = null

export function resolveOntologyApi(
  apiOption: StratumApi | undefined
): StratumApi {
  if (apiOption !== undefined) return apiOption
  if (process.env.NEXT_PUBLIC_STRATUM_API_MOCK !== "ontology")
    return createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
  sharedMockApi ??= withUnsupportedMethods(createMockOntologyApi())
  return sharedMockApi
}

// mock 只覆盖 Ontology 方法；其余 StratumApi 方法以显式 501 兜底，保持类型完整。
function withUnsupportedMethods(ontology: OntologyApi): StratumApi {
  const unsupported = async (): Promise<never> => {
    throw new ApiError(
      "mock_not_supported",
      501,
      "the ontology mock only implements ontology endpoints"
    )
  }
  return {
    ...ontology,
    createAgent: unsupported,
    getAgentTemplates: unsupported,
    getModels: unsupported,
    getAgent: unsupported,
    getHistory: unsupported,
    sendMessage: unsupported,
    resume: unsupported,
    cancel: unsupported,
    resolveApproval: unsupported,
  }
}

// ---- 种子数据 ----

function property(
  name: string,
  displayName: string,
  valueType: OntologyPropertyValueType,
  required: boolean,
  description?: string
): OntologyProperty {
  return {
    id: createUuidV7(),
    name,
    display_name: displayName,
    ...(description === undefined ? {} : { description }),
    value_type: valueType,
    required,
  }
}

function seedOntologies(): StoredOntology[] {
  const customerId = createUuidV7()
  const orderId = createUuidV7()
  const productId = createUuidV7()

  // 「客户洞察」：客户/订单/产品三类实体；产品节点故意不摆位置，触发网格自动布局
  const customer: OntologyObjectType = {
    id: customerId,
    name: "customer",
    display_name: "客户",
    description: "已注册的客户账户",
    properties: [
      property("name", "姓名", "string", true),
      property("phone", "手机号", "string", false),
      property("email", "邮箱", "string", false, "主要联系邮箱"),
      property("registered_at", "注册日期", "date", true),
      property("member_level", "会员等级", "integer", false),
      property("is_active", "是否活跃", "boolean", true),
    ],
  }
  const order: OntologyObjectType = {
    id: orderId,
    name: "order",
    display_name: "订单",
    description: "客户提交的一笔交易订单",
    properties: [
      property("order_no", "订单编号", "string", true),
      property("total_amount", "订单金额", "number", true),
      property("placed_at", "下单时间", "date_time", true),
      property("status", "订单状态", "string", false),
    ],
  }
  const product: OntologyObjectType = {
    id: productId,
    name: "product",
    display_name: "产品",
    properties: [
      property("title", "产品名称", "string", true),
      property("category", "类目", "string", false),
      property("price", "单价", "number", true),
      property("listed_at", "上架日期", "date", false),
    ],
  }

  const customerInsight: OntologyDocument = {
    id: createUuidV7(),
    name: "customer_insight",
    display_name: "客户洞察",
    description: "面向客户运营分析的核心领域模型",
    object_types: [customer, order, product],
    link_types: [
      {
        id: createUuidV7(),
        name: "places_order",
        display_name: "下单",
        description: "客户提交订单",
        source_object_type_id: customerId,
        target_object_type_id: orderId,
        source_to_target: "many",
        target_to_source: "one",
      },
      {
        id: createUuidV7(),
        name: "contains_product",
        display_name: "包含产品",
        description: "订单包含的产品明细",
        source_object_type_id: orderId,
        target_object_type_id: productId,
        source_to_target: "many",
        target_to_source: "many",
      },
    ],
    canvas: {
      positions: [
        { object_type_id: customerId, x: -320, y: -40 },
        { object_type_id: orderId, x: 40, y: -120 },
      ],
    },
  }

  // 「内容运营」：全部就位的小型双节点图
  const creatorId = createUuidV7()
  const contentId = createUuidV7()
  const contentOps: OntologyDocument = {
    id: createUuidV7(),
    name: "content_ops",
    display_name: "内容运营",
    description: "创作者与内容的发布关系",
    object_types: [
      {
        id: creatorId,
        name: "creator",
        display_name: "创作者",
        properties: [
          property("nickname", "昵称", "string", true),
          property("followers", "粉丝数", "integer", false),
        ],
      },
      {
        id: contentId,
        name: "content",
        display_name: "内容",
        properties: [
          property("title", "标题", "string", true),
          property("published_at", "发布时间", "date_time", false),
        ],
      },
    ],
    link_types: [
      {
        id: createUuidV7(),
        name: "publishes",
        display_name: "发布",
        source_object_type_id: creatorId,
        target_object_type_id: contentId,
        source_to_target: "many",
        target_to_source: "one",
      },
    ],
    canvas: {
      positions: [
        { object_type_id: creatorId, x: -240, y: 0 },
        { object_type_id: contentId, x: 160, y: 0 },
      ],
    },
  }

  // 「空白本体」：空文档，展示空画布行为
  const blankCanvas: OntologyDocument = {
    id: createUuidV7(),
    name: "blank_canvas",
    display_name: "空白本体",
    description: "尚未建模的空白本体",
    object_types: [],
    link_types: [],
    canvas: { positions: [] },
  }

  return [
    {
      document: customerInsight,
      revision: 3,
      createdAt: "2026-07-20T02:30:00Z",
      updatedAt: "2026-08-07T09:15:00Z",
    },
    {
      document: contentOps,
      revision: 1,
      createdAt: "2026-08-01T06:00:00Z",
      updatedAt: "2026-08-05T11:40:00Z",
    },
    {
      document: blankCanvas,
      revision: 1,
      createdAt: "2026-08-08T08:30:00Z",
      updatedAt: "2026-08-08T08:30:00Z",
    },
  ]
}
