import { describe, expect, it } from "vitest"

import { nodeHue } from "./hue"

describe("nodeHue", () => {
  it("同一 ID 多次计算结果稳定", () => {
    expect(nodeHue("customer")).toBe(nodeHue("customer"))
  })

  it("结果始终落在 0–359 色环内", () => {
    const ids = ["customer", "order", "product", "", "a", "客户", "0198f5e9-2eca-7b7c-93d7-b3ba92976384"]
    for (const id of ids) {
      const hue = nodeHue(id)
      expect(hue).toBeGreaterThanOrEqual(0)
      expect(hue).toBeLessThan(360)
    }
  })

  it("不同 ID 的色相自然错开", () => {
    const hues = new Set(
      ["customer", "order", "product", "invoice", "shipment", "user"].map(
        nodeHue
      )
    )
    expect(hues.size).toBeGreaterThan(3)
  })

  it("共享 UUID 前缀、仅尾部不同的 ID 也不会色相簇拥", () => {
    const ids = [
      "0198f5e9-2eca-7b7c-93d7-b3ba92976384",
      "0198f5e9-2eca-7b7c-93d7-b3ba92976385",
      "0198f5e9-2eca-7b7c-93d7-b3ba92976386",
      "0198f5e9-2eca-7b7c-93d7-b3ba92976387",
    ]
    const hues = ids.map(nodeHue)
    expect(new Set(hues).size).toBe(ids.length)
    const sorted = [...hues].sort((a, b) => a - b)
    for (let index = 1; index < sorted.length; index += 1) {
      expect(sorted[index] - sorted[index - 1]).toBeGreaterThan(30)
    }
  })
})
