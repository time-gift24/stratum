import type { Metadata } from "next"

import { WhiteboardWorkspace } from "@/components/stratum/excalidraw/whiteboard-workspace"

export const metadata: Metadata = {
  title: "Excalidraw - Stratum",
}

/** Excalidraw 页：整屏编辑器满铺（h-svh；导航常开悬浮，画布顶部不做避让，
 *  工具栏经 excalidraw-theme 的 verticalTools 竖置左缘）。 */
export default function WhiteboardPage() {
  return (
    <div className="h-svh font-sans">
      <WhiteboardWorkspace />
    </div>
  )
}
