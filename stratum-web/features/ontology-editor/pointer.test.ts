import { describe, expect, it } from "vitest"

import { parseJsonPointer } from "@/features/ontology-editor/pointer"

describe("parseJsonPointer", () => {
  it("parses the empty pointer to the document root", () => {
    expect(parseJsonPointer("")).toEqual([])
  })

  it("parses single and multi segment pointers", () => {
    expect(parseJsonPointer("/name")).toEqual(["name"])
    expect(parseJsonPointer("/object_types/1/name")).toEqual([
      "object_types",
      "1",
      "name",
    ])
  })

  it("keeps array indexes as string segments", () => {
    expect(parseJsonPointer("/canvas/positions/0/x")).toEqual([
      "canvas",
      "positions",
      "0",
      "x",
    ])
  })

  it("unescapes ~1 as slash and ~0 as tilde", () => {
    expect(parseJsonPointer("/a~1b/c~0d")).toEqual(["a/b", "c~d"])
  })

  it("unescapes combined sequences in the fixed order", () => {
    // RFC 6901 顺序为先 ~1 后 ~0：~01 不含 ~1 子串，~0 还原后得到字面量 ~1
    expect(parseJsonPointer("/~01")).toEqual(["~1"])
    expect(parseJsonPointer("/~00")).toEqual(["~0"])
  })

  it("keeps empty segments", () => {
    expect(parseJsonPointer("/a//b")).toEqual(["a", "", "b"])
    expect(parseJsonPointer("/")).toEqual([""])
  })

  it("rejects pointers not starting with a slash", () => {
    expect(parseJsonPointer("object_types")).toBeNull()
    expect(parseJsonPointer("object_types/0")).toBeNull()
  })

  it("rejects invalid tilde escapes", () => {
    expect(parseJsonPointer("/a~2b")).toBeNull()
    expect(parseJsonPointer("/a~")).toBeNull()
  })
})
