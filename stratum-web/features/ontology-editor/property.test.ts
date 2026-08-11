import { describe, expect, it } from "vitest"

import type { OntologyProperty } from "./types"
import {
  nextPropertyName,
  validatePropertyDisplayName,
  validatePropertyName,
} from "./property"

function property(name: string): OntologyProperty {
  return {
    id: `id-${name}`,
    name,
    display_name: name,
    value_type: "string",
    required: false,
  }
}

describe("validatePropertyName", () => {
  it("合法 name 返回 null", () => {
    expect(validatePropertyName("order_no")).toBeNull()
  })

  it("非法 name 返回提示文案", () => {
    expect(validatePropertyName("Order")).toContain("^[a-z]")
    expect(validatePropertyName("")).toContain("^[a-z]")
  })

  it.each([
    ["大写开头", "Order_no"],
    ["数字开头", "1st"],
    ["含连字符", "order-no"],
    ["含空格", "order no"],
    ["超过 64 字符", `a${"b".repeat(64)}`],
  ])("参数化非法用例：%s", (_label, name) => {
    expect(validatePropertyName(name)).not.toBeNull()
  })
})

describe("validatePropertyDisplayName", () => {
  it("非空返回 null，空白返回提示", () => {
    expect(validatePropertyDisplayName("订单编号")).toBeNull()
    expect(validatePropertyDisplayName("   ")).toBe("显示名不能为空")
  })
})

describe("nextPropertyName", () => {
  it("空列表从 field_1 开始", () => {
    expect(nextPropertyName([])).toBe("field_1")
  })

  it("从 properties.length + 1 起取不冲突的序号", () => {
    expect(
      nextPropertyName([property("field_1"), property("field_2")])
    ).toBe("field_3")
    expect(nextPropertyName([property("field_2")])).toBe("field_3")
    expect(
      nextPropertyName([property("name"), property("field_4")])
    ).toBe("field_3")
  })
})
