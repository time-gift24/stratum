import { describe, expect, it } from "vitest"

import {
  createOntologyDraftStore,
  type OntologyDraft,
} from "@/features/ontology-editor/recovery"
import type { OntologyDocument } from "@/features/ontology-editor/types"

function sampleDocument(id: string): OntologyDocument {
  return {
    id,
    name: "support_domain",
    display_name: "Support domain",
    object_types: [],
    link_types: [],
    canvas: { positions: [] },
  }
}

type FakeRequest<T> = {
  result: T
  error: unknown
  onsuccess: (() => void) | null
  onerror: (() => void) | null
  onupgradeneeded: (() => void) | null
}

// 内存版 IndexedDB fake：覆盖 open/upgrade/transaction/get/put/delete 的 glue 路径。
function createFakeIndexedDB() {
  const records = new Map<string, OntologyDraft>()
  let upgraded = false

  const succeed = <T>(value: T): FakeRequest<T> => {
    const request: FakeRequest<T> = {
      result: value,
      error: null,
      onsuccess: null,
      onerror: null,
      onupgradeneeded: null,
    }
    queueMicrotask(() => request.onsuccess?.())
    return request
  }

  const database = {
    createObjectStore: () => ({}),
    transaction: () => ({
      objectStore: () => ({
        get: (key: string) => succeed(records.get(key)),
        put: (value: OntologyDraft) => {
          records.set(value.ontology_id, value)
          return succeed(value.ontology_id)
        },
        delete: (key: string) => {
          records.delete(key)
          return succeed(undefined)
        },
      }),
    }),
    close: () => {},
  }

  const factory = {
    open: () => {
      const request: FakeRequest<typeof database> = {
        result: database,
        error: null,
        onsuccess: null,
        onerror: null,
        onupgradeneeded: null,
      }
      queueMicrotask(() => {
        if (!upgraded) {
          upgraded = true
          request.onupgradeneeded?.()
        }
        request.onsuccess?.()
      })
      return request
    },
  }

  return {
    factory: factory as unknown as IDBFactory,
    records,
  }
}

function createFailingIndexedDB(failure: Error): IDBFactory {
  const fail = <T>(): FakeRequest<T> => {
    const request: FakeRequest<T> = {
      result: undefined as T,
      error: failure,
      onsuccess: null,
      onerror: null,
      onupgradeneeded: null,
    }
    queueMicrotask(() => request.onerror?.())
    return request
  }
  return {
    open: () => fail(),
  } as unknown as IDBFactory
}

describe("createOntologyDraftStore", () => {
  it("returns null when no draft exists", async () => {
    const { factory } = createFakeIndexedDB()
    const store = createOntologyDraftStore(factory)
    await expect(store.loadDraft("missing")).resolves.toBeNull()
  })

  it("round-trips a saved draft by ontology_id", async () => {
    const { factory } = createFakeIndexedDB()
    const store = createOntologyDraftStore(factory)
    const draft: OntologyDraft = {
      ontology_id: "0198f5e8-92ce-7c52-b55f-ecdc06090f4a",
      base_etag: '"rev-1"',
      candidate: sampleDocument("0198f5e8-92ce-7c52-b55f-ecdc06090f4a"),
    }

    await store.saveDraft(draft)
    await expect(store.loadDraft(draft.ontology_id)).resolves.toEqual(draft)
  })

  it("stores a detached clone, not the caller's reference", async () => {
    const { factory, records } = createFakeIndexedDB()
    const store = createOntologyDraftStore(factory)
    const draft: OntologyDraft = {
      ontology_id: "ontology-1",
      base_etag: '"rev-1"',
      candidate: sampleDocument("ontology-1"),
    }

    await store.saveDraft(draft)
    expect(records.get("ontology-1")).not.toBe(draft)
    expect(records.get("ontology-1")?.candidate).not.toBe(draft.candidate)
    expect(records.get("ontology-1")).toEqual(draft)
  })

  it("overwrites an existing draft for the same ontology_id", async () => {
    const { factory } = createFakeIndexedDB()
    const store = createOntologyDraftStore(factory)
    const base: OntologyDraft = {
      ontology_id: "ontology-1",
      base_etag: '"rev-1"',
      candidate: sampleDocument("ontology-1"),
    }
    await store.saveDraft(base)
    await store.saveDraft({ ...base, base_etag: '"rev-2"' })

    const loaded = await store.loadDraft("ontology-1")
    expect(loaded?.base_etag).toBe('"rev-2"')
  })

  it("clears only the matching draft", async () => {
    const { factory } = createFakeIndexedDB()
    const store = createOntologyDraftStore(factory)
    await store.saveDraft({
      ontology_id: "ontology-1",
      base_etag: '"rev-1"',
      candidate: sampleDocument("ontology-1"),
    })
    await store.saveDraft({
      ontology_id: "ontology-2",
      base_etag: '"rev-3"',
      candidate: sampleDocument("ontology-2"),
    })

    await store.clearDraft("ontology-1")
    await expect(store.loadDraft("ontology-1")).resolves.toBeNull()
    await expect(store.loadDraft("ontology-2")).resolves.not.toBeNull()
  })

  it("rejects when the database cannot be opened", async () => {
    const failure = new Error("blocked")
    const store = createOntologyDraftStore(createFailingIndexedDB(failure))
    await expect(store.loadDraft("ontology-1")).rejects.toBe(failure)
  })
})
