# Design

<!-- 视觉世界的持久规则。产品事实见 PRODUCT.md；token 与组件实现即视觉真相。 -->

## World

克制的对话工作台：中性底（亮/暗随主题）+ 单一高饱和行动色（绿 primary），界面退后、内容在前。工具感来自排版与秩序，不来自装饰——没有渐变 hero、没有装饰性动效。canvas 时代的暗色节点世界（点阵网格、贝塞尔连线、固定暗色容器）已随 showcase 页面删除，不再是本产品的视觉语言。

## Tokens

- 唯一来源 `app/globals.css`（shadcn neutral 基色 + cssVariables），`:root` / `.dark` 双份，在 `@theme inline` 登记为 Tailwind 色。组件只消费语义 token（`bg-card`、`text-muted-foreground`、`border-border`、`bg-popover` 等），禁止写死色值/hex。
- `--primary(-foreground)` = 绿（浅色 `oklch(0.72 0.18 155)` / 深色 `oklch(0.87 0.22 150)`，前景深绿），`--ring`、`--sidebar-primary` 同源。语义：主行动（发送按钮）、选中态（会话 rail、provider chips、模型勾）、焦点环。
- `--destructive` = 错误语义：发送失败、生成中断、连接错误、会话 404。
- `--warning` = 黄（浅色 `oklch(0.65 0.15 75)` / 深色 `oklch(0.79 0.13 80)`）：需要知晓但非错误的进行中/降级语义——恢复执行、实时降级。与绿 primary（色相 155）、红 destructive（色相 27）拉开色相距离。
- 会话页所有状态提示统一走 `components/stratum/conversation/notice.tsx` 的 Notice：左对齐 tinted 横幅，结构恒定（图标 + 正文 + 可选尾部动作），三档色调 error（destructive/40 描边 + /10 底）/ warning（同构黄）/ neutral（muted 灰，取消类终态与纯信息）。terminal marker（failed/cancelled）、composer 上方提示栈（resume / degraded / 取消请求）、生成中断横幅全部同构，不做居中或裸文字特例。
- `--muted(-foreground)` = 次级文字与纯悬停反馈；`--accent(-foreground)` = 分段控件选中底（model-selector 的 Thinking 行）。
- 主题切换：`components/theme-provider.tsx`（next-themes）+ `.dark` class，整站跟随主题；不存在固定暗色的子世界。
- 遗产 token：`--canvas-grid`、`--edge`、`--port-model/-positive/-negative` 的消费方（canvas/markdown 页）已删除，不再具有设计语义，新组件不得使用，待后续清理。例外一：`--port-image`（蓝，light `oklch(0.6 0.15 250)` / dark `oklch(0.72 0.13 240)`）重新启用为"AI 处理中"状态色——目前只用于 reasoning streaming 的 Brain 图标与 shimmer（完成/折叠态回 muted 中性）；蓝只编码"进行中"，不做装饰。例外二：`--node-aurora` 已移除——Ontology 节点的顶部磨砂极光改为 CSS Module 真实类（`ontology-node.module.css` 的 `.aurora`）；自定义属性会在 `:root` 计算时固化其中的 `var()`，后代注入的每节点色相 `--node-hue` 无法生效，真实类在使用处解析才能按节点继承。
- 圆角体系：`--radius: 0.875rem` 及倍数（`rounded-sm` ~ `rounded-4xl`）。popover/卡片用 `rounded-xl`~`rounded-2xl`，小控件（触发器、chips、composer）用 pill（`rounded-full`）。阴影要有 offset + 柔和 blur（如 composer 的 `shadow-[0_8px_30px] shadow-black/10`）。

## Typography

- Geist（`font-sans`）：界面正文与控件；Geist Mono（`font-mono`）：html 默认基底、数据感场景；Roboto Slab（`--font-heading` → `font-heading`）：展示性标题（如对话页 welcome）。均由 `app/layout.tsx` 经 next/font 加载。
- 消息体排版：`components/stratum/styles/prose-medium.module.css`——`--font-reading`（Charter 系 + 中文宋体系衬线，系统字体零加载），对话用 `.proseMediumChat` 档（15→17px，行高 1.6，段落/列表/代码块等块级间距全面收紧到 0.2–1em，双类选择器压过文章档规则）。代码块与表格共享同一 ghost 语言：单描边圆角容器 + hairline，header/表头一律中性（transparent 底 + muted 文字），无 primary 彩色面。只消费外层 token、随主题切换；module 内规则保持无 `@layer`，压过 streamdown 注入元素的 utility class。
- 组件级 CSS 一律 CSS Module 随组件走（共享的放 `components/stratum/styles/`），不进 `globals.css`（globals 只放 token 与第三方样式引入）。

## Layout & Chrome

- 唯一固定外壳是顶部 SiteNav：`components/chrome/site-chrome.tsx`（数据来自 `components/react-bits/site-nav.tsx`，fixed 悬浮不占位），挂对话 / 本体 / 白板三个入口。SideDockNav 双 nav 体系已删除。
- 沉浸模式（`/excalidraw` 与本体编辑器 `/ontologies/[id]`）：SiteNavChrome 按路由把导航收起、只留画布——进入时 peek 1.6s 再滑出；顶边 8px 感应条（悬停 150ms 意图延迟）或居中阶梯两道杠手柄（w-8/5 漏斗形，点击 / 键盘聚焦，悬停微亮，Tab 第一站）唤出，离开 200ms 或 Esc 收回；GSAP 对 fixed nav 做 y/autoAlpha，展开完成 clearProps transform（避免困住内部 fixed 后代）；reduced-motion 全程瞬时。白板页与本体编辑器页因此不做顶部避让（`h-svh` 满铺），对话页与本体列表页保持常开导航 + `pt-24 sm:pt-28`。
- 页面自管避让：对话页（`app/(site)/conversation/page.tsx`）整屏 `h-svh` + 顶部留白（`pt-24 sm:pt-28`）；消息列 `max-w-[44rem]` 居中；composer sticky 在消息列底部。空会话是 Gemini 式居中开场：composer 脱离文档流绝对居中（`absolute inset-0` + flex 居中，位置与上方欢迎语/未来内容解耦，欢迎语独立锚定在中线上方），首发消息与回空态都做 GSAP FLIP（双向，composer 在中心 ⇄ 底部间滑动）。
- 会话列表 `components/stratum/conversation/thread-list-rail.tsx`：页面内 absolute 悬浮卡片，收起为图标列（w-11）、展开 w-64，选中态 `bg-primary/15 text-primary`，Esc 收回。

## Components

- **底稿隔离**：`components/ui`（shadcn 官方）只加不改；`components/assistant-ui` 是 CLI 拉取的第三方底稿，只读参考、不作为库使用；`components/react-bits` 引入后必须改造为数据驱动 + 只消费外层 token。定制全部落在 `components/stratum/`，数据走 props。
- **conversation**（`components/stratum/conversation/`）：数据驱动（`ConversationMessage`）。assistant 消息用 streamdown + `.proseMediumChat` 渲染，streaming 态带 caret；用户消息为右侧 muted 气泡；错误消息用 destructive 边框/横幅表达，重试入口由调用方决定是否提供。滚动策略：发送时用户消息先落位，再平滑滚动锚定到视口上 1/3 处（内容不够时消息列底部垫出空间，流式填满即撤并恢复贴底跟随）；跟随开关只由用户意图决定——向上滚动手势立即关，滚回底部再开。
- **渐进式透明块**（正文上方，顺序：reasoning → tools → 正文）：`reasoning.tsx` 三态（折叠/简略 3 行预览/撑开），GSAP 高度手风琴，历史默认折叠、本轮新消息默认简略；`tool-call.tsx` / `tool-group.tsx` 工具调用默认折叠（trigger = 状态图标 + 工具名，streaming 转圈 + shimmer），展开显示参数/结果/错误。审批操作入口是 composer 正上方的浮层 `approval-dock.tsx`（absolute bottom-full，不推挤消息区，GSAP 浮入/滑出，onComplete 后才移除）；内联工具块的审批区只读（待决"等待审批…"，已决终态）。卡片内容共享自 `approval-card.tsx`。dangerLevel 编码：high → destructive（边框 + 横幅），medium → port-image 蓝，low → 中性，并配中文危险度文案（不只靠颜色）。
- **PromptInput**（`components/stratum/prompt-input.tsx`）：药丸 composer，左侧 `leading` 插槽承载 agent-selector（新会话时 + 按钮触发，向上 popover；已有 runtime 不渲染），右侧 `trailing` 插槽承载 model-selector；单行时两侧按钮（28px）与文行（44px）垂直居中（items-center）；激活态是 BorderGlow 电弧（`components/react-bits/border-glow.tsx`——聚焦时整圈 mesh 渐变边框 + 外发光点亮，失焦淡出），这是界面里唯一的高亮度装饰，且有明确语义（输入激活）。
- **ModelSelector**（`components/stratum/model-selector.tsx`）：触发器 pill = 模型名 + Thinking 等级 badge + chevron；popover（`rounded-xl shadow-lg`）自上而下：cmdk 搜索框 → provider chips（选中态与 rail 同语言）→ 按 provider 分组的模型列表（选中打勾）→ Thinking 分段行。Thinking 等级由模型 schema 解析传入，无等级则不渲染该行；schema 驱动是本组件的硬约束。
- **Excalidraw（白板）**：两个承载面共享一套主题映射 `components/stratum/styles/excalidraw-theme.module.css`（无 `@layer`，把 Excalidraw 的 CSS 变量改接语义 token：紫 primary → 绿 `--primary`、浮岛 → `--popover`、字体 Geist、圆角 `--radius`，hover/选中 tint 用 color-mix 从 token 派生，亮暗同一套规则）。画布一律 `viewBackgroundColor: "transparent"` 透出容器底色（白板页 = `--background`，对话卡片 = `--card`）——Excalidraw 暗色靠 canvas invert 滤镜（透明像素不受影响），元素颜色随之翻转。Excalidraw 自带的主题切换与画布底色入口隐藏（`toggleTheme`/`changeViewBackgroundColor` false），主题只跟随站点。库与 144K 样式表走 `next/dynamic` chunk 懒加载，不进首屏。白板页 `/excalidraw`（`components/stratum/excalidraw/`，可编辑）；对话工具结果内嵌只读卡片（`conversation/excalidraw-result.tsx` + `excalidraw-canvas.tsx`，`excalidraw_render` 工具名 + scene 形状校验分发，失败回退原始 JSON）。
- **Ontology（本体管理）**：编辑器组件集中在 `components/stratum/ontology/`，路由 `/ontologies`（分页列表 + 新建/删除对话框）与 `/ontologies/[id]`（画布编辑器，沉浸模式——SiteNav 自动收起、画布满铺，同 /excalidraw）。画布用 @xyflow/react，主题只经 `ontology-theme.module.css` 把 `--xy-*` 变量改接语义 token（与 excalidraw-theme 同约定：无 `@layer`、亮暗同一套规则、画布透明透出容器底）；自定义节点/边本体直接用 Tailwind 语义类，legacy canvas token（`--canvas-grid`、`--edge`、`--port-*`）继续禁用。全部中文文案；加载、错误、空态如实呈现；412 冲突弹调和对话框，422 违例边用 `--destructive` 描边并映射到对应节点。
- **Ontology 画布节点**（`ontology-node.tsx`）：双层节点卡——玻璃背板（`bg-card/50 backdrop-blur-xl`）承载头部 = display_name + name + 描述，背板顶部衬 `.aurora` 三段 oklch 渐变（`ontology-node.module.css`，色相按节点 ID 经 `nodeHue` 稳定散列注入 `--node-hue`，每节点不同；blur-xl 化开成磨砂染色，只漫在头部区域，亮主题为低饱和粉彩、暗主题为可见光晕）；内层实心面板（`bg-popover rounded-xl`）承载属性行，行内直编辑（点名字失焦提交改名、value_type shadcn Select、必填勾选、悬停删除），底部虚线「添加属性」行自动命名并聚焦改名；交互元素挂 `nodrag`/`nowheel`，邻域只读画布省略回调、属性行退化为只读。
- **Ontology 编辑面板**（`object-type-panel.tsx`）：选中节点后画布右侧弹出宽浮层列表（`w-[28rem]`）——列表头 = display_name + name + 关闭；元信息（name / display_name / description）失焦提交；属性区一属性一行，name（mono）· display_name · value_type（Select）· 必填 · 删除全部单行内完成，行间 divide-y + 行悬停反馈，422 违例内联在对应行下方；底部「+ 添加属性」行自动命名。Link Type 面板保持窄卡（`w-80`）。

## Motion

- 动画库统一 GSAP（`gsap` + `@gsap/react`，`useGSAP` 带 scope）：SiteNav 入场/下拉、PageTransition 方向性页面转场（`components/chrome/page-transition.tsx`，`PAGE_ORDER` 当前仅 `/conversation`）。不引入第二个动画库。
- 时长/缓动全站统一尺度，唯一事实源是 `lib/motion.ts`：时长三档 fast 0.3s（退场/纯淡入淡出）、base 0.4s（进场/高度手风琴）、slow 0.55s（大位移，如 composer 中心 ⇄ 底部）；缓动两条 enter `expo.out`（一切"出现"）、exit `power2.in`（一切"消失"）。reduced-motion 判定与瞬时化（`motionDuration`）也由该模块提供，调用处不各自写 matchMedia 三元。例外：锚点滚动（`lib/scroll-to.ts`）按距离自适应属滚动行程，不纳入；react-bits 底稿只读不强制对齐。
- 基础件（popover、dialog 等）的进出动效来自 tw-animate-css，随 `components/ui` 底稿走，不再叠加。
- 所有动效必须提供 `prefers-reduced-motion` 最终态；不做装饰性循环、滚动劫持。流式消息的 caret 是排版状态而非动效，由 streamdown 提供。
