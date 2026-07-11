import { renderToStaticMarkup } from "react-dom/server"
import { describe, expect, it } from "vitest"

import {
  Reasoning,
  ReasoningContent,
  ReasoningTrigger,
} from "~/components/ai-elements/reasoning"

describe("Reasoning", () => {
  it("uses the AI Elements trigger and content composition while streaming", () => {
    const html = renderToStaticMarkup(
      <Reasoning isStreaming>
        <ReasoningTrigger getThinkingMessage={() => "正在思考"} />
        <ReasoningContent>Checking the request…</ReasoningContent>
      </Reasoning>
    )

    expect(html).toContain('data-slot="collapsible"')
    expect(html).toContain("正在思考")
    expect(html).toContain("Checking the request…")
  })
})
