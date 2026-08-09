import type { OntologyDocument } from "@/features/ontology-editor/types"

// 崩溃恢复草稿：单个 object store，keyPath 为 ontology_id，整表仅此一种记录。
export type OntologyDraft = {
  ontology_id: string
  base_etag: string
  candidate: OntologyDocument
}

export type OntologyDraftStore = {
  loadDraft(ontologyId: string): Promise<OntologyDraft | null>
  saveDraft(draft: OntologyDraft): Promise<void>
  clearDraft(ontologyId: string): Promise<void>
}

const DATABASE_NAME = "stratum-ontology-editor"
const DATABASE_VERSION = 1
const STORE_NAME = "drafts"

function promisifyRequest<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () =>
      reject(request.error ?? new Error("indexeddb request failed"))
  })
}

function openDatabase(factory: IDBFactory): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = factory.open(DATABASE_NAME, DATABASE_VERSION)
    request.onupgradeneeded = () => {
      request.result.createObjectStore(STORE_NAME, { keyPath: "ontology_id" })
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () =>
      reject(request.error ?? new Error("indexeddb open failed"))
  })
}

async function withStore<T>(
  factory: IDBFactory,
  mode: IDBTransactionMode,
  run: (store: IDBObjectStore) => IDBRequest<T>
): Promise<T> {
  const database = await openDatabase(factory)
  try {
    const transaction = database.transaction(STORE_NAME, mode)
    return await promisifyRequest(run(transaction.objectStore(STORE_NAME)))
  } finally {
    database.close()
  }
}

export function createOntologyDraftStore(
  factory: IDBFactory
): OntologyDraftStore {
  return {
    loadDraft: async (ontologyId) => {
      const result = await withStore(factory, "readonly", (store) =>
        store.get(ontologyId)
      )
      return (result as OntologyDraft | undefined) ?? null
    },
    saveDraft: async (draft) => {
      // structuredClone 立即暴露不可克隆数据，并让存储值与调用方引用脱钩
      await withStore(factory, "readwrite", (store) =>
        store.put(structuredClone(draft))
      )
    },
    clearDraft: async (ontologyId) => {
      await withStore(factory, "readwrite", (store) => store.delete(ontologyId))
    },
  }
}

// 浏览器环境入口；SSR 或 IndexedDB 不可用时返回 null，调用方按无草稿处理。
export function browserOntologyDraftStore(): OntologyDraftStore | null {
  if (typeof window === "undefined") return null
  try {
    return createOntologyDraftStore(window.indexedDB)
  } catch {
    return null
  }
}
