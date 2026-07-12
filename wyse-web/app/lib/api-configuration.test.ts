import { describe, expect, it } from "vitest"

import { apiConfiguration } from "./api-configuration"

describe("apiConfiguration", () => {
  it("uses the local Wyse API without a Vite environment variable", () => {
    expect(apiConfiguration()).toEqual({ baseUrl: "http://127.0.0.1:18080" })
  })
})
