import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
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

test("gets locale option labels from translation keys", async () => {
  const toggleUrl = new URL("../components/locale-toggle.tsx", import.meta.url)
  const toggle = await readFile(toggleUrl, "utf8")

  assert.match(toggle, /\{t\("locale\.option\.zh"\)\}/)
  assert.match(toggle, /\{t\("locale\.option\.en"\)\}/)
})

test("localizes the theme switch label through the typed locale dictionary", async () => {
  const [themeToggle, locale] = await Promise.all([
    readFile(
      new URL("../components/theme-toggle.tsx", import.meta.url),
      "utf8"
    ),
    readFile(new URL("./locale.ts", import.meta.url), "utf8"),
  ])

  assert.match(
    themeToggle,
    /import \{ useLocale \} from "~\/components\/locale-provider"/
  )
  assert.match(themeToggle, /const \{ t \} = useLocale\(\)/)
  assert.match(themeToggle, /aria-label=\{t\("theme\.toggle"\)\}/)
  assert.match(locale, /"theme\.toggle": "切换深色模式"/)
  assert.match(locale, /"theme\.toggle": "Toggle dark theme"/)
})
