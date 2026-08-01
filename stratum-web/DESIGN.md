# Design

<!-- 视觉世界的持久规则。产品事实见 PRODUCT.md；视觉基准：.impeccable/reference/workflow-editor.png -->

## World

暗色节点工作台（node workbench）：近黑画布 + 点阵网格，深色卡片节点悬浮其上，细灰贝塞尔连线表达数据流。工具感、低饱和，只有端口、状态与主行动作用语义色。UI 正文用 Geist sans，数据/遥测用 Geist Mono，展示页标题用 Roboto Slab（`--font-heading`）。

## Tokens

- 唯一来源 `app/globals.css`（shadcn neutral 基色 + cssVariables）。组件只消费语义 token（`bg-card`、`text-muted-foreground`、`border-border` 等），不写死色值。
- `--primary(-foreground)` = 画布绿（Generate 按钮的唯一高饱和色，浅色 `oklch(0.72 0.18 155)` / 深色 `oklch(0.87 0.22 150)`，前景为深绿），`--ring`、`--sidebar-primary` 同源。
- 领域 token 在同一文件内扩展（`:root` / `.dark` 双份）：`--canvas-grid`、`--edge`、`--port-model/-positive/-negative/-image`、`--node-aurora`（Generator 节点头部极光带），颜色类在 `@theme inline` 登记为 Tailwind 色。
- 色彩语义：绿（primary）= 行动/完成/品牌；蓝（port-image）= AI 生成中/处理中；中性（muted）= 纯悬停反馈。Markdown 世界整页主色为蓝——`app/(site)/markdown/layout.tsx` 用容器把 `--primary`/`--ring` 重映射为 `var(--port-image)`，容器内组件零改动自动跟随；站点 chrome 在容器外保持绿色。
- 画布世界固定暗色：用 `dark` class 容器呈现，不随主题切换；展示页其余部分跟随主题。
- 阅读衬线 `--font-reading`（Charter 系 + 中文宋体系，系统字体零加载），仅 `.prose-medium` 阅读排版使用。

## Node anatomy

- 卡片：`bg-card`、`border`（1px `--border`）、`rounded-2xl`；头部 = 状态点 + 标题 + 收起 chevron；主行动（Generate）为亮绿 pill，黑字。
- 端口：小色点 + muted 标签；输入在左、输出在右；色点颜色 = 数据类型语义（model 黄绿 / positive 绿 / negative 红 / image 蓝）。
- 节点标签（色点 + 名称）悬浮于节点上方，不属于卡片本体。

## Edges & Canvas

- 连线：1.5px 水平贝塞尔曲线，`--edge` 色、半透明，无箭头、无动画。
- 画布：近黑底 + 径向点阵网格（`--canvas-grid`），网格只在画布世界出现。
- 协作光标：彩色箭头指针 + 同色名牌，颜色是协作者身份，走 props 而非 token。

## Stratum 规则

- `components/ui`（shadcn 官方）只加不改；新组件一律用 shadcn CLI 从 registry 安装（官方 / reactbits），不手写已有组件。
- 内部组件放 `components/stratum`，通过组合官方组件扩展。
- 第三方组件（`components/react-bits`）引入后必须改造：数据驱动（内容走 props）+ 颜色只消费最外层 token（不写死 neutral/black/white 与 `dark:` 变体），动效与结构保留原样。
- 动画库统一 GSAP（`gsap` + `@gsap/react`；`useGSAP` 带 scope，reduced-motion 必须处理）；不引入第二个动画库（motion 已移除）。
- 展示页逐件登记：每件组件一个 section（标题 + 说明 + demo），新增组件 = 新 section + 在 `components/chrome/site-chrome.tsx` 登记两个 nav。现有展示页：`/`、`/markdown`；`/conversation` 为整屏界面页（chat 场景，非 section 模式）。
- conversation 组件库：`components/assistant-ui/` 是 CLI 拉取的底稿区（第三方源码，只读参考，eslint 忽略，不作为库使用）；fork 产物落 `components/stratum/conversation/`——剥掉 runtime（primitives/provider/hooks），数据全部走 props（`ConversationMessage` / `ConversationThreadMeta`），消息体渲染用我们自己的 streamdown + `.prose-medium`（不引 assistant-ui 的 markdown-text / composer）。
- 布局：双 nav 悬浮体系是唯一固定外壳——SiteNav 顶部 fixed（`(site)` 组 layout 挂载）、SideDockNav 左侧 fixed（各页面场景自挂自登记）；两者不占位、悬浮于所有页面之上，页面自管避让。核心界面 `/canvas` 也在 `(site)` 组内，整屏暗色，nav 悬浮其上。
- 阅读排版：Medium 风格集中在共享的 `components/stratum/styles/prose-medium.module.css`（`--font-reading` Charter 系衬线正文 + Geist 无衬线标题 + 居中三点分节符 + 无边斜体引用），只消费外层 token、随主题切换；组件级 CSS 一律用 CSS Module 随组件走（跨组件共享的放 `stratum/styles/`），不进 `globals.css`（globals 只放 token 与第三方样式引入）。module 内规则保持无 `@layer`，压过 Streamdown 注入元素的 utility class。
