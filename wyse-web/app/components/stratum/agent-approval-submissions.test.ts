import { describe, expect, it } from "vitest"

import {
  finishApprovalSubmission,
  startApprovalSubmission,
} from "./agent-approval-submissions"

describe("approval submission state", () => {
  it("remembers which decision is being submitted", () => {
    const submissions = startApprovalSubmission(
      new Map(),
      "approval-1",
      "approve"
    )

    expect(submissions.get("approval-1")).toBe("approve")
  })

  it("removes only the finished approval", () => {
    const submissions = new Map([
      ["approval-1", "approve" as const],
      ["approval-2", "reject" as const],
    ])

    expect(finishApprovalSubmission(submissions, "approval-1")).toEqual(
      new Map([["approval-2", "reject"]])
    )
  })
})
