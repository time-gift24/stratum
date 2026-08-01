import {
  Activity,
  Boxes,
  MousePointer2,
  PanelTop,
  Sparkles,
  Spline,
  Terminal,
  Wrench,
  Zap,
} from "lucide-react"

/**
 * 展示页 section 注册表（单一数据源）。
 * 首页每个 ShowcaseSection 的 id/title 与左侧 dock 的锚点项都由这里派生：
 * 新增组件 = 在这里加一条 + 页面写一个 ShowcaseSection（id 与之对应）。
 * dock 项由 site-chrome 从本表 map 生成，不再两处手维护。
 */
export const SHOWCASE_SECTIONS = [
  { id: "workflow-node", title: "WorkflowNode", icon: Boxes },
  { id: "canvas-edge", title: "CanvasEdge", icon: Spline },
  { id: "cursor-presence", title: "CursorPresence", icon: MousePointer2 },
  { id: "editor-top-bar", title: "EditorTopBar", icon: PanelTop },
  { id: "floating-toolbar", title: "FloatingToolbar", icon: Wrench },
  { id: "prompt-bar", title: "PromptBar", icon: Terminal },
  { id: "prompt-input", title: "PromptInput", icon: Zap },
  { id: "border-glow", title: "BorderGlow", icon: Sparkles },
  { id: "telemetry-readout", title: "TelemetryReadout", icon: Activity },
] as const

export type ShowcaseSectionId = (typeof SHOWCASE_SECTIONS)[number]["id"]

export function showcaseMeta(id: ShowcaseSectionId) {
  const meta = SHOWCASE_SECTIONS.find((s) => s.id === id)
  if (!meta) throw new Error(`showcase section 未注册: ${id}`)
  return meta
}
