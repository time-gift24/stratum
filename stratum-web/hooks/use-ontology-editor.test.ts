import { beforeEach, describe, expect, it, vi } from "vitest"

import type { OntologyDraftStore } from "@/features/ontology-editor/recovery"
import type { OntologyDocument } from "@/features/ontology-editor/types"
import type { OntologyResource, StratumApi } from "@/lib/stratum/api"

type RegisteredEffect = () => void | (() => void)

const reactHarness = vi.hoisted(() => ({
  dispatch: vi.fn(),
  effects: [] as RegisteredEffect[],
}))

vi.mock("react", () => ({
  useCallback: <Callback>(callback: Callback) => callback,
  useEffect: (effect: RegisteredEffect) => {
    reactHarness.effects.push(effect)
  },
  useMemo: <Value>(factory: () => Value) => factory(),
  useReducer: <State>(_reducer: unknown, initialState: State) =>
    [initialState, reactHarness.dispatch] as const,
  useRef: <Value>(value: Value) => ({ current: value }),
  useState: <Value>(value: Value) => [value, vi.fn()] as const,
}))

import { useOntologyEditor } from "@/hooks/use-ontology-editor"
import {
  readCreatedOntologyHandoff,
  stageCreatedOntology,
} from "@/lib/stratum/ontology-navigation-handoff"

const ONTOLOGY_ID = "0198f5e8-92ce-7c52-b55f-ecdc06090f4a"

function document(): OntologyDocument {
  return {
    id: ONTOLOGY_ID,
    name: "support_domain",
    display_name: "Support domain",
    object_types: [],
    link_types: [],
    canvas: { positions: [] },
  }
}

function resource(): OntologyResource {
  return {
    document: document(),
    etag: '"rev-1"',
    location: `/v1/ontologies/${ONTOLOGY_ID}`,
  }
}

function apiWithGet(getOntology: StratumApi["getOntology"]): StratumApi {
  return { getOntology } as StratumApi
}

function runEffects(): readonly (() => void)[] {
  return reactHarness.effects.flatMap((effect) => {
    const cleanup = effect()
    return typeof cleanup === "function" ? [cleanup] : []
  })
}

beforeEach(() => {
  reactHarness.dispatch.mockReset()
  reactHarness.effects.length = 0
  readCreatedOntologyHandoff("clear-pending-test-resource")
})

describe("useOntologyEditor load orchestration", () => {
  it("keeps a successful resource load ready when optional draft recovery fails", async () => {
    const loaded = resource()
    const getOntology = vi
      .fn<StratumApi["getOntology"]>()
      .mockResolvedValue(loaded)
    const draftFailure = new Error("indexeddb unavailable")
    const draftStore: OntologyDraftStore = {
      loadDraft: vi.fn().mockRejectedValue(draftFailure),
      saveDraft: vi.fn().mockResolvedValue(undefined),
      clearDraft: vi.fn().mockResolvedValue(undefined),
    }

    useOntologyEditor(ONTOLOGY_ID, {
      api: apiWithGet(getOntology),
      draftStore,
    })
    const cleanups = runEffects()

    await vi.waitFor(() =>
      expect(reactHarness.dispatch).toHaveBeenCalledWith({
        type: "load_succeeded",
        ontologyId: ONTOLOGY_ID,
        document: loaded.document,
        etag: loaded.etag,
      })
    )
    expect(reactHarness.dispatch).not.toHaveBeenCalledWith(
      expect.objectContaining({ type: "load_failed" })
    )
    expect(reactHarness.dispatch).not.toHaveBeenCalledWith({
      type: "draft_checked",
    })
    for (const cleanup of cleanups) cleanup()
  })

  it("keeps a create handoff across Strict Mode effect replay without a GET", async () => {
    const created = resource()
    stageCreatedOntology(created)
    const getOntology = vi.fn<StratumApi["getOntology"]>()

    useOntologyEditor(ONTOLOGY_ID, {
      api: apiWithGet(getOntology),
      draftStore: null,
    })
    const replayCleanups = runEffects()
    for (const cleanup of replayCleanups) cleanup()
    const finalCleanup = reactHarness.effects[1]?.()

    await vi.waitFor(() =>
      expect(reactHarness.dispatch).toHaveBeenCalledWith({
        type: "load_succeeded",
        ontologyId: ONTOLOGY_ID,
        document: created.document,
        etag: created.etag,
      })
    )
    expect(getOntology).not.toHaveBeenCalled()
    expect(
      reactHarness.dispatch.mock.calls.filter(
        ([action]) => action.type === "load_succeeded"
      )
    ).toHaveLength(1)
    if (typeof finalCleanup === "function") finalCleanup()
  })
})
