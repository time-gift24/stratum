# Stratum Site 开发约定

落地页与新前端底座。Next.js 16 + React 19 + Tailwind CSS v4 + pnpm，技术栈选型参照 `ai-saas/`。

## 设计权威

- 视觉系统唯一权威是 `DESIGN.md`（「流体雾场·纸墨」：宣纸米白→暖墨黑的垂直下潜、MoltenMetal 熔墨全页背景、中英混排大标题、朱砂配给制 + 古铜辅助、禁红绿并置）。改 UI 前先读它。
- 产品事实唯一权威是仓库根 `PRODUCT.md`；页面不得伪造客户、数据、定价。
- 方向契约记录在 `app/page.tsx` 头部注释（THESIS / OWN-WORLD / STORY / FIRST VIEWPORT / FORM）。

## 结构

- `app/`：路由与全局。`globals.css` 只允许 token（`@theme`）、base reset、跨页面基础规则（自定义 base 规则必须放进 `@layer base`，否则非层叠样式会压掉 utilities——focus ring 曾因此被错误激活）。路由：`/`（落地页）、`/components`（组件展示页）、`/conversation`（主对话区外壳）。
- `lib/`：`content.ts`（双语字典，zh 为形状权威，en 必须逐 key 对齐）、`i18n.tsx`（语言上下文，检测/切换都要同步 `document.documentElement.lang`）、`motion.tsx`（reduced-motion + variants/缓动尺度）、`config.ts`（外链常量，GitHub URL 在此替换）、`cn.ts`。
- `components/ui/`：基础组件（button / card / chip / section / prompt-box / trace-ribbon / terminal-card / language-toggle），shadcn 式写法：数据走 props、变体走 variant map、样式只消费语义 token。
- `components/react-bits/molten-metal.tsx`：React Bits 底稿（ogl shader，MIT），TS 化 + 背景场适配（指针监听挂 window、容器 pointer-events-none）。
- `components/ink-field.tsx`：全页固定熔墨背景（雾边纸色 → 深褐丝缕 → 朱砂红芯），reduced-motion 时不渲染，由 `page.tsx` 的半透明下潜渐变与 paper 底色保底。
- `components/site-nav.tsx` / `site-footer.tsx`：站点框架。顶栏滚动后渐变为磨砂玻璃 pill（透明度/blur/阴影随滚动进度经 ref 直改 DOM），并随下潜深度在墨色/骨白两态间切换（视口中线进入 abyss 渐变带即反相，`page.tsx` 渐变 stop 与 `site-nav.tsx` 阈值必须一起调），禁止改回 `mix-blend-difference`——中明度雾带上 blend 会失去对比度。
- `components/app/`：app 外壳（app-top-bar / app-sidebar）。顶栏与首页导航同为磨砂玻璃 pill，进入时宽度从 38% 丝滑展开到 100%（ease-fluid 0.9s）；侧栏顶部为品牌位（logo 待补，「筹」字章占位）+ 对话区右上方纯图标收起钮（PanelLeft）；资源区钉在侧栏最底部；lg 以下收纳为抽屉。
- `features/landing/`：落地页四个段落（hero / mechanism / depth / final-cta），只组合组件，不承载可复用控件。
- `features/conversation/`：主对话区外壳。Runtime 未接入本站点——落地页 `?task=` 预填输入框，提交只在本地追加用户消息，并明示"界面预览"；禁止伪造 agent 回复。
- `features/gallery/`：`/components` 展示页，实拍全部 token 与公共组件；新增公共组件必须同步加展区。

## 硬性约定

1. 颜色只消费 `globals.css` 的语义 token；**公共组件（`components/`）禁止出现任意值**（`tracking-[…]`、`text-[…px]`、`shadow-[…]`、`ease-[…]` 等）——重复出现的值必须先提为 `@theme` token 再使用。feature 页面里的文案行宽（`max-w-[Nch]`）与页面级渐变是仅有的豁免。
2. 编排式动画统一走 `lib/motion.tsx` 的尺度，唯一门控是 `prefers-reduced-motion`（瞬时呈现）。例外：app 顶栏的 width 展开（38% → 100%）是一次性入场签名动画，属 transform/opacity 规则的明示例外，保留不改。
3. 组件内部不定义子组件；可推导值不建 state；effect 只用于外部系统同步；遵循 `vercel-react-best-practices` skill。公共组件的样式覆盖只经 `cn()`（tailwind-merge，后传入的冲突类胜出）与 variant props；需要新形态时先加 variant，不要在调用方堆一长串覆盖类。
4. **新增自定义字号 token（`--text-*`）必须同步注册进 `lib/cn.ts` 的 twMerge `font-size` 组**——否则它会被误判为 `text-{color}`，吞掉同元素上的文字颜色类（按钮曾因此文字不可见）。
5. 文案改动只动 `lib/content.ts`，保持 zh/en key 对齐；中文标题两行以内，衬线斜体强调词永远是英文且每屏最多一个。
6. 变更必须过 `pnpm lint`、`pnpm typecheck`、`pnpm build`；若 3100 生产服务器在跑，build 后必须重启它，避免给人看旧版本。
7. 熔墨背景是签名体验：`components/react-bits/` 是受保护底稿目录，行为适配（如指针监听层级）必须注释说明；配色只允许从语义 token 取色；朱砂红芯是材质肌理，不计入每屏朱砂配额（见 DESIGN.md One Seal Rule）。
