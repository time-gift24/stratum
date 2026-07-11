import { renderToStaticMarkup } from "react-dom/server"
import { describe, expect, it } from "vitest"

import {
  PromptInput,
  PromptInputBody,
  PromptInputFooter,
  PromptInputSubmit,
  PromptInputTextarea,
  PromptInputTools,
} from "~/components/ai-elements/prompt-input"

describe("PromptInput", () => {
  it("composes the official prompt body, tools, and submit primitives", () => {
    const html = renderToStaticMarkup(
      <PromptInput onSubmit={() => {}}>
        <PromptInputBody>
          <PromptInputTextarea aria-label="Message" defaultValue="" />
        </PromptInputBody>
        <PromptInputFooter>
          <PromptInputTools>Connected</PromptInputTools>
          <PromptInputSubmit aria-label="发送" disabled />
        </PromptInputFooter>
      </PromptInput>
    )

    expect(html).toContain('data-slot="prompt-input"')
    expect(html).toContain('data-slot="input-group"')
    expect(html).toContain('data-slot="input-group-addon"')
    expect(html).toContain('rows="2"')
    expect(html).toContain("min-h-[4rem]")
    expect(html).not.toContain("min-h-36")
    expect(html).toContain('type="submit"')
    expect(html).toContain('aria-label="发送"')
    expect(html).not.toMatch(/(?:^|\s)border-t(?:\s|")/)
  })
})
