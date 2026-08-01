import {
  ChevronDown,
  Crop,
  Download,
  Frame,
  Globe,
  Share2,
  Sparkles,
  Wand2,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import {
  WorkflowBoard,
  type BoardNode,
} from "@/app/(site)/canvas/board"
import {
  GeneratorNodeBody,
  ModelNodeBody,
  NegativePromptBody,
  PositivePromptBody,
  PreviewNodeBody,
} from "@/app/(site)/canvas/nodes"
import {
  FloatingToolbar,
  FloatingToolbarButton,
} from "@/components/stratum/floating-toolbar"
import { PromptInput } from "@/components/stratum/prompt-input"
import { TelemetryReadout } from "@/components/stratum/telemetry-readout"
import type { GraphEdge } from "@/components/stratum/workflow-graph"

/**
 * DIRECTION CONTRACT —— /canvas 工作流编辑器（重设计）
 * THESIS: 画布占满全屏且真实可操作：空白拖拽平移、滚轮/按钮缩放、节点拖拽。
 * OWN-WORLD: 近黑点阵画布；节点为 z 轴双层结构——玻璃背板（半透明 + backdrop-blur
 *   + Generator 的极光透晕）压实心深色面板，端口语义色不变。
 * STORY: 访客一眼读懂 "Black bear" 工作流：Model → Prompt(+/-) → Generator → Preview。
 * FIRST VIEWPORT: 节点图居中铺开，SiteNav 悬浮其上不占位；底部 PromptInput 聚焦时
 *   全周 glow，角落遥测收边。
 * FORM: 整屏无滚动工作台，参考 .impeccable/reference/workflow-editor.png 的节点结构。
 *
 * 节点与连线为数据驱动：NODES 定义节点，EDGES 用 "nodeId" 或 "nodeId:portId"
 * 锚点键描述连接；坐标由 WorkflowGraph 测量推导，交互见 board.tsx。
 */

const WORLD = { width: 1200, height: 600 } as const

const NODES: BoardNode[] = [
  {
    id: "model",
    title: "Model",
    status: "model",
    position: { x: 0, y: 120 },
    body: <ModelNodeBody />,
  },
  {
    id: "positive",
    title: "Positive",
    status: "positive",
    position: { x: 280, y: 28 },
    label: { text: "Prompt", tone: "idle" },
    floatingAction: (
      <Button className="h-6 gap-1 rounded-full px-2.5 text-[0.625rem]">
        <Sparkles aria-hidden />
        Generate
      </Button>
    ),
    body: <PositivePromptBody />,
  },
  {
    id: "negative",
    title: "Negative",
    status: "negative",
    position: { x: 280, y: 300 },
    body: <NegativePromptBody />,
  },
  {
    id: "generator",
    title: "Image Generator",
    status: "image",
    position: { x: 560, y: 110 },
    aurora: true,
    className: "w-60 overflow-hidden",
    body: <GeneratorNodeBody />,
  },
  {
    id: "preview",
    title: "Preview Image",
    status: "image",
    position: { x: 840, y: 28 },
    className: "w-64",
    label: { text: "Preview Image", tone: "image" },
    body: <PreviewNodeBody />,
  },
]

const EDGES: GraphEdge[] = [
  { from: "model:model", to: "generator:model" },
  { from: "positive", to: "generator:positive" },
  { from: "negative", to: "generator:negative" },
  { from: "generator:image", to: "preview:image" },
]

const CURSORS = [
  { name: "Paul", color: "#d9e021", position: { x: 196, y: 282 } },
  { name: "Maria", color: "#b365e0", position: { x: 516, y: 430 } },
  { name: "Kate", color: "#4d9de0", position: { x: 684, y: 246 } },
]

const TELEMETRY = [
  { label: "T", value: "0.00s" },
  { label: "I", value: "0" },
  { label: "N", value: "10 (DI)" },
  { label: "S", value: "60.24" },
]

export default function CanvasPage() {
  return (
    <div className="dark h-svh overflow-hidden bg-background font-sans text-foreground">
      <main className="relative size-full overflow-hidden">
        <WorkflowBoard
          nodes={NODES}
          edges={EDGES}
          cursors={CURSORS}
          world={WORLD}
        />

        {/* 浮动 chrome：SiteNav 之下只有少量悬浮件 */}
        <div className="absolute top-24 right-3 flex items-center gap-2 sm:top-28">
          <Button size="sm" className="gap-1.5 font-sans">
            <Share2 aria-hidden />
            Share
          </Button>
          <Button variant="ghost" size="sm" className="gap-1.5 font-sans">
            <Globe aria-hidden />
            Make Public
          </Button>
        </div>

        <FloatingToolbar
          orientation="horizontal"
          className="absolute top-1/2 left-[calc(50%+240px)] -translate-y-1/2 translate-y-40"
        >
          <FloatingToolbarButton label="重绘">
            <Wand2 aria-hidden />
          </FloatingToolbarButton>
          <FloatingToolbarButton label="取景">
            <Frame aria-hidden />
          </FloatingToolbarButton>
          <FloatingToolbarButton label="裁剪">
            <Crop aria-hidden />
          </FloatingToolbarButton>
          <FloatingToolbarButton label="缩放 2x" className="gap-1">
            2x
            <ChevronDown aria-hidden />
          </FloatingToolbarButton>
          <FloatingToolbarButton label="格式 PNG" className="gap-1">
            PNG
            <ChevronDown aria-hidden />
          </FloatingToolbarButton>
          <FloatingToolbarButton label="下载">
            <Download aria-hidden />
          </FloatingToolbarButton>
        </FloatingToolbar>

        <PromptInput
          className="absolute bottom-6 left-1/2 w-[480px] -translate-x-1/2"
          placeholder="描述想生成的画面…"
        />

        <div className="pointer-events-none absolute inset-x-0 bottom-0 flex items-end justify-between p-3">
          <TelemetryReadout items={TELEMETRY} />
          <TelemetryReadout items={TELEMETRY} className="text-right" />
        </div>
      </main>
    </div>
  )
}
