import { describe, expect, it } from "vitest"

import {
  resolveMessageIntent,
  type PendingMessageIntent,
} from "@/features/agent-conversation/message-intent"

function intent(
  overrides: Partial<PendingMessageIntent> = {}
): PendingMessageIntent {
  return {
    agentRuntimeId: "runtime-1",
    text: "original text",
    expectedCurrentTurnId: "turn-before",
    modelConfig: {
      model: "anthropic:claude-sonnet",
      parameters: { thinking: "low" },
    },
    ...overrides,
  }
}

describe("message intent CAS retention", () => {
  it("reuses the original CAS when an ambiguous response is retried", () => {
    const pending = intent({ expectedCurrentTurnId: null })
    const latestViewWouldOpenAnotherTurn = intent({
      expectedCurrentTurnId: "turn-created-by-ambiguous-request",
    })

    const retry = resolveMessageIntent(pending, latestViewWouldOpenAnotherTurn)

    expect(retry).toBe(pending)
    expect(retry.expectedCurrentTurnId).toBeNull()
  })

  it("treats changed raw text as an explicit new intent", () => {
    const pending = intent()
    const next = intent({
      text: "new text",
      expectedCurrentTurnId: "latest-turn",
    })

    expect(resolveMessageIntent(pending, next)).toBe(next)
  })

  it("treats a changed full model replacement as an explicit new intent", () => {
    const pending = intent()
    const next = intent({
      expectedCurrentTurnId: "latest-turn",
      modelConfig: {
        model: "openai:gpt-5",
        parameters: { reasoning: "high" },
      },
    })

    expect(resolveMessageIntent(pending, next)).toBe(next)
  })

  it("never carries a pending CAS across AgentRuntimes", () => {
    const pending = intent()
    const next = intent({
      agentRuntimeId: "runtime-2",
      expectedCurrentTurnId: "agent-2-current-turn",
    })

    expect(resolveMessageIntent(pending, next)).toBe(next)
  })
})
