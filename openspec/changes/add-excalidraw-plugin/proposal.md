# Proposal: add-excalidraw-plugin

## Why

Agent 未来会通过工具产出 Excalidraw 图示（scene JSON），但当前前端把所有工具结果一律渲染为 `<pre>` 文本，图示无法直观呈现。本 change 先把最基础的前端部分整合进来：在对话的工具调用块中，把 Excalidraw scene 结果渲染为可缩放的只读白板，为后续后端工具接入铺好渲染通路。

## What Changes

- 前端新增依赖 `@excalidraw/excalidraw`，**动态导入**（`next/dynamic`，SSR 关闭），不拖慢 `/chat` 首屏。
- 新增组件 `components/stratum/conversation/excalidraw-result.tsx`：把 Excalidraw scene JSON 渲染为只读（view mode）白板卡片；固定高度、内嵌在工具调用块内；随 next-themes 主题切换 dark/light；加载中显示与最终形状一致的骨架，解析失败回退到现有原始 JSON 展示。
- `tool-call.tsx` 增加按工具名分发的渲染分支：`call.name === "excalidraw_render"` 且结果通过最小 scene 形状校验时，结果区渲染白板组件；其余工具渲染路径完全不变。
- 设计遵循 PRODUCT.md「渐进式透明」：白板收在工具调用块内部，块默认折叠的现状不变；视觉只消费 `app/globals.css` 语义 token，GSAP/动效体系不新增。
- 新增独立白板页 `/excalidraw`（应用户明确要求）：完整可编辑的 Excalidraw 编辑器，动态加载、主题跟随；SiteNav 新增「Excalidraw」入口，路由登记进 `PAGE_ORDER`，与 `/conversation` 间跳转带方向性左右转场。该页是独立路由，不把画布元素引入对话页面（PRODUCT.md 的对话页约束不受影响）。
- 非目标：后端 Rust 工具实现与注册（后续 change）；白板内容持久化与回写；协作。
- 无既有 change 被取代；无 BREAKING 变更。

## Capabilities

### New Capabilities

- `excalidraw-whiteboard`: 对话界面把 Excalidraw scene 工具结果内嵌渲染为只读白板的能力，覆盖按工具名的渲染分发、scene 形状校验与回退、主题适配和加载/错误状态；另含独立白板页 `/excalidraw`（可编辑编辑器 + SiteNav 入口 + 方向性转场）。

### Modified Capabilities

（无——本 change 只新增前端渲染能力，不改动任何现有 spec 的 requirement。）

## Impact

- **前端**：`stratum-web/package.json` 新增 `@excalidraw/excalidraw`；新增 `components/stratum/conversation/excalidraw-result.tsx` + `excalidraw-canvas.tsx`、`components/stratum/excalidraw/excalidraw-{workspace,editor}.tsx`、`app/(site)/excalidraw/page.tsx`；修改 `components/stratum/conversation/tool-call.tsx`（分发分支）、`components/chrome/site-chrome.tsx`（导航入口）、`components/chrome/page-transition.tsx`（PAGE_ORDER）。
- **后端/协议**：零改动。渲染输入复用现有 `ConversationToolCall.result`（JSON 文本）通道；未来后端工具只需让结果满足约定的 scene 形状即可被渲染。
- **设计约束**：只消费语义 token（`bg-card`、`border-border`、`text-muted-foreground` 等），禁止写死色值；组件级样式走 CSS Module；遵守 `prefers-reduced-motion`；键盘可达与 aria 语义与现有 ToolCall 块一致。
