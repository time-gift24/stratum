# Design

<!-- 视觉世界的持久规则。产品事实见 PRODUCT.md；token 与组件实现即视觉真相。 -->

## World

克制的双界面工作台：对话让内容在前，Studio 让配置与真实状态在前。Light 是从 `rbp-portfolio` 提炼的暖纸 soft-minimalism；dark 保留近黑石墨与 Stratum 绿色。工具感来自排版、秩序和稳定反馈，不来自装饰。

## Tokens

- 唯一来源 `app/globals.css`（shadcn neutral 基色 + cssVariables），`:root` / `.dark` 双份，在 `@theme inline` 登记为 Tailwind 色。组件只消费语义 token（`bg-card`、`text-muted-foreground`、`border-border`、`bg-popover` 等），禁止写死色值/hex。
- Light 固定语义：background `#F7F4EE`、card/popover `#FAF8F3`、foreground `#2E2D2A`、primary `#383735`、muted `#F0ECE3`、border `#E7E2DA`、scarce accent/success `#9EB6A6`、focus ring 深苔绿 `oklch(0.52 0.07 155)`（与 dark 绿 ring 同族）、destructive `#B3462F`。组件仍只消费 token，不写 hex。
- Dark 的 `--primary(-foreground)` 保留 Stratum 绿（`oklch(0.87 0.22 150)`）；dark 选中与焦点继续使用 primary。两个主题共享语义，不共享物理色值。
- `--destructive` = 错误语义：发送失败、生成中断、连接错误、会话 404。
- `--muted(-foreground)` = 次级文字与纯悬停反馈；`--accent(-foreground)` = 分段控件选中底（model-selector 的 Thinking 行）。
- 主题切换：`components/theme-provider.tsx`（next-themes）+ `.dark` class，整站跟随主题；不存在固定暗色的子世界。
- 遗产 token：`--canvas-grid`、`--edge`、`--port-model/-positive/-negative`、`--node-aurora` 的消费方（canvas/markdown 页）已删除，不再具有设计语义，新组件不得使用，待后续清理。例外：`--port-image`（蓝，light `oklch(0.6 0.15 250)` / dark `oklch(0.72 0.13 240)`）重新启用为"AI 处理中"状态色——目前只用于 reasoning streaming 的 Brain 图标与 shimmer（完成/折叠态回 muted 中性）；蓝只编码"进行中"，不做装饰。
- 圆角体系：`--radius: 0.875rem` 及倍数（`rounded-sm` ~ `rounded-4xl`）。popover/卡片用 `rounded-xl`~`rounded-2xl`，小控件用 pill。Light 层级靠暖色 tonal steps、hairline border 与 5–12% 暖墨 offset shadow；不得使用纯黑阴影。

## Typography

- Geist（`font-sans`）：界面正文与控件；Geist Mono（`font-mono`）：html 默认基底、数据感场景；Roboto Slab（`--font-heading` → `font-heading`）：展示性标题（如对话页 welcome）。均由 `app/layout.tsx` 经 next/font 加载。Studio 表面以 sans 为基底（`StudioPage` 统一挂 `font-sans`），mono 只用于数据标识与配置文本：agent 名、model id、tool 名、TOML/JSON/schema 编辑区。
- 消息体排版：`components/stratum/styles/prose-medium.module.css`——`--font-reading`（Charter 系 + 中文宋体系衬线，系统字体零加载），对话用 `.proseMediumChat` 档（15→17px，行高收紧）。只消费外层 token、随主题切换；module 内规则保持无 `@layer`，压过 streamdown 注入元素的 utility class。
- 组件级 CSS 一律 CSS Module 随组件走（共享的放 `components/stratum/styles/`），不进 `globals.css`（globals 只放 token 与第三方样式引入）。

## Layout & Chrome

- 唯一固定外壳是顶部 SiteNav：`components/chrome/site-chrome.tsx` + react-bits `SiteNav`（数据驱动），入口为对话 / 仪表盘（`/studio`）/ 本体 / Excalidraw，右端 CTA 为设置（`/studio/settings/providers`）。不得修改受保护的 `components/react-bits/*` 实现。
- 页面转场：`components/chrome/page-transition.tsx`，`PAGE_ORDER` 按导航顺序登记全部一级入口（对话 → 仪表盘 → 本体 → Excalidraw），子路由按首段归并；新增内部页面必须登记，否则只有入场没有出场。时长/缓动走 `lib/motion.ts` 统一尺度。
- 列表/表单页共享外壳与原语在 `components/stratum/studio/primitives.tsx`（跨表面复用，本体列表同款）：`PageShell`（max-w-6xl + 顶部避让 + font-sans 基底）、`PageHeader`、`ResourceCard`（squircle 标识 + 名称 + 真实状态 chip + 虚线分隔 mono meta 行，可选 action 槽）、`StatusChip`、`LoadingState`（整页/整区加载一律转圈；骨架只用于卡片框架已在、局部内容在加载的场景）、`ErrorState` / `NotFoundState`（平面虚线面板，附重试/新建出路）、`Pagination`。仪表盘（`/studio`）与本体列表（`/ontologies`）都是 ResourceCard 双列网格；沉浸页（对话、白板、本体画布编辑器）豁免。
- Studio 设置区用左侧垂直导航（`SettingsShell`，移动端横排）切换 Provider / Model；编辑使用全页平面表单——`FormSection`（ui/field 的 fieldset/legend，无卡片容器）+ `Field` + `StudioInput/StudioTextarea/StudioSelect`，raw config 只是次级视图；不得新增 Agents 页签、资源配置首屏区或监控占位。Agent 工具从 `GET /v1/tools` 目录多选（`tools-select.tsx`），不做自由文本输入。Agent 编辑器页签为 结构化 / System prompt / Raw TOML；System prompt 页签是撰写纸面而非表单字段：编辑/预览共用居中纸张（max-w-46rem、bg-card、无边框、focus-within 软 ring），编辑态用 `--font-reading` 阅读字体 + `field-sizing-content` 自动增高（min-h-50vh），与预览的 streamdown + `.proseMedium`（对话同款排版）同字同宽，切换即所见即所得。编辑器 catch 链统一走 `features/studio-management/form-state.ts` 的 `dispatchApiError`。
- 页面数据缓存：`lib/page-cache.ts`（SWR 语义，client-only）——列表与编辑器重访先渲染缓存、后台刷新替换，骨架只在冷启动出现；写操作后按前缀失效（删除类操作全清）。
- 沉浸模式（`/excalidraw`）：SiteNavChrome 按路由把导航收起、只留画布——进入时 peek 1.6s 再滑出；顶边 8px 感应条（悬停 150ms 意图延迟）或居中阶梯两道杠手柄（w-8/5 漏斗形，点击 / 键盘聚焦，悬停微亮，Tab 第一站）唤出，离开 200ms 或 Esc 收回；GSAP 对 fixed nav 做 y/autoAlpha，展开完成 clearProps transform（避免困住内部 fixed 后代）；reduced-motion 全程瞬时。白板页因此不做顶部避让（`h-svh` 满铺），对话页保持常开导航 + `pt-24 sm:pt-28`。
- 页面自管避让：对话页（`app/(site)/conversation/page.tsx`）整屏 `h-svh` + 顶部留白（`pt-24 sm:pt-28`）；消息列 `max-w-[44rem]` 居中；composer sticky 在消息列底部。空会话是 Gemini 式居中开场：composer 脱离文档流绝对居中（`absolute inset-0` + flex 居中，位置与上方欢迎语/未来内容解耦，欢迎语独立锚定在中线上方），首发消息与回空态都做 GSAP FLIP（双向，composer 在中心 ⇄ 底部间滑动）。
- 会话列表 `components/stratum/conversation/thread-list-rail.tsx`：页面内 absolute 悬浮卡片，收起为图标列（w-11）、展开 w-64，选中态 `bg-primary/15 text-primary`，Esc 收回。

## Components

- **底稿隔离**：`components/ui`（shadcn 官方）只加不改；`components/assistant-ui` 是 CLI 拉取的第三方底稿，只读参考、不作为库使用；`components/react-bits` 引入后必须改造为数据驱动 + 只消费外层 token。定制全部落在 `components/stratum/`，数据走 props。
- **conversation**（`components/stratum/conversation/`）：数据驱动（`ConversationMessage`）。assistant 消息用 streamdown + `.proseMediumChat` 渲染，streaming 态带 caret；用户消息为右侧 muted 气泡；错误消息用 destructive 边框/横幅表达，重试入口由调用方决定是否提供。
- **渐进式透明块**（正文上方，顺序：reasoning → tools → 正文）：`reasoning.tsx` 三态（折叠/简略 3 行预览/撑开），GSAP 高度手风琴，历史默认折叠、本轮新消息默认简略；`tool-call.tsx` / `tool-group.tsx` 工具调用默认折叠（trigger = 状态图标 + 工具名，streaming 转圈 + shimmer），展开显示参数/结果/错误。审批操作入口是 composer 正上方的浮层 `approval-dock.tsx`（absolute bottom-full，不推挤消息区，GSAP 浮入/滑出，onComplete 后才移除）；内联工具块的审批区只读（待决"等待审批…"，已决终态）。卡片内容共享自 `approval-card.tsx`。dangerLevel 编码：high → destructive（边框 + 横幅），medium → port-image 蓝，low → 中性，并配中文危险度文案（不只靠颜色）。
- **PromptInput**（`components/stratum/prompt-input.tsx`）：药丸 composer，右侧 `trailing` 插槽承载 model-selector。Light 聚焦只使用清晰 border/ring，不渲染 BorderGlow、backdrop blur 或 glow；dark 可保留既有语义 glow。
- **ModelSelector**（`components/stratum/model-selector.tsx`）：触发器 pill = 模型名 + Thinking 等级 badge + chevron；popover（`rounded-xl shadow-lg`）自上而下：cmdk 搜索框 → provider chips（选中态与 rail 同语言）→ 按 provider 分组的模型列表（选中打勾）→ Thinking 分段行。Thinking 等级由模型 schema 解析传入，无等级则不渲染该行；schema 驱动是本组件的硬约束。
- **Excalidraw（白板）**：两个承载面共享一套主题映射 `components/stratum/styles/excalidraw-theme.module.css`（无 `@layer`，把 Excalidraw 的 CSS 变量改接语义 token：紫 primary → 绿 `--primary`、浮岛 → `--popover`、字体 Geist、圆角 `--radius`，hover/选中 tint 用 color-mix 从 token 派生，亮暗同一套规则）。画布一律 `viewBackgroundColor: "transparent"` 透出容器底色（白板页 = `--background`，对话卡片 = `--card`）——Excalidraw 暗色靠 canvas invert 滤镜（透明像素不受影响），元素颜色随之翻转。Excalidraw 自带的主题切换与画布底色入口隐藏（`toggleTheme`/`changeViewBackgroundColor` false），主题只跟随站点。库与 144K 样式表走 `next/dynamic` chunk 懒加载，不进首屏。白板页 `/excalidraw`（`components/stratum/excalidraw/`，可编辑）；对话工具结果内嵌只读卡片（`conversation/excalidraw-result.tsx` + `excalidraw-canvas.tsx`，`excalidraw_render` 工具名 + scene 形状校验分发，失败回退原始 JSON）。

## Motion

- 动画库统一 GSAP（`gsap` + `@gsap/react`，`useGSAP` 带 scope）。Light 禁止编排式页面入场、WebGL、smooth scroll、physics chips 和装饰性 card lift；允许 Settings selected underlay 与保存反馈等局部、任务驱动动效。Dark 保留现有必要动效，不引入第二个动画库。
- 基础件（popover、dialog 等）的进出动效来自 tw-animate-css，随 `components/ui` 底稿走，不再叠加。
- 所有动效必须提供 `prefers-reduced-motion` 最终态；不做装饰性循环、滚动劫持。流式消息的 caret 是排版状态而非动效，由 streamdown 提供。
