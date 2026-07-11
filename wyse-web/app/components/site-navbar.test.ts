import { readFileSync } from "node:fs"
import { fileURLToPath } from "node:url"
import { describe, expect, it } from "vitest"

const source = readFileSync(
  fileURLToPath(new URL("./site-navbar.tsx", import.meta.url)),
  "utf8"
)

describe("SiteNavbar section navigation", () => {
  it("does not retain the removed 80px Longzhong anchor offset", () => {
    expect(source).not.toMatch(/window\.scrollY\s*-\s*scrollOffset/)
  })
})
