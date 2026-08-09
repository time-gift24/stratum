# Design: add-excalidraw-plugin

## Context

对话界面的工具调用块（`stratum-web/components/stratum/conversation/tool-call.tsx`）目前是无差别的通用折叠卡片：结果一律经 `ToolCallSection` 以 `<pre>` 文本展示，没有按工具名分发渲染的机制。本 change 只整合最基础的前端部分——把 Excalidraw 以"工具结果的只读内嵌白板"形态接入，不动后端、不动协议。

约束来源：

- PRODUCT.md：渐进式透明（工具块默认折叠）；画布是未来的独立能力，不得把画布/节点隐喻带进对话页。
- `stratum-web/DESIGN.md`：只消费 `app/globals.css` 语义 token；组件级 CSS 走 CSS Module；动画统一 GSAP 且本 change 不需要动效；主题由 next-themes `.dark` class 驱动。
- AGENTS.md：`components/ui`、`components/react-bits`、`components/assistant-ui` 底稿只加不改，定制落在 `components/stratum/`。
- 数据模型：`ConversationToolCall.result` 是 JSON 文本串（`string | null`），见 `conversation/types.ts`。

## Goals / Non-Goals

**Goals:**

- 新增可复用的只读白板渲染组件，输入是 Excalidraw scene JSON 文本。
- `tool-call.tsx` 增加按工具名分发的最小分支，其余工具渲染零回归。
- 动态导入 Excalidraw，含骨架加载态与失败回退；主题跟随；token 合规。

**Non-Goals:**

- 后端 Rust 工具（`excalidraw_render`）实现与注册——后续 change，本设计只约定前端消费的 scene 形状。
- 白板编辑、导出 PNG/SVG、回写场景给 Agent。
- 独立画布路由/页面、协作。

## Decisions

### D1: 组件落点 `components/stratum/conversation/excalidraw-result.tsx`

定制组件归 `components/stratum/`（DESIGN.md 底稿隔离规则），与 tool-call 同目录，数据走 props（`sceneText: string`）。

备选：放在 `components/ai-elements/`——否决，该目录是外部底稿约束区，且 Excalidraw 渲染与对话工具块强耦合，不是通用底稿。

### D2: 按工具名 + 形状校验双重判定分发

`tool-call.tsx` 展开区中，当 `call.name === "excalidraw_render"` 时尝试将 `call.result` 解析为 JSON 并校验 `elements` 为数组；通过则渲染 `<ExcalidrawResult>`，失败回退现有 `ToolCallSection`。

备选：纯形状嗅探（任何工具结果含 `elements` 都渲染白板）——否决，误伤面大且语义不清；纯名字分发不做校验——否决，后端尚未实现，非法结果会直接炸渲染。双重判定让前端先于后端工具落地，且向后兼容。

### D3: `next/dynamic` + `ssr: false` 动态导入

Excalidraw 包体大（数 MB）且依赖浏览器 API。用 `next/dynamic` 关闭 SSR，`loading` 槽渲染固定高度骨架（`bg-muted/50`，与白板卡片同形状，避免 CLS）。导入失败由动态导入的错误边界/降级处理回退为原始 JSON。

备选：静态 import——否决，首屏 bundle 不可接受；React.lazy——`next/dynamic` 在 App Router 下是既定惯例。

### D4: 只读 view mode，固定高度

`<Excalidraw>` 传 `viewModeEnabled`、`zenModeEnabled={false}`、`gridModeEnabled={false}`，`UIOptions` 精简（隐藏 canvasActions 中与编辑相关的入口）。容器固定高度（约 `h-80`）+ `rounded-xl border-border bg-card`，只消费语义 token；不引入编辑态意味着无需处理 onChange 回写。

### D5: 主题跟随 next-themes

`useTheme()` 解析 `resolvedTheme`，映射到 Excalidraw 的 `theme="dark" | "light"` prop。容器与骨架全部使用语义 token，不写死色值。

### D6: 样式表随动态 chunk 懒加载（实现修正）

Excalidraw 样式表（约 144K）最初计划放 `globals.css` 第三方样式引入区，但 `@excalidraw/excalidraw` 的 exports 字段中 `./index.css` 只有 `development`/`production` 条件，postcss 的 `style` 条件解析失败，构建报错。改为在 `excalidraw-canvas.tsx`（被动态导入的内层模块）中 `import "@excalidraw/excalidraw/index.css"`，CSS 与 JS 一起随动态 chunk 懒加载，不进首屏 CSS。组件自身覆写（容器内字体、滚动条）如需要则随组件 CSS Module。

### D7: 独立白板页 `/excalidraw`（应用户要求加入范围）

用户明确要求可直接使用的 Excalidraw 页面 + 顶部导航入口 + 左右方向转场。实现：

- `app/(site)/excalidraw/page.tsx`：server 组件，`h-svh` 满铺无顶部避让（沉浸模式，导航由 SiteNavChrome 收起，见 D8 后的沉浸条目），metadata 标题「Excalidraw」。
- `components/stratum/excalidraw/excalidraw-workspace.tsx`：client 外壳，`next/dynamic`（`ssr: false`）+ 同形状骨架；内层 `whiteboard-editor.tsx` 携带样式表 import（同 D6 模式），渲染默认**可编辑**的 `<Excalidraw>`，主题跟随 next-themes。
- `components/chrome/site-chrome.tsx`：`links` 增加「Excalidraw」（SiteNav 内部已用 TransitionLink，转场自动生效）。
- `components/chrome/page-transition.tsx`：`PAGE_ORDER` 登记 `/excalidraw` 于 `/conversation` 右侧（对话 → 白板 = 向右滑入）。

备选：白板页复用对话内嵌的只读组件——否决，用户要的是"能直接使用"的编辑器；画布进入对话页——违反 PRODUCT.md 边界，独立路由恰好满足约束。

与 PRODUCT.md「画布不得进入对话页面」的关系：白板页是独立路由、独立交互模型，对话页零侵入；未来工作流画布落地时可再评估两者关系。

### D8: Excalidraw 主题改接产品 token（零色差）

Excalidraw 暗色画布默认 `#121212`、浮岛 `#232329` 偏蓝、主色紫——与产品 token 有明显色差。机制：Excalidraw 暗色 = 画布涂白 + `.theme--dark canvas` 的 `invert(93%) hue-rotate(180deg)` CSS 滤镜。决策：

- 画布 `viewBackgroundColor: "transparent"`（invert 滤镜对透明像素无影响），透出容器底色（白板页 `--background` / 对话卡片 `--card`），画布与页面零色差。
- 共享 CSS Module `components/stratum/styles/excalidraw-theme.module.css` 把 Excalidraw 变量改接语义 token（浮岛 `--popover`、主色 `--primary`、字体 Geist、圆角 `--radius` 等），亮暗一套规则随 `.dark` 切换；hover/选中 tint 用 `color-mix(in oklch, …)` 从 token 派生，不写死色值。特异性用 `.theme .excalidraw.theme--dark`（0,3,0）压过其三方的（0,2,0）。
- 隐藏 Excalidraw 自带主题切换与画布底色入口（`toggleTheme`/`changeViewBackgroundColor` false），主题来源唯一。

备选：覆盖 `--theme-filter` 并自管元素色——否决，元素暗色可读性全靠该滤镜；运行时 `getComputedStyle` 解析后写死进 appState——否决，主题切换需重算且引入命令式色彩逻辑。

## Risks / Trade-offs

- [Excalidraw 包体大，动态 chunk 首次加载慢] → 骨架占位 + 仅在展开工具块时才触发加载；折叠状态不渲染不加载。
- [scene 形状约定与未來后端工具漂移] → design 中显式约定（`{ elements: unknown[] }` 最小形状），后端 change 以本契约为准。
- [Excalidraw 内部样式与 shadcn token 不完全融合] → 只做容器级 token 适配，不深挖 Excalidraw 内部 CSS 变量；接受画布内默认配色（其 dark theme 已成熟）。
- [SSR 关闭导致首渲染闪烁] → 骨架高度与最终容器一致，替换无布局跳动。

## Migration Plan

纯新增：新依赖 + 新组件 + tool-call 一个分支。回滚 = 移除分支与依赖即可，无数据/协议迁移。

## Open Questions

- 后端工具产出的 scene 是否包含 `appState`（如 `viewBackgroundColor`）需要前端覆盖以适配主题？暂定：前端以最小形状渲染，appState 透传但不依赖；待后端 change 时复核。
- 是否需要"查看源码 JSON"的次级入口？暂定不需要——回退路径已覆盖排障场景。
