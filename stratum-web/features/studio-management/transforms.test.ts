import { describe, expect, it } from "vitest"

import {
  agentDraftToInput,
  agentVersionValidationMessage,
  agentViewToDraft,
  encodeAgentToml,
  parseAgentToml,
} from "@/features/studio-management/transforms"
import type { AgentDraft } from "@/features/studio-management/types"
import type { AgentDefinitionView } from "@/lib/stratum/api"

const VIEW: AgentDefinitionView = {
  agent_name: "researcher",
  agent_version: "release-7",
  model: "openai:gpt-5",
  model_parameters: { reasoning_effort: "high" },
  tools: ["web_search"],
  prompt: "Research carefully.",
  updated_at: "2026-08-19T00:00:00Z",
}

describe("Studio Agent transforms", () => {
  it("preserves the author version through view, draft, and save input", () => {
    const draft = agentViewToDraft(VIEW)

    expect(draft.agentVersion).toBe("release-7")
    expect(
      agentDraftToInput({ ...draft, agentVersion: " release-8 " })
    ).toEqual({
      agent_name: "researcher",
      agent_version: " release-8 ",
      model: "openai:gpt-5",
      model_parameters: { reasoning_effort: "high" },
      tools: ["web_search"],
      prompt: "Research carefully.",
    })
  })

  it("preserves authored prompt whitespace in the save payload", () => {
    const draft = agentViewToDraft(VIEW)
    const prompt = "\n  indented Markdown\n"

    expect(agentDraftToInput({ ...draft, prompt }).prompt).toBe(prompt)
  })

  it("does not silently normalize author identities", () => {
    const draft = agentViewToDraft(VIEW)

    expect(
      agentDraftToInput({
        ...draft,
        agentName: " researcher ",
        tools: [" echo "],
      })
    ).toMatchObject({
      agent_name: " researcher ",
      tools: [" echo "],
    })
  })

  it("preserves exact version tags and rejects non-canonical boundaries", () => {
    expect(agentVersionValidationMessage("release-8")).toBeNull()
    expect(agentVersionValidationMessage(" release-8 ")).toBe(
      "版本标签首尾不能有空白"
    )
    expect(agentVersionValidationMessage("release\n8")).toBe(
      "版本标签不能包含控制字符"
    )
    expect(agentVersionValidationMessage("测".repeat(43))).toBe(
      "版本标签不能超过 128 字节"
    )
  })

  it("round-trips agent_version in canonical TOML", () => {
    const draft: AgentDraft = agentViewToDraft(VIEW)
    const source = encodeAgentToml(draft)

    expect(source).toContain('agent_version = "release-7"')
    expect(parseAgentToml(source)).toEqual({
      ok: true,
      draft: { ...draft, agentName: "" },
    })
  })

  it("rejects raw TOML that omits the required agent_version", () => {
    const parsed = parseAgentToml(`
model = "openai:gpt-5"
tools = []
prompt = "Research carefully."
`)

    expect(parsed.ok).toBe(false)
  })

  it.each([
    [
      'agent_version = " v2 "\nmodel = "openai:gpt-5"\ntools = []\nprompt = "ok"',
      "版本标签",
    ],
    ['agent_version = "v2"\nmodel = ""\ntools = []\nprompt = "ok"', "Model"],
    [
      'agent_version = "v2"\nmodel = "openai:gpt-5"\ntools = []\nprompt = "   "',
      "prompt",
    ],
    [
      'agent_version = "v2"\nmodel = "openai:gpt-5"\ntools = ["echo", "echo"]\nprompt = "ok"',
      "重复",
    ],
  ])(
    "rejects invalid raw Agent values before draft sync",
    (source, message) => {
      const parsed = parseAgentToml(source)

      expect(parsed.ok).toBe(false)
      if (!parsed.ok) expect(parsed.message).toContain(message)
    }
  )

  it("rejects an oversized UTF-8 raw version tag", () => {
    const parsed = parseAgentToml(
      `agent_version = "${"测".repeat(43)}"\nmodel = "openai:gpt-5"\ntools = []\nprompt = "ok"`
    )

    expect(parsed.ok).toBe(false)
    if (!parsed.ok) expect(parsed.message).toContain("128 字节")
  })
})
