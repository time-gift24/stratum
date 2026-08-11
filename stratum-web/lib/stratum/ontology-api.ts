// 两个 hook 共用的 api 解析入口：显式传入的 api 永远优先；
// NEXT_PUBLIC_STRATUM_API_MOCK=ontology 时返回模块级单例 mock，保证列表页与
// 编辑器共享同一份内存数据。mock 经动态 import 懒加载：非 mock 构建里
// mock-ontology-api 模块（含种子数据）不会进入运行时加载路径。
import {
  ApiError,
  createStratumApi,
  STRATUM_API_BASE_URL,
  type StratumApi,
} from "@/lib/stratum/api"
import type { OntologyApi } from "@/lib/stratum/mock-ontology-api"

let sharedMockApi: StratumApi | null = null

export function resolveOntologyApi(
  apiOption: StratumApi | undefined
): StratumApi {
  if (apiOption !== undefined) return apiOption
  if (process.env.NEXT_PUBLIC_STRATUM_API_MOCK !== "ontology")
    return createStratumApi({ baseUrl: STRATUM_API_BASE_URL })
  sharedMockApi ??= createLazyMockOntologyApi()
  return sharedMockApi
}

// mock 模块在首个方法调用前即开始加载；各方法原地等待加载完成后委托。
// 只在 mock 路径上存在这一层间接，真实模式行为与直接 createStratumApi 一致。
function createLazyMockOntologyApi(): StratumApi {
  const mock = import("@/lib/stratum/mock-ontology-api").then((module) =>
    module.createMockOntologyApi()
  )
  return withUnsupportedMethods({
    listOntologies: (query) => mock.then((api) => api.listOntologies(query)),
    createOntology: (input) => mock.then((api) => api.createOntology(input)),
    getOntology: (ontologyId) => mock.then((api) => api.getOntology(ontologyId)),
    replaceOntology: (ontologyId, document, etag) =>
      mock.then((api) => api.replaceOntology(ontologyId, document, etag)),
    deleteOntology: (ontologyId, etag) =>
      mock.then((api) => api.deleteOntology(ontologyId, etag)),
    getObjectTypeNeighborhood: (ontologyId, objectTypeId, depth) =>
      mock.then((api) =>
        api.getObjectTypeNeighborhood(ontologyId, objectTypeId, depth)
      ),
  })
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
