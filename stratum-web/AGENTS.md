# Stratum Web 开发约定

## Utility-first 宪章

以下规则是前端样式实现的最高约束；新增、重构与评审都必须执行：

1. **`app/app.css` 只定义系统，不实现页面。** 该文件只允许包含依赖导入、shadcn 语义 Token、Tailwind `@theme` 映射、字体、基础元素样式和全局无障碍规则。禁止放入聊天、导航、弹窗、卡片或具体路由的类选择器。
2. **具体样式写在组件的 Tailwind utilities 中。** 布局、间距、排版、颜色、状态与响应式行为应在 TSX 上清晰可见；禁止用 `@apply` 把 utilities 重新包装成传统 CSS 类。
3. **复用依靠组件边界，不依靠全局 CSS 类。** 同时包含结构、视觉和行为的重复模式必须提取为 `app/components/stratum/` 下的模块级 React 组件。只有纯字符串重复且不形成独立语义时，才允许使用同模块的 class 常量。
4. **颜色只消费语义 Token。** 组件不得写 Hex、RGB 或同义颜色变量；使用 `background`、`foreground`、`card`、`popover`、`primary`、`secondary`、`muted`、`accent`、`destructive`、`border`、`ring`、`chart-*` 与 `sidebar-*`。状态优先使用 `data-*` / ARIA variants 表达。
5. **优先使用 Tailwind v4 的标准能力。** 使用移动优先断点、`gap-*`、`size-*`、`min-h-dvh`、`bg-linear-*` 等当前命名。任意值仅用于 Token 无法表达的真实约束；同一任意值重复出现时必须提升为 `@theme` Token。
6. **React 结构必须可维护。** 不在组件函数内部声明子组件；可从 props 或现有 state 推导的值不另建 state；effect 依赖保持稳定；仅对真实昂贵计算使用 memoization。
7. **外部组件保持隔离。** `ui`、`react-bits` 与 `ai-elements` 的适配优先通过 props、utility class、CSS 变量或 Stratum 包裹组件完成，不把产品实现重新写回供应组件或 `app.css`。

违反以上任一项的改动不能视为完成。

## 设计上下文

- 修改界面前必须阅读仓库根目录 `PRODUCT.md` 与本目录 `DESIGN.md`。
- 当前面向用户的产品只提供对话；`/component-gallery` 是内部视觉校准页，不属于一级产品导航。未来画布不得提前进入产品壳层或 `/chat`。
- `app/app.css` 的标准 shadcn 语义 Token 是产品颜色唯一来源。业务组件不得写裸 RGB、Hex 或同义颜色变量；透明度与阴影从标准 Token 推导。
- UI 使用 Geist，标题使用 Outfit，数据使用 Geist Mono，中文回退使用 Noto Sans。
- 禁止用主题化文案、无功能小字、伪技术参数或“就绪/未就绪”说明制造产品感。

## 页面与壳层

- `/` 只负责重定向到 `/chat`，不得恢复概览。
- 壳层只显示品牌、历史、唯一的新建对话入口和语言切换。
- 不使用左工具轨、概览导航、页面标签、运行状态或重复新建入口。
- 最近会话从顶部历史按钮进入右侧浮层；聊天消息使用文档滚动，不新增内部消息滚动轨道。
- `/component-gallery` 不进入 ProductShell，只用于检查 Token、排版、导航、控件和交互状态；它不得改变 `/` 到 `/chat` 的默认产品路径。

## 组件校准页

- `vertical-navigation` / `VerticalNavigation` 是固定命名，不得恢复成供应方的 `navigation-4` 名称。
- 桌面为左侧磁性 Dock，基础尺寸 48px、邻近指针最大 60px；移动端转换为至少 44px 的底部 Dock。
- 组件只消费 `app.css` 的标准 shadcn / sidebar / chart Token，不在实现内写颜色值。
- 点阵测量面只允许出现在 `/component-gallery` 与未来真实画布，不得作为全局科技装饰。

## Prompt Input

- 新对话状态只显示 Composer，不添加标题、副标题、配置节点或状态面板。
- Agent、模型与思考配置必须保留现有真实行为，并只出现在 Composer 工具区。
- 发送、取消、重连、审批和会话恢复不得因视觉删减退化。
- 移动端工具可以横向滚动，发送按钮始终可见。
- 视口 bottom 间距由最外层 fixed 容器表达，消息区必须预留完整 Composer 高度与安全区。

## 动效

- CSS 用于短反馈，Motion 用于历史浮层与 `vertical-navigation` 的磁性缩放，GSAP 只用于需要编排的一次性产品入场。
- 未来画布的空间动效等画布能力正式开发时再实现。
- 所有动效必须提供 `prefers-reduced-motion` 最终态，不做装饰性循环或滚动劫持。

## 组件所有权

- Stratum 自有组件位于 `app/components/stratum/`。
- `app/components/react-bits/` 与 `app/components/ai-elements/` 默认不得直接修改。用户已明确授权新增并适配 `app/components/react-bits/vertical-navigation.tsx`；该授权不自动扩展到其他文件。
- 用户已批准本轮在必要时调整 `app/components/ui/` 的 shadcn 源码；组件 API 与标准 Token 契约必须保持稳定。

## 验证与测试

- 不得在 `stratum-web` 下新增、恢复或维护前端测试文件。
- 前端变更至少运行格式化、`pnpm typecheck` 与 `pnpm build`。
- 视觉变更必须检查根路由重定向、`/chat` 与 `/component-gallery` 的桌面/移动、中英文、键盘焦点、对比度、减少动态效果、无横向溢出、加载/错误/禁用/发送状态和控制台错误。
