import { describe, expect, it } from "vitest"

import type { StreamEnvelope } from "~/lib/wyse-api"
import { conversationReducer, initialConversationState } from "./reducer"

function messageEnvelope(
  businessSeq: number,
  message: Extract<
    StreamEnvelope["event"]["data"]["event"],
    { type: "message" }
  >["data"]["message"]
): StreamEnvelope {
  return {
    business_seq: businessSeq,
    run_id: "run-1",
    timestamp: `2026-07-13T00:00:0${businessSeq}Z`,
    event: {
      type: "agent",
      data: {
        agent_id: "agent-1",
        event: {
          type: "message",
          data: { turn_id: "turn-1", message },
        },
      },
    },
  }
}

describe("conversationReducer tool traces", () => {
  it("keeps the LLM call association for chronological rendering", () => {
    const envelope: StreamEnvelope = {
      run_id: "run-1",
      timestamp: "2026-07-13T00:00:00Z",
      event: {
        type: "agent",
        data: {
          agent_id: "agent-1",
          event: {
            type: "llm",
            data: {
              llm_call_id: "llm-1",
              event: {
                type: "tool_call_started",
                data: { call_id: "tool-1", name: "search_project_files" },
              },
            },
          },
        },
      },
    }

    const state = conversationReducer(
      { ...initialConversationState, agentId: "agent-1" },
      { type: "envelope_received", envelope }
    )

    expect(state.tools["tool-1"]?.llmCallId).toBe("llm-1")
  })

  it("rebuilds a completed tool trace from persisted messages", () => {
    const assistantToolCall = messageEnvelope(1, {
      role: "assistant",
      content: { type: "text", data: "I will check." },
      tool_calls: [
        {
          call_id: "tool-1",
          name: "echo",
          arguments: { value: "hello" },
        },
      ],
    })
    const toolResult = messageEnvelope(2, {
      role: "tool",
      content: {
        type: "json",
        data: { output: "tool-result-only" },
      },
      tool_call_id: "tool-1",
    })
    const finalAnswer = messageEnvelope(3, {
      role: "assistant",
      content: { type: "text", data: "The check is complete." },
    })

    const state = conversationReducer(
      { ...initialConversationState, agentId: "agent-1" },
      {
        type: "history_loaded",
        events: [assistantToolCall, toolResult, finalAnswer],
      }
    )

    expect(state.messages.map((message) => message.role)).toEqual([
      "assistant",
      "assistant",
    ])
    expect(state.tools["tool-1"]).toMatchObject({
      name: "echo",
      argumentsText: '{"value":"hello"}',
      result: { output: "tool-result-only" },
      status: "finished",
    })
  })
})
