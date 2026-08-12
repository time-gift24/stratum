import { describe, expect, it } from "vitest"

import { ApiError } from "@/lib/stratum/api"
import {
  canClearOntologyDraft,
  initialOntologyEditorState,
  isOntologyEditorDirty,
  ontologyDocumentsEqual,
  ontologyEditorReducer,
  type OntologyEditorState,
} from "@/features/ontology-editor/reducer"
import type { OntologyDraft } from "@/features/ontology-editor/recovery"
import type {
  OntologyDocument,
  OntologyLinkType,
  OntologyObjectType,
  OntologyProperty,
} from "@/features/ontology-editor/types"

const ONTOLOGY_ID = "0198f5e8-92ce-7c52-b55f-ecdc06090f4a"

function makeProperty(id: string, name = "email"): OntologyProperty {
  return {
    id,
    name,
    display_name: name,
    value_type: "string",
    required: true,
  }
}

function makeObjectType(
  id: string,
  name = "customer",
  properties: readonly OntologyProperty[] = []
): OntologyObjectType {
  return { id, name, display_name: name, properties }
}

function makeLinkType(
  id: string,
  sourceId: string,
  targetId: string
): OntologyLinkType {
  return {
    id,
    name: `link_${id}`,
    display_name: id,
    source_object_type_id: sourceId,
    target_object_type_id: targetId,
    source_to_target: "many",
    target_to_source: "one",
  }
}

function makeDocument(
  overrides: Partial<OntologyDocument> = {}
): OntologyDocument {
  return {
    id: ONTOLOGY_ID,
    name: "support_domain",
    display_name: "Support domain",
    object_types: [],
    link_types: [],
    canvas: { positions: [] },
    ...overrides,
  }
}

function readyState(
  document: OntologyDocument = makeDocument(),
  etag = '"rev-1"'
): OntologyEditorState {
  return ontologyEditorReducer(
    ontologyEditorReducer(initialOntologyEditorState, {
      type: "load_started",
      ontologyId: ONTOLOGY_ID,
    }),
    { type: "load_succeeded", ontologyId: ONTOLOGY_ID, document, etag }
  )
}

describe("load lifecycle", () => {
  it("acknowledges the loaded document and deep-copies the candidate", () => {
    const document = makeDocument({ object_types: [makeObjectType("ot-1")] })
    const state = readyState(document)

    expect(state.phase).toBe("ready")
    expect(state.acknowledged).toEqual({ document, etag: '"rev-1"' })
    expect(state.candidate).toEqual(document)
    expect(state.candidate).not.toBe(document)
    expect(state.candidate?.object_types[0]).not.toBe(document.object_types[0])
    expect(isOntologyEditorDirty(state)).toBe(false)
  })

  it("marks 404 loads as missing and other failures as error", () => {
    const loading = ontologyEditorReducer(initialOntologyEditorState, {
      type: "load_started",
      ontologyId: ONTOLOGY_ID,
    })
    const missing = ontologyEditorReducer(loading, {
      type: "load_failed",
      ontologyId: ONTOLOGY_ID,
      error: new ApiError("ontology_not_found", 404, "not found"),
    })
    const failed = ontologyEditorReducer(loading, {
      type: "load_failed",
      ontologyId: ONTOLOGY_ID,
      error: new ApiError("internal_error", 500, "boom"),
    })

    expect(missing.phase).toBe("missing")
    expect(failed.phase).toBe("error")
    expect(failed.error?.code).toBe("internal_error")
  })

  it("ignores load results for a stale ontology id", () => {
    const loading = ontologyEditorReducer(initialOntologyEditorState, {
      type: "load_started",
      ontologyId: "other",
    })
    const state = ontologyEditorReducer(loading, {
      type: "load_succeeded",
      ontologyId: ONTOLOGY_ID,
      document: makeDocument(),
      etag: '"rev-1"',
    })
    expect(state.phase).toBe("loading")
    expect(state.acknowledged).toBeNull()
  })
})

describe("editing actions", () => {
  it("adds, updates, and removes object types", () => {
    let state = readyState()
    state = ontologyEditorReducer(state, {
      type: "object_type_added",
      objectType: makeObjectType("ot-1"),
    })
    expect(state.candidate?.object_types).toHaveLength(1)
    expect(isOntologyEditorDirty(state)).toBe(true)

    state = ontologyEditorReducer(state, {
      type: "object_type_updated",
      objectType: makeObjectType("ot-1", "account"),
    })
    expect(state.candidate?.object_types[0]?.name).toBe("account")

    state = ontologyEditorReducer(state, {
      type: "object_type_removed",
      objectTypeId: "ot-1",
    })
    expect(state.candidate?.object_types).toHaveLength(0)
    expect(isOntologyEditorDirty(state)).toBe(false)
  })

  it("cascades object type removal to link types and canvas positions", () => {
    const document = makeDocument({
      object_types: [
        makeObjectType("ot-1"),
        makeObjectType("ot-2"),
        makeObjectType("ot-3"),
      ],
      link_types: [
        makeLinkType("lt-1", "ot-1", "ot-2"),
        makeLinkType("lt-2", "ot-3", "ot-2"),
        makeLinkType("lt-3", "ot-2", "ot-1"),
      ],
      canvas: {
        positions: [
          { object_type_id: "ot-1", x: 0, y: 0 },
          { object_type_id: "ot-2", x: 10, y: 10 },
        ],
      },
    })
    const state = ontologyEditorReducer(readyState(document), {
      type: "object_type_removed",
      objectTypeId: "ot-1",
    })

    expect(state.candidate?.object_types.map((ot) => ot.id)).toEqual([
      "ot-2",
      "ot-3",
    ])
    expect(state.candidate?.link_types.map((lt) => lt.id)).toEqual(["lt-2"])
    expect(state.candidate?.canvas.positions).toEqual([
      { object_type_id: "ot-2", x: 10, y: 10 },
    ])
  })

  it("adds, updates, and removes properties within an object type", () => {
    let state = readyState(
      makeDocument({ object_types: [makeObjectType("ot-1")] })
    )
    state = ontologyEditorReducer(state, {
      type: "property_added",
      objectTypeId: "ot-1",
      property: makeProperty("p-1"),
    })
    state = ontologyEditorReducer(state, {
      type: "property_added",
      objectTypeId: "ot-1",
      property: makeProperty("p-2", "age"),
    })
    expect(state.candidate?.object_types[0]?.properties).toHaveLength(2)

    state = ontologyEditorReducer(state, {
      type: "property_updated",
      objectTypeId: "ot-1",
      property: { ...makeProperty("p-1"), required: false },
    })
    expect(state.candidate?.object_types[0]?.properties[0]?.required).toBe(
      false
    )

    state = ontologyEditorReducer(state, {
      type: "property_removed",
      objectTypeId: "ot-1",
      propertyId: "p-1",
    })
    expect(
      state.candidate?.object_types[0]?.properties.map((p) => p.id)
    ).toEqual(["p-2"])
  })

  it("ignores property edits targeting an unknown object type", () => {
    const state = readyState()
    const next = ontologyEditorReducer(state, {
      type: "property_added",
      objectTypeId: "missing",
      property: makeProperty("p-1"),
    })
    expect(next).toBe(state)
  })

  it("adds, updates, and removes link types", () => {
    let state = readyState(
      makeDocument({
        object_types: [makeObjectType("ot-1"), makeObjectType("ot-2")],
      })
    )
    state = ontologyEditorReducer(state, {
      type: "link_type_added",
      linkType: makeLinkType("lt-1", "ot-1", "ot-2"),
    })
    expect(state.candidate?.link_types).toHaveLength(1)

    state = ontologyEditorReducer(state, {
      type: "link_type_updated",
      linkType: {
        ...makeLinkType("lt-1", "ot-1", "ot-2"),
        source_to_target: "one",
      },
    })
    expect(state.candidate?.link_types[0]?.source_to_target).toBe("one")

    state = ontologyEditorReducer(state, {
      type: "link_type_removed",
      linkTypeId: "lt-1",
    })
    expect(state.candidate?.link_types).toHaveLength(0)
  })

  it("creates, updates, and cascades canvas positions", () => {
    let state = readyState(
      makeDocument({
        object_types: [makeObjectType("ot-1"), makeObjectType("ot-2")],
      })
    )
    state = ontologyEditorReducer(state, {
      type: "position_set",
      objectTypeId: "ot-1",
      x: 5,
      y: 6,
    })
    expect(state.candidate?.canvas.positions).toEqual([
      { object_type_id: "ot-1", x: 5, y: 6 },
    ])

    state = ontologyEditorReducer(state, {
      type: "position_set",
      objectTypeId: "ot-1",
      x: 7,
      y: 8,
    })
    expect(state.candidate?.canvas.positions).toEqual([
      { object_type_id: "ot-1", x: 7, y: 8 },
    ])

    state = ontologyEditorReducer(state, {
      type: "object_type_removed",
      objectTypeId: "ot-1",
    })
    expect(state.candidate?.canvas.positions).toEqual([])
  })

  it("refuses a position for an unknown object type", () => {
    const state = readyState()
    const next = ontologyEditorReducer(state, {
      type: "position_set",
      objectTypeId: "missing",
      x: 1,
      y: 2,
    })
    expect(next).toBe(state)
  })

  it("does not edit before a document is loaded", () => {
    const state = ontologyEditorReducer(initialOntologyEditorState, {
      type: "object_type_added",
      objectType: makeObjectType("ot-1"),
    })
    expect(state.candidate).toBeNull()
  })
})

// 加载后先做一笔编辑，使 candidate 相对 acknowledged 为 dirty
function dirtyState(): OntologyEditorState {
  return ontologyEditorReducer(readyState(), {
    type: "object_type_added",
    objectType: makeObjectType("ot-1"),
  })
}

describe("save lifecycle", () => {
  it("save_started snapshots candidate and base etag into in_flight", () => {
    const state = ontologyEditorReducer(dirtyState(), {
      type: "save_started",
    })

    expect(state.inFlight).toEqual({
      document: state.candidate,
      baseEtag: '"rev-1"',
    })
    expect(isOntologyEditorDirty(state)).toBe(true)
  })

  it("ignores save_started while a save is already in flight", () => {
    let state = readyState(
      makeDocument({ object_types: [makeObjectType("ot-1")] })
    )
    state = ontologyEditorReducer(state, { type: "save_started" })
    const inFlight = state.inFlight
    state = ontologyEditorReducer(state, {
      type: "object_type_added",
      objectType: makeObjectType("ot-2"),
    })
    state = ontologyEditorReducer(state, { type: "save_started" })
    expect(state.inFlight).toBe(inFlight)
  })

  it("save success with no concurrent edits leaves a clean editor", () => {
    let state = readyState(
      makeDocument({ object_types: [makeObjectType("ot-1")] })
    )
    state = ontologyEditorReducer(state, { type: "save_started" })
    state = ontologyEditorReducer(state, {
      type: "save_succeeded",
      etag: '"rev-2"',
    })

    expect(state.acknowledged?.etag).toBe('"rev-2"')
    expect(state.acknowledged?.document).toEqual(state.candidate)
    expect(state.inFlight).toBeNull()
    expect(isOntologyEditorDirty(state)).toBe(false)
  })

  it("save success with concurrent edits acknowledges only the in-flight snapshot", () => {
    let state = readyState(
      makeDocument({ object_types: [makeObjectType("ot-1")] })
    )
    state = ontologyEditorReducer(state, { type: "save_started" })
    // 飞行期间继续编辑 candidate
    state = ontologyEditorReducer(state, {
      type: "object_type_added",
      objectType: makeObjectType("ot-2"),
    })
    state = ontologyEditorReducer(state, {
      type: "save_succeeded",
      etag: '"rev-2"',
    })

    expect(state.acknowledged?.etag).toBe('"rev-2"')
    expect(
      state.acknowledged?.document.object_types.map((ot) => ot.id)
    ).toEqual(["ot-1"])
    expect(state.candidate?.object_types.map((ot) => ot.id)).toEqual([
      "ot-1",
      "ot-2",
    ])
    expect(state.inFlight).toBeNull()
    expect(isOntologyEditorDirty(state)).toBe(true)
  })

  it("412 keeps candidate intact and records the remote for reconciliation", () => {
    let state = dirtyState()
    const candidateBefore = state.candidate
    state = ontologyEditorReducer(state, { type: "save_started" })
    const remote = {
      document: makeDocument({ object_types: [makeObjectType("ot-remote")] }),
      etag: '"rev-9"',
    }
    state = ontologyEditorReducer(state, { type: "save_conflict", remote })

    expect(state.candidate).toBe(candidateBefore)
    expect(state.inFlight).toBeNull()
    expect(state.conflict).toEqual({ remote })
    expect(state.acknowledged?.etag).toBe('"rev-1"')
    expect(isOntologyEditorDirty(state)).toBe(true)
  })

  it("reconcile local keeps candidate and re-bases acknowledged on the remote", () => {
    let state = readyState(
      makeDocument({ object_types: [makeObjectType("ot-1")] })
    )
    state = ontologyEditorReducer(state, { type: "save_started" })
    const remote = {
      document: makeDocument({ object_types: [makeObjectType("ot-remote")] }),
      etag: '"rev-9"',
    }
    state = ontologyEditorReducer(state, { type: "save_conflict", remote })
    state = ontologyEditorReducer(state, {
      type: "conflict_resolved",
      resolution: "local",
      remote,
    })

    expect(state.conflict).toBeNull()
    expect(state.acknowledged).toEqual(remote)
    expect(state.candidate?.object_types.map((ot) => ot.id)).toEqual(["ot-1"])
    expect(isOntologyEditorDirty(state)).toBe(true)
  })

  it("reconcile remote replaces candidate with a deep copy of the remote", () => {
    let state = readyState(
      makeDocument({ object_types: [makeObjectType("ot-1")] })
    )
    state = ontologyEditorReducer(state, { type: "save_started" })
    const remote = {
      document: makeDocument({ object_types: [makeObjectType("ot-remote")] }),
      etag: '"rev-9"',
    }
    state = ontologyEditorReducer(state, { type: "save_conflict", remote })
    state = ontologyEditorReducer(state, {
      type: "conflict_resolved",
      resolution: "remote",
      remote,
    })

    expect(state.candidate).toEqual(remote.document)
    expect(state.candidate).not.toBe(remote.document)
    expect(state.acknowledged).toEqual(remote)
    expect(isOntologyEditorDirty(state)).toBe(false)
  })

  it("422 keeps candidate intact and records violations in order", () => {
    let state = dirtyState()
    const candidateBefore = state.candidate
    state = ontologyEditorReducer(state, { type: "save_started" })
    const violations = [
      {
        code: "duplicate_object_type_name",
        path: "/object_types/0/name",
        message: "object type name is already used in this ontology",
      },
    ]
    state = ontologyEditorReducer(state, { type: "save_invalid", violations })

    expect(state.candidate).toBe(candidateBefore)
    expect(state.inFlight).toBeNull()
    expect(state.violations).toEqual(violations)
    expect(state.violationDocument).toBe(candidateBefore)
    expect(isOntologyEditorDirty(state)).toBe(true)
  })

  it("422 keeps the exact in-flight snapshot while concurrent edits continue", () => {
    let state = dirtyState()
    state = ontologyEditorReducer(state, { type: "save_started" })
    const submitted = state.inFlight?.document
    expect(submitted).not.toBeNull()

    state = ontologyEditorReducer(state, {
      type: "object_type_added",
      objectType: makeObjectType("ot-2"),
    })
    expect(state.candidate?.object_types.map(({ id }) => id)).toEqual([
      "ot-1",
      "ot-2",
    ])

    state = ontologyEditorReducer(state, {
      type: "save_invalid",
      violations: [
        {
          code: "invalid_name",
          path: "/object_types/0/name",
          message: "invalid submitted name",
        },
      ],
    })

    expect(state.violationDocument).toBe(submitted)
    expect(state.violationDocument?.object_types.map(({ id }) => id)).toEqual([
      "ot-1",
    ])
    expect(state.candidate?.object_types.map(({ id }) => id)).toEqual([
      "ot-1",
      "ot-2",
    ])
    expect(state.inFlight).toBeNull()

    state = ontologyEditorReducer(state, {
      type: "object_type_added",
      objectType: makeObjectType("ot-3"),
    })
    expect(state.violations).toBeNull()
    expect(state.violationDocument).toBeNull()
    state = ontologyEditorReducer(state, { type: "save_started" })
    expect(state.inFlight?.document).toBe(state.candidate)
  })

  it("a new save attempt clears previous violations and conflict", () => {
    let state = readyState(
      makeDocument({ object_types: [makeObjectType("ot-1")] })
    )
    state = ontologyEditorReducer(state, { type: "save_started" })
    state = ontologyEditorReducer(state, {
      type: "save_invalid",
      violations: [{ code: "c", path: "/name", message: "m" }],
    })
    state = ontologyEditorReducer(state, { type: "save_started" })

    expect(state.violations).toBeNull()
    expect(state.conflict).toBeNull()
    expect(state.inFlight).not.toBeNull()
  })

  it("editing after a 422 clears the stale violations", () => {
    let state = dirtyState()
    state = ontologyEditorReducer(state, { type: "save_started" })
    state = ontologyEditorReducer(state, {
      type: "save_invalid",
      violations: [{ code: "c", path: "/object_types/0/name", message: "m" }],
    })
    expect(state.violations).not.toBeNull()

    state = ontologyEditorReducer(state, {
      type: "object_type_updated",
      objectType: makeObjectType("ot-1", "renamed"),
    })
    expect(state.violations).toBeNull()
    expect(state.violationDocument).toBeNull()
  })

  it("timeout re-read: remote equals in_flight resolves as success", () => {
    let state = readyState(
      makeDocument({ object_types: [makeObjectType("ot-1")] })
    )
    state = ontologyEditorReducer(state, { type: "save_started" })
    const inFlight = state.inFlight
    expect(inFlight).not.toBeNull()
    if (inFlight === null) return

    // hook 重读后确认远端已提交：按成功处理并采用远端 ETag
    const remote = { document: inFlight.document, etag: '"rev-2"' }
    expect(ontologyDocumentsEqual(remote.document, inFlight.document)).toBe(
      true
    )
    state = ontologyEditorReducer(state, {
      type: "save_succeeded",
      etag: remote.etag,
    })

    expect(state.acknowledged?.etag).toBe('"rev-2"')
    expect(isOntologyEditorDirty(state)).toBe(false)
  })

  it("timeout re-read: remote differs keeps candidate and marks the save failed", () => {
    let state = dirtyState()
    state = ontologyEditorReducer(state, { type: "save_started" })
    const candidateBefore = state.candidate

    const remoteDocument = makeDocument({
      object_types: [makeObjectType("ot-other")],
    })
    const inFlight = state.inFlight
    expect(inFlight).not.toBeNull()
    if (inFlight === null) return
    expect(ontologyDocumentsEqual(remoteDocument, inFlight.document)).toBe(
      false
    )
    state = ontologyEditorReducer(state, {
      type: "save_failed",
      error: new ApiError("save_unconfirmed", 0, "save result is unknown"),
    })

    expect(state.candidate).toBe(candidateBefore)
    expect(state.inFlight).toBeNull()
    expect(state.saveError?.code).toBe("save_unconfirmed")
    expect(isOntologyEditorDirty(state)).toBe(true)
  })

  it("ignores save_succeeded without an in-flight save", () => {
    const state = readyState()
    const next = ontologyEditorReducer(state, {
      type: "save_succeeded",
      etag: '"rev-2"',
    })
    expect(next).toBe(state)
  })
})

describe("crash recovery drafts", () => {
  const draftFor = (candidate: OntologyDocument): OntologyDraft => ({
    ontology_id: ONTOLOGY_ID,
    base_etag: '"rev-1"',
    candidate,
  })

  it("exposes a found draft only when it matches the loaded ontology", () => {
    const state = readyState()
    const matched = ontologyEditorReducer(state, {
      type: "draft_found",
      draft: draftFor(makeDocument()),
    })
    expect(matched.draftAvailable).not.toBeNull()
    expect(matched.draftChecked).toBe(true)
    expect(canClearOntologyDraft(matched)).toBe(false)

    const other = ontologyEditorReducer(state, {
      type: "draft_found",
      draft: { ...draftFor(makeDocument()), ontology_id: "other" },
    })
    expect(other.draftAvailable).toBeNull()
  })

  it("never clears a differing draft before recover or discard", () => {
    let state = readyState()
    const draft = draftFor(
      makeDocument({ object_types: [makeObjectType("ot-draft")] })
    )

    state = ontologyEditorReducer(state, { type: "draft_found", draft })

    expect(isOntologyEditorDirty(state)).toBe(false)
    expect(state.draftAvailable).toBe(draft)
    expect(canClearOntologyDraft(state)).toBe(false)

    state = ontologyEditorReducer(state, { type: "draft_discarded" })
    expect(canClearOntologyDraft(state)).toBe(true)
  })

  it("resets the draft check when the same ontology reloads", () => {
    let state = readyState()
    state = ontologyEditorReducer(state, { type: "draft_checked" })
    expect(canClearOntologyDraft(state)).toBe(true)

    state = ontologyEditorReducer(state, {
      type: "load_started",
      ontologyId: ONTOLOGY_ID,
    })
    state = ontologyEditorReducer(state, {
      type: "load_succeeded",
      ontologyId: ONTOLOGY_ID,
      document: makeDocument(),
      etag: '"rev-2"',
    })

    expect(state.draftChecked).toBe(false)
    expect(canClearOntologyDraft(state)).toBe(false)
  })

  it("allows cleanup only after a check finds no recovery candidate", () => {
    let state = readyState()
    expect(canClearOntologyDraft(state)).toBe(false)

    state = ontologyEditorReducer(state, { type: "draft_checked" })

    expect(state.draftChecked).toBe(true)
    expect(canClearOntologyDraft(state)).toBe(true)
  })

  it("restores the draft candidate and clears the offer", () => {
    const draftCandidate = makeDocument({
      object_types: [makeObjectType("ot-draft")],
    })
    let state = readyState()
    state = ontologyEditorReducer(state, {
      type: "draft_found",
      draft: draftFor(draftCandidate),
    })
    state = ontologyEditorReducer(state, { type: "draft_restored" })

    expect(state.candidate).toEqual(draftCandidate)
    expect(state.candidate).not.toBe(draftCandidate)
    expect(state.draftAvailable).toBeNull()
    expect(isOntologyEditorDirty(state)).toBe(true)
    expect(canClearOntologyDraft(state)).toBe(false)
  })

  it("discards the draft without touching candidate", () => {
    let state = readyState()
    const candidateBefore = state.candidate
    state = ontologyEditorReducer(state, {
      type: "draft_found",
      draft: draftFor(makeDocument({ object_types: [makeObjectType("ot-1")] })),
    })
    state = ontologyEditorReducer(state, { type: "draft_discarded" })

    expect(state.draftAvailable).toBeNull()
    expect(state.candidate).toBe(candidateBefore)
    expect(isOntologyEditorDirty(state)).toBe(false)
  })
})

describe("ontologyDocumentsEqual", () => {
  it("is insensitive to object key order", () => {
    const left = makeDocument({ description: "a" })
    const right: OntologyDocument = {
      canvas: { positions: [] },
      link_types: [],
      object_types: [],
      display_name: "Support domain",
      name: "support_domain",
      id: ONTOLOGY_ID,
      description: "a",
    }
    expect(ontologyDocumentsEqual(left, right)).toBe(true)
  })

  it("is sensitive to array order and values", () => {
    const left = makeDocument({
      object_types: [makeObjectType("ot-1"), makeObjectType("ot-2")],
    })
    const reversed = makeDocument({
      object_types: [makeObjectType("ot-2"), makeObjectType("ot-1")],
    })
    expect(ontologyDocumentsEqual(left, reversed)).toBe(false)
    expect(ontologyDocumentsEqual(left, makeDocument())).toBe(false)
  })
})
