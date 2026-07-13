import { renderToStaticMarkup } from "react-dom/server"
import { I18nextProvider } from "react-i18next"
import { describe, expect, it } from "vitest"

import { createI18n } from "~/lib/i18n"
import { AgentApprovalCard } from "./agent-approval-card"
import { AgentMessageList } from "./agent-message-list"
import { ToolTraceRow } from "./agent-tool-trace"

const i18n = createI18n("en")

function render(component: React.ReactNode) {
  return renderToStaticMarkup(
    <I18nextProvider i18n={i18n}>{component}</I18nextProvider>
  )
}

describe("agent execution UI", () => {
  it("keeps a running tool collapsed with human-readable shimmer copy", () => {
    const html = render(
      <ToolTraceRow
        tool={{
          callId: "tool-1",
          llmCallId: "llm-1",
          name: "search_project_files",
          argumentsText: '{"query":"auth"}',
          result: null,
          errorText: null,
          status: "streaming",
        }}
      />
    )

    expect(html).toContain("Running search project files")
    expect(html).not.toContain("Technical details")
    expect(html).not.toMatch(/<details[^>]* open/)
    expect(html).not.toContain('data-slot="card"')
    expect(html).not.toContain("border-l")
    expect(html).not.toContain(">Why<")
    expect(html).not.toContain("Waiting for the tool to finish.")
  })

  it("presents approval consequences before technical payload", () => {
    const html = render(
      <AgentApprovalCard
        approval={{
          approvalId: "approval-1",
          agentName: "Longzhong",
          callId: "tool-1",
          toolName: "write_file",
          arguments: { file_path: "config/stratum.toml", content: "secret" },
          toolKind: "write",
          dangerLevel: "medium",
        }}
        submittingDecision={null}
        onDecision={() => undefined}
      />
    )

    expect(html).toContain(
      "Allow Longzhong to use write file on config/stratum.toml?"
    )
    expect(html).toContain("Why this is needed")
    expect(html).toContain("What will happen")
    expect(html).toContain("Risk and reversibility")
    expect(html).not.toContain("Technical details")
    expect(html).not.toContain('data-slot="card"')
  })

  it("keeps the tool trace between its call message and final answer", () => {
    const html = render(
      <AgentMessageList
        messages={[
          {
            agentId: "agent-1",
            businessSeq: 1,
            role: "assistant",
            text: "I will check.",
            json: null,
            reasoning: "I should inspect the project first.",
            toolCalls: [{ callId: "tool-1", name: "echo", arguments: {} }],
            timestamp: "2026-07-13T00:00:01Z",
          },
          {
            agentId: "agent-1",
            businessSeq: 3,
            role: "assistant",
            text: "The check is complete.",
            json: null,
            reasoning: null,
            toolCalls: [],
            timestamp: "2026-07-13T00:00:03Z",
          },
        ]}
        drafts={{}}
        tools={{
          "tool-1": {
            callId: "tool-1",
            llmCallId: "turn:turn-1",
            name: "echo",
            argumentsText: "{}",
            result: { output: "tool-result-only" },
            errorText: null,
            status: "finished",
          },
        }}
        approvals={{}}
        approvalSubmissions={new Map()}
        onApprovalDecision={() => undefined}
      />
    )

    expect(html.indexOf("I will check.")).toBeLessThan(
      html.indexOf("echo completed")
    )
    expect(html.indexOf("echo completed")).toBeLessThan(
      html.indexOf("The check is complete.")
    )
    expect(html.match(/tool-result-only/g)).toHaveLength(1)
    expect(html.match(/data-slot="agent-disclosure-trigger"/g)).toHaveLength(2)
  })

  it("does not append a completed unassociated tool to the conversation end", () => {
    const html = render(
      <AgentMessageList
        messages={[
          {
            agentId: "agent-1",
            businessSeq: 1,
            role: "assistant",
            text: "The final answer.",
            json: null,
            reasoning: null,
            toolCalls: [],
            timestamp: "2026-07-13T00:00:01Z",
          },
        ]}
        drafts={{}}
        tools={{
          "tool-1": {
            callId: "tool-1",
            llmCallId: "llm-missing",
            name: "echo",
            argumentsText: "{}",
            result: {},
            errorText: null,
            status: "finished",
          },
        }}
        approvals={{}}
        approvalSubmissions={new Map()}
        onApprovalDecision={() => undefined}
      />
    )

    expect(html).not.toContain("echo completed")
  })
})
