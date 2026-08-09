import { describe, expect, it } from "vitest"

import { createUuidV7 } from "@/features/ontology-editor/ids"

const UUID_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/

describe("createUuidV7", () => {
  it("generates well-formed lowercase UUID strings", () => {
    for (let index = 0; index < 100; index += 1) {
      expect(createUuidV7()).toMatch(UUID_PATTERN)
    }
  })

  it("sets version 7 and variant 10 bits", () => {
    for (let index = 0; index < 100; index += 1) {
      const uuid = createUuidV7()
      expect(uuid[14]).toBe("7")
      expect(["8", "9", "a", "b"]).toContain(uuid[19])
    }
  })

  it("encodes the millisecond timestamp in the leading 48 bits", () => {
    const now = 1_754_637_000_123
    const uuid = createUuidV7(now)
    const encoded = BigInt(`0x${uuid.replaceAll("-", "").slice(0, 12)}`)
    expect(encoded).toBe(BigInt(now))
  })

  it("orders lexicographically by timestamp prefix", () => {
    const earlier = createUuidV7(1_700_000_000_000)
    const later = createUuidV7(1_800_000_000_000)
    expect(earlier.slice(0, 13) < later.slice(0, 13)).toBe(true)
  })

  it("draws unique values over many generations", () => {
    const seen = new Set<string>()
    for (let index = 0; index < 10_000; index += 1) {
      seen.add(createUuidV7())
    }
    expect(seen.size).toBe(10_000)
  })
})
