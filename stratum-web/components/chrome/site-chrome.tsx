"use client"

import {
  BookOpen,
  Boxes,
  FileText,
  GitCompare,
  Home,
  LifeBuoy,
  PenTool,
  Radio,
  Sparkles,
  Spline,
  Workflow,
  Zap,
} from "lucide-react"

import { SiteNav, type SiteNavMenu } from "@/components/react-bits/site-nav"
import {
  SideDockNav,
  type SideDockNavItem,
} from "@/components/react-bits/side-dock-nav"
import { SHOWCASE_SECTIONS } from "@/components/chrome/showcase-registry"

/**
 * 站点导航外壳（client 组件：图标是函数，不能从 Server Component 传入）。
 * SiteNavChrome —— root 级业务导航，由 (site) 路由组 layout 挂载，fixed 悬浮于所有页面之上。
 * HomeDockNav / CanvasDockNav / MarkdownDockNav —— 各页面场景的左侧 dock，由页面自己挂载。
 */

const SITE_NAV_MENUS: SiteNavMenu[] = [
  {
    label: "组件",
    href: "/",
    items: [
      {
        icon: Boxes,
        title: "WorkflowNode",
        description: "画布节点卡片与语义化端口",
        href: "/#workflow-node",
      },
      {
        icon: Spline,
        title: "CanvasEdge",
        description: "端口之间的贝塞尔连线",
        href: "/#canvas-edge",
      },
      {
        icon: Zap,
        title: "PromptInput",
        description: "电弧边框的药丸提示词输入框",
        href: "/#prompt-input",
      },
      {
        icon: BookOpen,
        title: "MarkdownArticle",
        description: "Medium 风格的文章排版渲染",
        href: "/markdown#markdown-article",
      },
      {
        icon: Radio,
        title: "MarkdownStream",
        description: "AI 流式输出的 Markdown 渲染",
        href: "/markdown#markdown-stream",
      },
      {
        icon: GitCompare,
        title: "MarkdownDiff",
        description: "Markdown 原文对比与渲染对比",
        href: "/markdown#markdown-diff",
      },
    ],
  },
  {
    label: "资源",
    href: "https://ui.shadcn.com",
    items: [
      {
        icon: FileText,
        title: "shadcn/ui",
        description: "官方组件与文档",
        href: "https://ui.shadcn.com",
      },
      {
        icon: Sparkles,
        title: "ReactBits",
        description: "动画组件与区块",
        href: "https://pro.reactbits.dev",
      },
      {
        icon: PenTool,
        title: "Tailwind CSS",
        description: "实用类与 token 体系",
        href: "https://tailwindcss.com",
      },
      {
        icon: LifeBuoy,
        title: "Next.js",
        description: "App Router 文档",
        href: "https://nextjs.org/docs",
      },
    ],
  },
]

export function SiteNavChrome() {
  return (
    <SiteNav
      brand={{ name: "Stratum", href: "/" }}
      menus={SITE_NAV_MENUS}
      links={[
        { label: "画布", href: "/canvas" },
        { label: "Markdown", href: "/markdown" },
        { label: "对话", href: "/conversation" },
      ]}
      cta={{ label: "打开画布", href: "/canvas" }}
    />
  )
}

const HOME_DOCK_ITEMS: SideDockNavItem[] = [
  ...SHOWCASE_SECTIONS.map((s) => ({
    icon: s.icon,
    label: s.title,
    href: `#${s.id}`,
  })),
  { icon: Workflow, label: "画布", href: "/canvas" },
]

export function HomeDockNav() {
  return <SideDockNav items={HOME_DOCK_ITEMS} />
}

const MARKDOWN_DOCK_ITEMS: SideDockNavItem[] = [
  { icon: BookOpen, label: "MarkdownArticle", href: "#markdown-article" },
  { icon: Radio, label: "MarkdownStream", href: "#markdown-stream" },
  { icon: GitCompare, label: "MarkdownDiff", href: "#markdown-diff" },
  { icon: Home, label: "首页", href: "/" },
  { icon: Workflow, label: "画布", href: "/canvas" },
]

export function MarkdownDockNav() {
  return <SideDockNav items={MARKDOWN_DOCK_ITEMS} />
}
