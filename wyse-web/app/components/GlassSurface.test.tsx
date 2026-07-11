import { renderToStaticMarkup } from "react-dom/server"
import { describe, expect, it } from "vitest"

import GlassSurface from "~/components/GlassSurface"

describe("GlassSurface", () => {
  it("exposes displace to the final backdrop layer", () => {
    const html = renderToStaticMarkup(
      <GlassSurface displace={2.2} width="100%" height="100%" />
    )

    expect(html).toContain("--glass-displace:2.2px")
  })
})
