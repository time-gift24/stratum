import { describe, expect, it } from "vitest"

import {
  approvalResource,
  humanizeToolName,
} from "./agent-execution-presentation"

describe("agent execution presentation", () => {
  it("turns internal tool identifiers into readable actions", () => {
    expect(humanizeToolName("search_projectFiles", "Unknown tool")).toBe(
      "search project files"
    )
    expect(humanizeToolName(null, "Unknown tool")).toBe("Unknown tool")
  })

  it("extracts only an explicit file resource for approval copy", () => {
    expect(approvalResource({ file_path: "config/stratum.toml" })).toBe(
      "config/stratum.toml"
    )
    expect(
      approvalResource({ api_key: "secret", command: "deploy" })
    ).toBeNull()
  })
})
