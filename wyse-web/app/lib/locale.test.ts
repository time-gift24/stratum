import assert from "node:assert/strict"
import test from "node:test"

import { LOCALE_STORAGE_KEY, resolveLocale } from "./locale"

test("uses a valid stored locale before the system language", () => {
  assert.equal(resolveLocale("en", "zh-CN"), "en")
})

test("uses English for an English system language", () => {
  assert.equal(resolveLocale(null, "en-GB"), "en")
})

test("uses Chinese for Chinese and unsupported system languages", () => {
  assert.equal(resolveLocale(null, "zh-TW"), "zh")
  assert.equal(resolveLocale(null, "ja-JP"), "zh")
  assert.equal(resolveLocale("fr", "en-US"), "en")
})

test("keeps the locale key stable for persisted manual choices", () => {
  assert.equal(LOCALE_STORAGE_KEY, "wyse-locale")
})
