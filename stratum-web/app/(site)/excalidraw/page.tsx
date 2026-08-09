import type { Metadata } from "next"

import { WhiteboardWorkspace } from "@/components/stratum/excalidraw/whiteboard-workspace"

export const metadata: Metadata = {
  title: "Excalidraw - Stratum",
}

/** Excalidraw 页：整屏编辑器满铺（沉浸模式，导航由 SiteNavChrome 收起，顶部无避让） */
export default function WhiteboardPage() {
  return (
    <div className="h-svh font-sans">
      <WhiteboardWorkspace />
    </div>
  )
}
