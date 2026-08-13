import { describe, expect, it } from "vitest"

import type { OntologyResource } from "@/lib/stratum/api"
import {
  finishCreatedOntologyHandoff,
  readCreatedOntologyHandoff,
  stageCreatedOntology,
} from "@/lib/stratum/ontology-navigation-handoff"

const resource = (id: string): OntologyResource => ({
  document: {
    id,
    name: "memory",
    display_name: "Memory",
    object_types: [],
    link_types: [],
    canvas: { positions: [] },
  },
  etag: `"ontology:${id}:1"`,
  location: `/v1/ontologies/${id}`,
})

describe("created ontology navigation handoff", () => {
  it("keeps the POST resource through effect replay and clears after commit", () => {
    const created = resource("0198f5e8-92ce-7c52-b55f-ecdc06090f4a")
    stageCreatedOntology(created)

    expect(readCreatedOntologyHandoff(created.document.id)).toBe(created)
    expect(readCreatedOntologyHandoff(created.document.id)).toBe(created)
    finishCreatedOntologyHandoff(created.document.id)
    expect(readCreatedOntologyHandoff(created.document.id)).toBeNull()
  })

  it("does not leak a staged resource into a different editor", () => {
    stageCreatedOntology(resource("0198f5e8-92ce-7c52-b55f-ecdc06090f4a"))

    expect(
      readCreatedOntologyHandoff("0198f5e8-92ce-7c52-b55f-ecdc06090f4b")
    ).toBeNull()
  })
})
