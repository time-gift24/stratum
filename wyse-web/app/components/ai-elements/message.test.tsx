import { renderToStaticMarkup } from "react-dom/server"
import { describe, expect, it } from "vitest"

import {
  Message,
  MessageContent,
  MessageResponse,
} from "~/components/ai-elements/message"

describe("Message", () => {
  it("uses the AI Elements message and streamdown response composition", () => {
    const html = renderToStaticMarkup(
      <Message from="assistant">
        <MessageContent>
          <MessageResponse>**response**</MessageResponse>
        </MessageContent>
      </Message>
    )

    expect(html).toContain("response")
    expect(html).toContain("is-assistant")
  })
})
