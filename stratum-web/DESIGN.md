# Design

<!-- 视觉世界的持久规则。产品事实见 PRODUCT.md；token 与组件实现即视觉真相。 -->

## World

克制的双界面工作台：对话让内容在前，Studio 让配置与真实状态在前。Light 采用 `rbp-portfolio` 的 “Sunlit Reading Room” 暖纸 soft-minimalism（暖白/炭黑/鼠尾草绿，低饱和、可信 SaaS 气质）；dark 是 PicGen 式 AI 工作台（深炭画布/炭灰表面/荧光信号色，信号色只编码语义不装饰）。工具感来自排版、秩序和稳定反馈，不来自装饰。

## Tokens

- 唯一来源 `app/globals.css`（shadcn neutral 基色 + cssVariables），`:root` / `.dark` 双份，在 `@theme inline` 登记为 Tailwind 色。组件只消费语义 token（`bg-card`、`text-muted-foreground`、`border-border`、`bg-popover` 等），禁止写死色值/hex。
- Light = `rbp-portfolio` “Sunlit Reading Room” 配方：background `#F7F4EE`、card/popover `#FAF8F3`、foreground `#2E2D2A`、primary `#383735`、muted `#F0ECE3`、border `#E7E2DA`、品牌/选中 `#9EB6A6`（鼠尾草绿，低饱和）、focus ring `#7378D8`（靛蓝，只编码键盘焦点）、destructive `#B3462F`、`--success #5DBB7A`（只编码成功态，chip/状态点用，不写小字号正文）、`--ai #7378D8`（靛蓝，只用于 AI 功能与链接，不扩展成主色）。参考项目的 content-muted `#9AA3B2` 在暖白上仅约 2.8:1 不达 AA 正文线，`--muted-foreground` 仍用更深的 `#68645D`。组件仍只消费 token，不写 hex。
- Dark = PicGen 信号色板：background `#0D0F0E`、card/popover `#282B29`、elevated（secondary/muted/accent）`#333633`、foreground `#F1F4EF`、muted-foreground `#A2AAA1`、border `#4C514D`、primary/ring `#A5FF4F`（荧光绿=主要操作，不是装饰）、destructive `#F15C63`、`--success #68EBA0`、`--ai #73BFFF`。chart1-5 = lime/yellow/green/blue/pink 信号族。荧光色的语义分工：黄=模型/参数，绿=正向/成功/可操作，红=负向/风险，蓝=媒体/AI，粉=协作标记——颜色是状态编码，不是氛围装饰。
- `--destructive` = 错误语义：发送失败、生成中断、连接错误、会话 404。
- `--muted(-foreground)` = 次级文字与纯悬停反馈；`--accent(-foreground)` = 分段控件选中底（model-selector 的 Thinking 行）。
- 主题切换：`components/theme-provider.tsx`（next-themes）+ `.dark` class，整站跟随主题；入口是导航右端的图标切换钮（SiteNavChrome actions 槽），没有键盘快捷键；不存在固定暗色的子世界。
- 端口/信号 token：`--port-model/-positive/-negative/-image/-collab` 在 dark 即 PicGen 信号色（`#E7F238`/`#68EBA0`/`#F15C63`/`#73BFFF`/`#F05BD2`），light 为同色相的降明度版本。`--port-image`（蓝）同时承担"AI 处理中"状态色（reasoning streaming 的 Brain 图标与 shimmer，完成/折叠态回 muted 中性）；`--ai` 是 AI 功能/链接的通用强调色，二者色相相近但角色不同。`--canvas-grid` dark = `#1C231F` 点阵。
- 圆角体系：`--radius: 0.875rem` 及倍数（`rounded-sm` ~ `rounded-4xl`）。popover/卡片用 `rounded-xl`~`rounded-2xl`，小控件用 pill。Light 层级靠暖色 tonal steps、hairline border 与 5–12% 暖墨 offset shadow；不得使用纯黑阴影。

## Typography

- Geist（`font-sans`）：界面正文与控件；Geist Mono（`font-mono`）：html 默认基底、数据感场景；Roboto Slab（`--font-heading` → `font-heading`）：展示性标题（如对话页 welcome）。均由 `app/layout.tsx` 经 next/font 加载。Studio 表面以 sans 为基底（`StudioPage` 统一挂 `font-sans`），mono 只用于数据标识与配置文本：agent 名、model id、tool 名、TOML/JSON/schema 编辑区。
- 消息体排版：`components/stratum/styles/prose-medium.module.css`——`--font-reading`（Charter 系 + 中文宋体系衬线，系统字体零加载），对话用 `.proseMediumChat` 档（15→17px，行高收紧）。只消费外层 token、随主题切换；module 内规则保持无 `@layer`，压过 streamdown 注入元素的 utility class。
- 组件级 CSS 一律 CSS Module 随组件走（共享的放 `components/stratum/styles/`），不进 `globals.css`（globals 只放 token 与第三方样式引入）。

## Layout & Chrome

- 唯一固定外壳是顶部 SiteNav：`components/chrome/site-chrome.tsx` + react-bits `SiteNav`（数据驱动），入口为对话 / 仪表盘（`/studio`）/ 本体 / Excalidraw，右端是 actions 槽的图标操作（主题切换 + 设置入口 `/studio/settings/providers`，纯图标无文字，44px 触控盒，设置恒为最后一项）。业务 `TransitionLink` 为当前链接及其子路由提供 `aria-current="page"`；从仪表盘进入设置时携带仅含 `q/page` 的安全 `returnTo`，其他路由回退 `/studio`。吸顶状态机：页顶展开为 `max-w-6xl` 宽条（与 PageShell 内容同宽）；滚动超过 12px 收缩为 `max-w-[38rem]` 居中 pill——容器恒为 `w-full`，两态切换走 `max-width` 数值过渡（500ms `cubic-bezier(0.32,0.72,0,1)`），收缩/展开是连续动画而不是跳变（capture 阶段监听 scroll——scroll 不冒泡但会捕获，window 与对话线程流等内部滚动容器都能驱动切换）。Light 由使用方 `components/chrome/site-chrome.module.css` 覆盖为不透明 `bg-card` + hairline border，无 blur、glow、shadow 或装饰入场；dark 保留 `bg-card/55` + `backdrop-blur-2xl` + `saturate-150` + hairline border + 浅阴影的 graphite glass。栏内布局：品牌（text-base 字标 + 主色状态点）与导航链接（text-[0.9375rem]）组成左侧文字组，不居中；右侧 actions 图标槽。受保护的 `components/react-bits/*` 实现改动需用户批准（actions 槽与吸顶状态机均已获批准）。
- 页面转场：`components/chrome/page-transition.tsx`，`PAGE_ORDER` 按导航顺序登记全部一级入口（对话 → 仪表盘 → 本体 → Excalidraw），子路由按首段归并；新增内部页面必须登记，否则只有入场没有出场。同级路径（父目录相同）视为页签切换、设置区内部（`/studio/settings/**` 任意互跳）视为区内导航，两者都跳过整页滑入，由局部动效接管。双主题统一按 `lib/motion.ts` 的统一尺度播放入场/出场，reduced-motion 瞬时。
- 列表/表单页共享外壳与原语在 `components/stratum/studio/primitives.tsx`（跨表面复用，本体列表同款）：`PageShell`（max-w-6xl + 顶部避让 pt-24 sm:pt-28——悬浮 nav 底边约 88px，列表页顶部留白必须大于标题下方间距 + font-sans 基底）、`PageHeader`、`SearchRow`（搜索框——放大镜即提交钮/回车提交——右侧紧跟图标化新建操作；仪表盘与本体列表同款，新建不再是 PageHeader 里的文字按钮）、`ResourceCard`（squircle 标识 + 名称 + 真实状态 chip + 虚线分隔 mono meta 行，可选 action 槽）、`StatusChip`、`ResourceGridSkeleton`（Agent/Provider/Model 列表冷启动使用与最终两列卡片同形的标识、标题和 meta 行骨架）、`LoadingState`（编辑器等未知内容形态的整区加载）、`ErrorState` / `NotFoundState`（平面虚线面板，附重试/新建出路）、`Pagination`。命中页面缓存时先保留内容，权威刷新失败仍显示安全错误与重试。仪表盘（`/studio`）与本体列表（`/ontologies`）都是 ResourceCard 双列网格 + SearchRow；本体搜索走后端 `search` 参数（name/display_name 大小写不敏感包含匹配），空结果空态给"清除筛选"出路；沉浸页（对话、白板、本体画布编辑器）豁免。
- Studio 设置区是 Provider 单入口（不再有 Provider/Model 平行页签与左侧 SettingsNav）：`/studio/settings/providers` 列表 → Provider 编辑器。共享 layout（`app/(site)/studio/settings/layout.tsx` + `components/stratum/studio/settings-chrome.tsx`）只保留 PageShell + 区内导航（列表 ↔ 编辑器）的一次快速内容淡入上浮（双主题一致，首屏到达交给整页转场，reduced-motion 瞬时）。入场动画遵循单层契约：导航（pathname 变化）时入场由容器淡入/整页转场独占，卡片不再叠加级联；卡片级联（`provider-list.tsx`）只在无导航的参数变化（搜索/翻页）后数据到达时播放，后台刷新替换永不播。所有列表/区块的加载指示经 `useDelayedFlag` 延迟 150ms，短于延迟的加载不闪 spinner。Model 挂在 Provider 下且没有独立编辑页：Provider 编辑器内的 `ProviderModelsSection`（`components/stratum/studio/provider-models.tsx`）列出该 Provider 的全部 Model，行内发真实消息测试（`POST /v1/providers/{p}/models/{m}/test`，成功显示延迟毫秒、失败显示原因）、行内 Popover 确认删除（先 GET 取 ETag 再 DELETE，412 提示重试），底部行内输入名称直接添加；测试结果文本用 success/destructive token，不用装饰色。页面头部（PageHeader，含各页标题/返回/操作）随内容切换。删除操作统一 `DeleteAction`——头部右上角幽灵图标钮 + Popover 确认（解释 + 取消/确认），禁止页面底部大红色区块。编辑使用全页平面表单——`FormSection`（ui/field 的 fieldset/legend，无卡片容器）+ `Field` + `StudioInput/StudioTextarea/StudioSelect`；不得新增 Agents 页签、资源配置首屏区或监控占位。Agent 工具从 `GET /v1/tools` 目录多选（`tools-select.tsx`），不做自由文本输入。Agent 编辑器页签为 结构化 / System prompt / Raw TOML；System prompt 页签是撰写纸面而非表单字段：编辑/预览共用居中纸张（max-w-46rem、bg-card、无边框、focus-within 软 ring），编辑态用 `--font-reading` 阅读字体 + `field-sizing-content` 自动增高（min-h-50vh），与预览的 streamdown + `.proseMedium`（对话同款排版）同字同宽，切换即所见即所得。编辑器 catch 链统一走 `features/studio-management/form-state.ts` 的 `dispatchApiError`。
- 页面数据缓存：`lib/page-cache.ts`（SWR 语义，client-only）——列表与编辑器重访先渲染缓存、后台刷新替换，骨架只在冷启动出现；写操作后按前缀失效（删除类操作全清）。
- 本体画布编辑器（`/ontologies/[id]`）满铺无头部栏（`ontology-chrome.tsx` 原语）：顶部悬浮 pill 群——左 = 返回+标题+保存状态，右 = 视图切换/新增 pill + 独立保存主操作（`PrimaryPillButton`，有脏数据时实心 primary、图标+文字）；节点/边的操作长在卡片上（`CardIconButton`/`CardIconPopover`，nodrag）：节点头部 display_name/name/描述双击即行内编辑（失焦提交+校验），聚焦是图标+深度直选（选中即聚焦，无弹窗），删除悬停/选中显现；边选中后标签下浮出编辑、删除；不用任何侧栏或常驻面板。节点属性是单行两列（name mono 与 display_name 各自点击行内编辑、失焦提交+校验，两值独立不再强制同值），value_type 深色瓦片 Select。light 的节点、pill、横幅与浮岛一律使用实色 token + hairline、无 blur/玻璃/黑色阴影/极光渐变；dark 节点极光染色锚定 root（relative）只漫头部，并保留既有 graphite glass。草稿/保存错误/违例/聚焦提示是顶部居中浮层，只在有事时出现。
- 站点导航（SiteNavChrome）在所有页面常开悬浮，包括 `/excalidraw` 与 `/ontologies/[id]` 沉浸页（不再有自动收起/唤出手柄）；沉浸页画布满铺 `h-svh` 不做顶部避让，页内悬浮 chrome 自行避让顶栏（本体编辑器 pill 群 pt-20、横幅 top-32）。白板工具栏经 `excalidraw-theme.module.css` 的 `verticalTools` 竖置右缘（shapes-section 锚定 layer-ui 右缘垂直居中——左缘会遮挡 Excalidraw 样式面板；按钮容器 `.Stack_horizontal` 是 grid，用 `grid-auto-flow: row` 改列；`.HintViewer` 与顶部 hint 隐藏；`.App-menu_top` 改两列 `1fr 1fr`（!important，与库样式同优先级后加载会输）避让常开导航并让 Library 回右列；仅白板页挂该类，对话内嵌卡片与移动端底栏不受影响）。对话页保持 `pt-20`。
- 页面自管避让：对话页（`app/(site)/conversation/page.tsx`）整屏 `h-svh` + 顶部留白（`pt-20`，对齐收紧后的导航高度）；消息列 `max-w-[44rem]` 居中；composer sticky 在消息列底部。空会话是 Gemini 式居中开场：composer 脱离文档流绝对居中（`absolute inset-0` + flex 居中，位置与上方欢迎语/未来内容解耦，欢迎语独立锚定在中线上方），首发消息与回空态都做 GSAP FLIP（双向，composer 在中心 ⇄ 底部间滑动）。
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
