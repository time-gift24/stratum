import { renderToStaticMarkup } from "react-dom/server"
import { describe, expect, it } from "vitest"

import { AiReasoning } from "~/components/ai-elements/reasoning"

describe("AiReasoning", () => {
  it("renders a completed, collapsed reasoning disclosure after a stream ends", () => {
    const html = renderToStaticMarkup(
      <AiReasoning completeLabel="推理完成" thinkingLabel="正在思考">
        Checking the request…
      </AiReasoning>
    )

    expect(html).toContain('data-state="complete"')
    expect(html).toContain("推理完成")
    expect(html).not.toContain("<details open")
  })
})
