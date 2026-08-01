import {
  Crosshair,
  Layers,
  LayoutGrid,
  Minus,
  MousePointer2,
  Play,
  Plus,
  SlidersHorizontal,
  Sparkles,
  Square,
  Expand,
  Hexagon,
  Image as ImageIcon,
  Bot,
} from "lucide-react"

import { Button } from "@/components/ui/button"
import { HomeDockNav } from "@/components/chrome/site-chrome"
import {
  showcaseMeta,
  type ShowcaseSectionId,
} from "@/components/chrome/showcase-registry"
import { BorderGlow } from "@/components/react-bits/border-glow"
import { CanvasEdge, CanvasEdges } from "@/components/stratum/canvas-edge"
import { CursorPresence } from "@/components/stratum/cursor-presence"
import {
  EditorTopBar,
  EditorTopBarGroup,
  EditorTopBarTitle,
} from "@/components/stratum/editor-top-bar"
import {
  FloatingToolbar,
  FloatingToolbarButton,
} from "@/components/stratum/floating-toolbar"
import { NodeCanvas } from "@/components/stratum/node-canvas"
import { PromptBar } from "@/components/stratum/prompt-bar"
import { PromptInput } from "@/components/stratum/prompt-input"
import {
  ShowcaseDemo,
  ShowcaseSection,
} from "@/components/stratum/showcase/showcase-section"
import { ScrollReveal } from "@/components/stratum/showcase/scroll-reveal"
import { TelemetryReadout } from "@/components/stratum/telemetry-readout"
import {
  NodePort,
  WorkflowNode,
  WorkflowNodeLabel,
} from "@/components/stratum/workflow-node"

/**
 * Stratum 组件展示页。
 * 新增组件流程：在 components/chrome/showcase-registry.ts 注册一条
 * （id + title + 图标，dock 项自动派生），再追加一个 ShowcaseSection（含 demo）。
 * canvas 世界的组件用 <ShowcaseDemo dark> 固定暗色呈现；
 * fixed 定位的组件在 demo 面板用 transform 建立包含块。
 */

/** section 的 id/title 统一从注册表取（防与 dock 漂移） */
const sec = (id: ShowcaseSectionId) => {
  const { id: sectionId, title } = showcaseMeta(id)
  return { id: sectionId, title }
}

export default function Page() {
  return (
    <div className="min-h-svh pt-28 font-sans sm:pt-32">
      <HomeDockNav />

      <div className="mx-auto max-w-6xl px-6 pt-2 pb-10 md:pl-24 xl:pl-6">
        <main className="grid min-w-0 grid-cols-1 gap-12 md:grid-cols-2 md:gap-x-8">
          <ScrollReveal className="md:col-span-2">
            <header className="flex flex-col gap-3">
              <h1 className="font-heading text-3xl tracking-tight">Stratum</h1>
              <p className="max-w-prose text-sm leading-relaxed text-muted-foreground">
                内部组件库：基于 shadcn 官方组件组合，只消费全局 token。
                核心展示物是节点式工作流编辑器画布。
              </p>
            </header>
          </ScrollReveal>

          <ShowcaseSection
            {...sec("workflow-node")}
            description="画布节点卡片：状态点 + 标题 + 收起 chevron，正文组合端口列表与控件。端口颜色表达数据类型语义，标签悬浮于节点上方。"
            className="md:col-span-2"
          >
            <ShowcaseDemo dark scale={1.25} className="p-0">
              <NodeCanvas className="h-72 w-full">
                <WorkflowNodeLabel
                  tone="idle"
                  className="absolute top-6 left-10"
                >
                  Prompt
                </WorkflowNodeLabel>
                <WorkflowNode
                  title="Model"
                  status="model"
                  className="absolute top-16 left-10"
                >
                  <div className="flex flex-col items-end gap-1">
                    <NodePort tone="model" align="end">
                      model
                    </NodePort>
                    <NodePort tone="positive" align="end">
                      positive
                    </NodePort>
                    <NodePort tone="negative" align="end">
                      negative
                    </NodePort>
                  </div>
                </WorkflowNode>
                <WorkflowNode
                  title="Image Generator"
                  status="image"
                  className="absolute top-12 right-10 w-64"
                  action={
                    <Button
                      size="xs"
                      className="rounded-full bg-primary text-primary-foreground hover:bg-primary/90"
                    >
                      <Sparkles aria-hidden />
                      Generate
                    </Button>
                  }
                >
                  <div className="flex flex-col gap-1">
                    <NodePort tone="model">model</NodePort>
                    <NodePort tone="positive">positive</NodePort>
                    <NodePort tone="negative">negative</NodePort>
                  </div>
                </WorkflowNode>
              </NodeCanvas>
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            {...sec("canvas-edge")}
            description="两端口间的水平贝塞尔连线：1.5px、--edge 色、半透明，无箭头无动画。坐标系与画布世界一致。"
            className="md:col-span-2"
          >
            <ShowcaseDemo dark scale={1.25} className="p-0">
              <NodeCanvas className="h-48 w-full">
                <CanvasEdges width={800} height={192}>
                  <CanvasEdge from={{ x: 60, y: 60 }} to={{ x: 400, y: 120 }} />
                  <CanvasEdge from={{ x: 60, y: 140 }} to={{ x: 400, y: 80 }} />
                </CanvasEdges>
              </NodeCanvas>
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            {...sec("cursor-presence")}
            description="协作光标：彩色箭头指针 + 同色名牌。颜色代表协作者身份，由调用方传入。"
          >
            <ShowcaseDemo dark scale={1.4} className="p-0">
              <NodeCanvas className="h-40 w-full">
                <CursorPresence
                  name="Paul"
                  color="#d9e021"
                  className="top-10 left-1/4"
                />
                <CursorPresence
                  name="Maria"
                  color="#b365e0"
                  className="top-20 left-1/2"
                />
                <CursorPresence
                  name="Kate"
                  color="#4d9de0"
                  className="top-8 left-3/4"
                />
              </NodeCanvas>
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            {...sec("editor-top-bar")}
            description="编辑器顶栏：三段式（左组 / 中央标题导航 / 右组），内容由调用方组合。"
            className="md:col-span-2"
          >
            <ShowcaseDemo dark scale={1.2} className="p-6">
              <EditorTopBar className="w-full">
                <EditorTopBarGroup>
                  <Button variant="secondary" size="sm">
                    Workflow
                  </Button>
                  <Button variant="ghost" size="sm">
                    Edit
                  </Button>
                </EditorTopBarGroup>
                <EditorTopBarTitle title="Black bear" />
                <EditorTopBarGroup>
                  <FloatingToolbarButton
                    label="运行"
                    className="rounded-md p-1.5"
                  >
                    <Play aria-hidden />
                  </FloatingToolbarButton>
                  <FloatingToolbarButton
                    label="设置"
                    className="rounded-md p-1.5"
                  >
                    <SlidersHorizontal aria-hidden />
                  </FloatingToolbarButton>
                </EditorTopBarGroup>
              </EditorTopBar>
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            {...sec("floating-toolbar")}
            description="悬浮图标工具条：vertical 用于画布边缘，horizontal 用于预览操作行。"
          >
            <ShowcaseDemo dark scale={1.4} className="gap-6">
              <FloatingToolbar orientation="vertical">
                <FloatingToolbarButton label="全屏">
                  <Expand aria-hidden />
                </FloatingToolbarButton>
                <FloatingToolbarButton label="缩小">
                  <Minus aria-hidden />
                </FloatingToolbarButton>
                <FloatingToolbarButton label="放大" active>
                  <Plus aria-hidden />
                </FloatingToolbarButton>
                <FloatingToolbarButton label="布局">
                  <LayoutGrid aria-hidden />
                </FloatingToolbarButton>
                <FloatingToolbarButton label="聚焦">
                  <Crosshair aria-hidden />
                </FloatingToolbarButton>
                <FloatingToolbarButton label="图层">
                  <Layers aria-hidden />
                </FloatingToolbarButton>
              </FloatingToolbar>
              <FloatingToolbar orientation="horizontal">
                <FloatingToolbarButton label="指针">
                  <MousePointer2 aria-hidden />
                </FloatingToolbarButton>
                <FloatingToolbarButton label="画幅" active>
                  <Square aria-hidden />
                </FloatingToolbarButton>
                <FloatingToolbarButton label="模型设置">
                  <Hexagon aria-hidden />
                </FloatingToolbarButton>
              </FloatingToolbar>
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            {...sec("prompt-bar")}
            description="画布底部的提示词输入条：展示文本 + 动作行，左侧可挂主入口按钮。"
          >
            <ShowcaseDemo dark scale={1.25}>
              <PromptBar
                label="Prompt"
                value="Minimalist illustration of a black bear with a pink snout, soft gradients, and smooth shapes"
                leading={
                  <Button
                    size="icon-lg"
                    className="rounded-xl"
                    aria-label="AI 助手"
                  >
                    <Bot aria-hidden />
                  </Button>
                }
                actions={
                  <>
                    <FloatingToolbarButton label="添加图片">
                      <ImageIcon aria-hidden />
                    </FloatingToolbarButton>
                    <FloatingToolbarButton label="参数">
                      <SlidersHorizontal aria-hidden />
                    </FloatingToolbarButton>
                  </>
                }
              />
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            {...sec("prompt-input")}
            description="Gemini 式药丸提示词输入框：聚焦时 BorderGlow 全线段点亮——整圈 mesh 渐变边框 + 全周外发光（port/primary token 配色），失焦淡出；空输入禁用发送，Enter 提交。"
          >
            <ShowcaseDemo scale={1.15} className="py-16">
              <PromptInput />
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            {...sec("border-glow")}
            description="追光渐变边框卡片：指针接近边缘时，mesh 彩色边框与外发光随角度与接近度浮现；挂载可播一圈扫光。GSAP 驱动，配色取自 port/primary token 家族。"
          >
            <ShowcaseDemo dark className="py-14">
              <BorderGlow animated className="w-full max-w-sm">
                <div className="p-6">
                  <p className="text-sm font-medium">Glow Card</p>
                  <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                    指针靠近边缘，光随其至。
                  </p>
                </div>
              </BorderGlow>
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            {...sec("telemetry-readout")}
            description="画布角落的遥测读数：等宽小字逐行排列。"
          >
            <ShowcaseDemo dark scale={1.4} className="justify-around">
              <TelemetryReadout
                items={[
                  { label: "T", value: "0.00s" },
                  { label: "I", value: "0" },
                  { label: "N", value: "10 (DI)" },
                  { label: "S", value: "60.24" },
                ]}
              />
              <TelemetryReadout
                className="text-right"
                items={[
                  { label: "FPS", value: "60" },
                  { label: "Nodes", value: "4" },
                ]}
              />
            </ShowcaseDemo>
          </ShowcaseSection>
        </main>
      </div>
    </div>
  )
}
