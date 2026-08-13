import { describe, expect, it } from "vitest"

import { ApiError } from "@/lib/stratum/api"
import type { OntologyEditorAction } from "@/features/ontology-editor/reducer"
import {
  attemptOntologySave,
  type OntologySaveDependencies,
} from "@/features/ontology-editor/save"
import type { OntologyDocument } from "@/features/ontology-editor/types"

const ONTOLOGY_ID = "0198f5e8-92ce-7c52-b55f-ecdc06090f4a"

function makeDocument(name = "support_domain"): OntologyDocument {
  return {
    id: ONTOLOGY_ID,
    name,
    display_name: "Support domain",
    object_types: [],
    link_types: [],
    canvas: { positions: [] },
  }
}

function createHarness(api: Partial<OntologySaveDependencies["api"]>) {
  const actions: OntologyEditorAction[] = []
  const calls = { replace: 0, get: 0 }
  const dependencies: OntologySaveDependencies = {
    api: {
      replaceOntology: async (id, document, etag) => {
        calls.replace += 1
        if (api.replaceOntology)
          return api.replaceOntology(id, document, etag)
        throw new Error("replaceOntology not stubbed")
      },
      getOntology: async (id) => {
        calls.get += 1
        if (api.getOntology) return api.getOntology(id)
        throw new Error("getOntology not stubbed")
      },
    },
    dispatch: (action) => {
      actions.push(action)
    },
  }
  return { actions, calls, dependencies }
}

const input = { ontologyId: ONTOLOGY_ID, document: makeDocument(), baseEtag: '"rev-1"' }

describe("attemptOntologySave", () => {
  it("dispatches save_succeeded with the new etag on 204", async () => {
    const { actions, dependencies } = createHarness({
      replaceOntology: async () => ({ etag: '"rev-2"' }),
    })

    const result = await attemptOntologySave(dependencies, input)
    expect(result).toEqual({ outcome: "saved", etag: '"rev-2"' })
    expect(actions).toEqual([{ type: "save_succeeded", etag: '"rev-2"' }])
  })

  it("passes the in-flight document and base etag to PUT", async () => {
    let seen: { id: string; doc: OntologyDocument; etag: string } | null = null
    const { dependencies } = createHarness({
      replaceOntology: async (id, doc, etag) => {
        seen = { id, doc, etag }
        return { etag: '"rev-2"' }
      },
    })

    await attemptOntologySave(dependencies, input)
    expect(seen).toEqual({
      id: ONTOLOGY_ID,
      doc: input.document,
      etag: '"rev-1"',
    })
  })

  it("412 re-reads the remote and dispatches save_conflict without retrying", async () => {
    const remoteDocument = makeDocument("remote_name")
    const { actions, calls, dependencies } = createHarness({
      replaceOntology: async () => {
        throw new ApiError("ontology_precondition_failed", 412, "stale etag")
      },
      getOntology: async () => ({ document: remoteDocument, etag: '"rev-9"', location: null }),
    })

    const result = await attemptOntologySave(dependencies, input)
    expect(result).toEqual({ outcome: "conflict" })
    expect(calls.replace).toBe(1)
    expect(actions).toEqual([
      {
        type: "save_conflict",
        remote: { document: remoteDocument, etag: '"rev-9"' },
      },
    ])
  })

  it("412 with a failing re-read dispatches save_failed", async () => {
    const readError = new ApiError("ontology_store_unavailable", 503, "down")
    const { actions, dependencies } = createHarness({
      replaceOntology: async () => {
        throw new ApiError("ontology_precondition_failed", 412, "stale etag")
      },
      getOntology: async () => {
        throw readError
      },
    })

    const result = await attemptOntologySave(dependencies, input)
    expect(result).toEqual({ outcome: "failed" })
    expect(actions).toEqual([{ type: "save_failed", error: readError }])
  })

  it("422 dispatches save_invalid with the response violations", async () => {
    const violations = [
      {
        code: "duplicate_object_type_name",
        path: "/object_types/1/name",
        message: "object type name is already used in this ontology",
      },
    ]
    const { actions, dependencies } = createHarness({
      replaceOntology: async () => {
        throw new ApiError(
          "invalid_ontology_schema",
          422,
          "ontology schema is invalid",
          violations
        )
      },
    })

    const result = await attemptOntologySave(dependencies, input)
    expect(result).toEqual({ outcome: "invalid" })
    expect(actions).toEqual([{ type: "save_invalid", violations }])
  })

  it("other HTTP errors dispatch save_failed without re-reading", async () => {
    const error = new ApiError("ontology_payload_too_large", 413, "too large")
    const { actions, calls, dependencies } = createHarness({
      replaceOntology: async () => {
        throw error
      },
    })

    const result = await attemptOntologySave(dependencies, input)
    expect(result).toEqual({ outcome: "failed" })
    expect(calls.get).toBe(0)
    expect(actions).toEqual([{ type: "save_failed", error }])
  })

  it("lost response: treats the save as committed when remote equals in-flight", async () => {
    const { actions, dependencies } = createHarness({
      replaceOntology: async () => {
        throw new TypeError("fetch failed")
      },
      getOntology: async () => ({
        document: makeDocument(),
        etag: '"rev-2"',
        location: null,
      }),
    })

    const result = await attemptOntologySave(dependencies, input)
    expect(result).toEqual({ outcome: "saved", etag: '"rev-2"' })
    expect(actions).toEqual([{ type: "save_succeeded", etag: '"rev-2"' }])
  })

  it("lost response: keeps candidate and marks unsaved when remote differs", async () => {
    const { actions, dependencies } = createHarness({
      replaceOntology: async () => {
        throw new TypeError("fetch failed")
      },
      getOntology: async () => ({
        document: makeDocument("older_name"),
        etag: '"rev-1"',
        location: null,
      }),
    })

    const result = await attemptOntologySave(dependencies, input)
    expect(result).toEqual({ outcome: "failed" })
    expect(actions).toHaveLength(1)
    expect(actions[0]).toMatchObject({ type: "save_failed" })
    const failure = actions[0] as Extract<
      OntologyEditorAction,
      { type: "save_failed" }
    >
    expect(failure.error.code).toBe("save_unconfirmed")
  })

  it("lost response: dispatches save_failed when the re-read also fails", async () => {
    const { actions, dependencies } = createHarness({
      replaceOntology: async () => {
        throw new TypeError("fetch failed")
      },
      getOntology: async () => {
        throw new TypeError("still offline")
      },
    })

    const result = await attemptOntologySave(dependencies, input)
    expect(result).toEqual({ outcome: "failed" })
    expect(actions).toHaveLength(1)
    expect(actions[0]).toMatchObject({ type: "save_failed" })
    const failure = actions[0] as Extract<
      OntologyEditorAction,
      { type: "save_failed" }
    >
    expect(failure.error.code).toBe("connection_error")
  })
})
