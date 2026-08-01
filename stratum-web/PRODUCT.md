# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

作者本人（前端开发者）的个人学习与实战组件库：在学习 shadcn/Tailwind 体系的同时，为后续真实网站搭建沉淀可复用组件。代码质量要求高，因为会在实际项目中复用。

## Product Purpose

一个内部组件库（stratum）+ 组件展示页：复用 shadcn 官方组件与全局 CSS token，逐件积累内部组件；核心展示物是参考图中的节点式 AI 图像生成工作流编辑器画布（整屏界面还原）。

## Positioning

个人组件库而非通用开源库：官方 shadcn 组件保持原样，内部组件收在 `components/stratum`，统一消费最外层 token。

## Operating Context

Next.js 16 + React 19 + Tailwind v4 + shadcn（base-mira 风格，neutral 基色，cssVariables）+ lucide 图标。开发命令：`pnpm dev` / `pnpm lint` / `pnpm typecheck`。纯前端实验场，无后端。

## Capabilities and Constraints

- 原则上不修改 shadcn 官方组件（`components/ui`），扩展通过组合与 stratum 包装完成。
- 组件获取一律走 shadcn CLI（`npx shadcn@latest add`，官方 registry 或 reactbits），不手写已有组件。
- 内部组件放 `components/stratum`，复用 `app/globals.css` 的最外层 token，不另建 token 体系。
- 组件展示页：逐件添加，持续积累。
- 画布（workflow editor）是核心展示物，单独成页展示。
- 无真实后端与生成能力，界面数据为静态演示数据。
- 主题 preset：用户在 ui.shadcn.com/create 选定 preset `b5TiBnTwaW`（内容未能解析，待用户补充具体 token 值）。

## Brand Commitments

- 视觉基准：`.impeccable/reference/workflow-editor.png`（暗色节点式图像生成工作流编辑器界面）。
- 绑定约束：使用 shadcn 组件 + token 组织形式。

## Evidence on Hand

- `.impeccable/reference/workflow-editor.png` —— 画布还原的唯一视觉真相。
- 无真实生成数据、用户内容或文案素材，不得虚构。

## Product Principles

- 官方组件只加不改：所有定制经组合或 stratum 包装完成。
- 单一 token 来源：一切组件消费 `app/globals.css` 的 token。
- 画布保真优先：以参考图为验收标准。
- 展示页即文档：每件组件必须在展示页可见、可用。
