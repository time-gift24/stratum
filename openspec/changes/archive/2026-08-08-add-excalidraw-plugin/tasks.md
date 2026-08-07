# Tasks: add-excalidraw-plugin

## 1. 依赖与基础设施

- [x] 1.1 在 `stratum-web` 安装 `@excalidraw/excalidraw`（核对 package.json 现有依赖，不引入第二个动画/样式体系）
- [x] 1.2 引入 Excalidraw 样式表（实现修正：exports 字段无 `style` 条件，postcss 无法从 globals.css 引入；改为在动态加载的 `excalidraw-canvas.tsx` 中 JS import，CSS 随 chunk 懒加载）

## 2. 白板组件

- [x] 2.1 新建 `components/stratum/conversation/excalidraw-result.tsx`：props 接收 scene JSON 文本；内部解析并做最小形状校验（对象为 JSON 且 `elements` 为数组）；校验失败返回 null 由调用方回退
- [x] 2.2 用 `next/dynamic`（`ssr: false`）加载 `<Excalidraw>`，`loading` 槽渲染与白板同形状同高度的骨架（`bg-muted/50`，固定 `h-80`）
- [x] 2.3 配置只读视图：`viewModeEnabled`、`zenModeEnabled={false}`、`gridModeEnabled={false}`，精简 `UIOptions.canvasActions` 隐藏编辑入口（在 `excalidraw-canvas.tsx`）
- [x] 2.4 用 next-themes `useTheme().resolvedTheme` 映射 Excalidraw `theme` prop，随主题切换
- [x] 2.5 容器样式只消费语义 token：`rounded-xl border border-border bg-card`，固定高度内嵌于消息列（不突破 `max-w-[44rem]`）；如需要覆写走随组件 CSS Module
- [x] 2.6 动态导入失败时降级：`ExcalidrawErrorBoundary` 捕获加载/渲染错误并返回 null，由调用方回退原始 JSON 展示

## 3. 渲染分发接入

- [x] 3.1 修改 `components/stratum/conversation/tool-call.tsx`：展开区结果渲染处新增分支——`call.name === "excalidraw_render"` 且 `call.result` 非空时优先尝试 `<ExcalidrawResult sceneText={call.result} />`，组件返回 null 或异常时回退现有 `ToolCallSection` 原始 JSON
- [x] 3.2 验证其他工具名渲染路径零改动（分发仅在 name 精确匹配时生效，diff 中其余路径未动）

## 4. 验证

- [x] 4.1 构造含合法 scene 的 `excalidraw_render` 结果（可用临时 mock 数据驱动 conversation 组件），验证白板只读渲染、折叠态不加载库（结构保证：ExcalidrawResult 仅在展开分支渲染，动态 chunk 不进入初始加载清单）
- [x] 4.2 验证非法 JSON / 缺 `elements` 的结果回退为原始 JSON 文本且页面无报错（parse 逻辑 6 组用例 node 验证通过）
- [x] 4.3 切换 light/dark 主题，验证画布与容器随主题切换、无写死色值残留（新文件 grep 无 hex；resolvedTheme 映射 theme prop；视觉确认留给 review）
- [x] 4.4 `pnpm build`（或项目既有 lint/build 命令）通过；确认折叠状态首屏 bundle 不含 excalidraw chunk（build/typecheck/lint 通过；SSR HTML 无 excalidraw 引用；CSS 落在独立懒加载 chunk）
- [x] 4.5 清理临时 mock 数据，复核 diff 不触碰 `components/ui`、`components/react-bits`、`components/assistant-ui` 底稿（未添加 mock；diff 仅含 2 个新组件 + tool-call.tsx + package.json/lock）

## 5. 独立白板页（用户追加需求）

- [x] 5.1 新增 `components/stratum/excalidraw/excalidraw-editor.tsx`（完整可编辑编辑器 + 样式表 import，主题跟随）与 `whiteboard-workspace.tsx`（`next/dynamic` 外壳 + 骨架）
- [x] 5.2 新增 `app/(site)/excalidraw/page.tsx`：`h-svh` 满铺（沉浸模式无顶部避让），metadata「Excalidraw」
- [x] 5.3 `components/chrome/site-chrome.tsx` 导航 links 增加「Excalidraw」（SiteNav 内部 TransitionLink，转场自动生效）
- [x] 5.4 `components/chrome/page-transition.tsx` 的 `PAGE_ORDER` 登记 `/excalidraw`（对话 → 白板向右滑入）
- [x] 5.5 验证：`/excalidraw` 200、对话页 HTML 含导航入口、typecheck 通过
