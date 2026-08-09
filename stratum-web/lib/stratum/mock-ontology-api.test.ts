import { describe, expect, it } from "vitest"

import { ApiError } from "@/lib/stratum/api"
import {
  createMockOntologyApi,
  type OntologyApi,
} from "@/lib/stratum/mock-ontology-api"
import type { OntologyDocument } from "@/features/ontology-editor/types"

const createApi = (): OntologyApi => createMockOntologyApi({ delayMs: 0 })

async function findByName(
  api: OntologyApi,
  name: string
): Promise<OntologyDocument> {
  const page = await api.listOntologies()
  const summary = page.data.find((item) => item.name === name)
  expect(summary).toBeDefined()
  const resource = await api.getOntology(summary?.id as string)
  return resource.document
}

async function catchError(promise: Promise<unknown>): Promise<ApiError> {
  const failure = await promise.catch((error: unknown) => error)
  expect(failure).toBeInstanceOf(ApiError)
  return failure as ApiError
}

describe("mock ontology api", () => {
  it("lists the seeded ontologies sorted by -updated_at", async () => {
    const api = createApi()

    const page = await api.listOntologies({
      page: 1,
      perPage: 20,
      sort: "-updated_at",
    })
    expect(page.pagination).toEqual({ page: 1, per_page: 20, total: 3 })
    expect(page.data.map((item) => item.name)).toEqual([
      "blank_canvas",
      "customer_insight",
      "content_ops",
    ])
  })

  it("creates an ontology with etag and location", async () => {
    const api = createApi()

    const created = await api.createOntology({
      name: "logistics",
      displayName: "物流网络",
      description: "仓配链路模型",
    })
    expect(created.document.name).toBe("logistics")
    expect(created.document.object_types).toEqual([])
    expect(created.etag).toBe('"rev-1"')
    expect(created.location).toBe(
      `/v1/ontologies/${created.document.id}`
    )

    const resource = await api.getOntology(created.document.id)
    expect(resource.etag).toBe('"rev-1"')
    expect(resource.document).toEqual(created.document)
  })

  it("rejects a duplicate name on create with 409", async () => {
    const api = createApi()

    const error = await catchError(
      api.createOntology({ name: "customer_insight", displayName: "客户洞察" })
    )
    expect(error.status).toBe(409)
    expect(error.code).toBe("ontology_name_conflict")
  })

  it("rejects a duplicate name on replace with 409", async () => {
    const api = createApi()
    const target = await api.getOntology(
      (await findByName(api, "content_ops")).id
    )

    const error = await catchError(
      api.replaceOntology(
        target.document.id,
        { ...target.document, name: "customer_insight" },
        target.etag
      )
    )
    expect(error.status).toBe(409)
    expect(error.code).toBe("ontology_name_conflict")
  })

  it("returns 404 ontology_not_found for unknown ids", async () => {
    const api = createApi()

    const error = await catchError(
      api.getOntology("0198f5e8-92ce-7c52-b55f-ecdc06090f4a")
    )
    expect(error.status).toBe(404)
    expect(error.code).toBe("ontology_not_found")
  })

  it("bumps the etag on every successful replace", async () => {
    const api = createApi()
    const resource = await api.getOntology(
      (await findByName(api, "content_ops")).id
    )
    expect(resource.etag).toBe('"rev-1"')

    const first = await api.replaceOntology(
      resource.document.id,
      { ...resource.document, description: "第一次保存" },
      resource.etag
    )
    expect(first.etag).toBe('"rev-2"')

    const second = await api.replaceOntology(
      resource.document.id,
      { ...resource.document, description: "第二次保存" },
      first.etag
    )
    expect(second.etag).toBe('"rev-3"')

    const reread = await api.getOntology(resource.document.id)
    expect(reread.etag).toBe('"rev-3"')
    expect(reread.document.description).toBe("第二次保存")
  })

  it("rejects a stale etag on replace with 412", async () => {
    const api = createApi()
    const resource = await api.getOntology(
      (await findByName(api, "content_ops")).id
    )

    await api.replaceOntology(
      resource.document.id,
      resource.document,
      resource.etag
    )
    const error = await catchError(
      api.replaceOntology(
        resource.document.id,
        resource.document,
        resource.etag
      )
    )
    expect(error.status).toBe(412)
    expect(error.code).toBe("ontology_precondition_failed")
  })

  it("rejects a stale etag on delete with 412", async () => {
    const api = createApi()
    const resource = await api.getOntology(
      (await findByName(api, "content_ops")).id
    )

    const error = await catchError(
      api.deleteOntology(resource.document.id, '"rev-99"')
    )
    expect(error.status).toBe(412)
    expect(error.code).toBe("ontology_precondition_failed")
  })

  it("deletes an ontology with the current etag", async () => {
    const api = createApi()
    const resource = await api.getOntology(
      (await findByName(api, "blank_canvas")).id
    )

    await api.deleteOntology(resource.document.id, resource.etag)
    const error = await catchError(api.getOntology(resource.document.id))
    expect(error.code).toBe("ontology_not_found")
  })

  it("computes the neighborhood with BFS honoring depth", async () => {
    const api = createApi()
    const document = await findByName(api, "customer_insight")
    const customer = document.object_types.find(
      (objectType) => objectType.name === "customer"
    )

    const depthZero = await api.getObjectTypeNeighborhood(
      document.id,
      customer?.id as string,
      0
    )
    expect(depthZero.object_types.map((item) => item.name)).toEqual([
      "customer",
    ])
    expect(depthZero.link_types).toEqual([])

    const depthOne = await api.getObjectTypeNeighborhood(
      document.id,
      customer?.id as string
    )
    expect(depthOne.depth).toBe(1)
    expect(depthOne.object_types.map((item) => item.name)).toEqual([
      "customer",
      "order",
    ])
    expect(depthOne.link_types.map((item) => item.name)).toEqual([
      "places_order",
    ])
    // 订单有画布位置，邻域结果应带上
    expect(depthOne.canvas.positions).toHaveLength(2)

    const depthTwo = await api.getObjectTypeNeighborhood(
      document.id,
      customer?.id as string,
      2
    )
    expect(depthTwo.object_types.map((item) => item.name)).toEqual([
      "customer",
      "order",
      "product",
    ])
    expect(depthTwo.link_types.map((item) => item.name)).toEqual([
      "places_order",
      "contains_product",
    ])
  })

  it("returns 404 object_type_not_found for an unknown origin", async () => {
    const api = createApi()
    const document = await findByName(api, "customer_insight")

    const error = await catchError(
      api.getObjectTypeNeighborhood(
        document.id,
        "0198f5e9-2eca-7b7c-93d7-b3ba92976384"
      )
    )
    expect(error.status).toBe(404)
    expect(error.code).toBe("object_type_not_found")
  })
})
