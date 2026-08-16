<!-- BEGIN:nextjs-agent-rules -->
# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` before writing any code. Heed deprecation notices.
<!-- END:nextjs-agent-rules -->

# Stratum Web 开发约定

本目录是基于 `~/projects/front-playground` 整体重写的前端（Next.js 16 + React 19 + Tailwind v4 + pnpm）。旧 React Router 前端已废弃，只保留了与后端的交互层。

## 后端交互层（核心资产，勿随意改写）

- `lib/stratum/api.ts` — REST client + 全部协议类型（`AgentEvent` / `RuntimeEvent` / `StreamEnvelope` / `ChatMessage` / `LlmEvent`）。base URL 为 `process.env.NEXT_PUBLIC_STRATUM_API_BASE_URL ?? "http://127.0.0.1:18080"`。
- `lib/stratum/event-stream.ts` — SSE 解析与 `subscribeToAgentEvents`。
- `lib/stratum/model-config.ts`、`lib/stratum/recent-agents.ts` — 模型配置 helper、最近会话与 SSE cursor 的 localStorage 持久化。模型参数（如 Thinking 等级）一律从每个模型自己的 `parameters_schema` 动态解析（`thinkingLevels` / `currentThinkingLevel` / `withThinkingLevel`），禁止在 UI 或 hook 里硬编码等级。
- `features/agent-conversation/{types,reducer,recovery}.ts` — 事件流 → 会话状态的 reducer，以及 recovery（历史分页 + SSE replay + cursor 过期重试）。UI 无关。
- `hooks/use-agent-conversation.ts` — 自包含 hook（不依赖任何 shell）：拉取 templates/models，管理 recent agents 与 cursor，驱动 reducer/recovery，暴露 `state`、`createConversation`、`sendMessage`、`cancel`、`resume`、`resolveApproval`、`reconnect`。
- `/conversation` 是面向最终用户的真实 runtime 页面：state → `ConversationMessage[]` 的映射在该页完成，conversation 组件保持数据驱动、无 runtime；Studio 则通过独立 management client 连接同一后端。
- reasoning 与 tool calls/approvals 在消息正文上方渲染：`components/stratum/conversation/reasoning.tsx`（三态折叠 + GSAP 手风琴）、`tool-call.tsx` / `tool-group.tsx`（默认折叠）。审批操作入口是 composer 上方的浮层 `approval-dock.tsx`（GSAP 进出场，按钮调 hook 的 `resolveApproval`，页面侧管 submitting/已决终态）；内联审批区只读，卡片内容与浮层共享 `approval-card.tsx`。历史消息工具结果从 `state.tools[callId]` 配对，配不上就只渲染 name + arguments。
- 模型/Thinking 选择器为 `components/stratum/model-selector.tsx`（assistant-ui model-selector 底稿的数据驱动 fork，不接其 runtime/ModelContext）：搜索 + provider chips + 分组列表 + Thinking 分段行，经 `composerConfiguration` 与 hook 接线；通过 `PromptInput` 的 `trailing` 插槽挂载。

## Studio 管理面

- `/studio` 是 Agent-first 的编排仪表盘；Provider 与 Model 从顶部 SiteNav 右端的设置入口进入（设置区内为左侧垂直导航），不增加 Agents tab、说明文案、prompt 摘要或伪监控指标。
- `/studio/agents/*` 管理 Agent definition；`/studio/settings/providers/*` 与 `/studio/settings/models/*` 管理底层资源。所有列表和编辑器都必须连接真实 management API，禁止用产品 mock 数据填充状态。
- management DTO、分页、错误 envelope 与 ETag helper 统一维护在 `lib/stratum/api.ts`；更新和删除必须携带最近一次读取到的 `If-Match`，`412` 显示可恢复的冲突状态，`409` 显示引用 blocker。
- Provider secret 只允许单向替换：编辑页永不回显已存 secret，留空表示保留，测试与保存期间都属于未完成操作并受离开提醒保护。
- Studio 领域状态与 raw/structured 转换放在 `features/studio-management/`；页面文件只组装路由级组件，复用的 Studio 视图放在 `components/stratum/studio/`。

## Runtime protocol projection

- Project Session-scoped envelopes and ignore non-Agent events in an Agent conversation view.
- Stable message identity is `(agentId, messageSeq)`; `messageSeq` is nested in the committed Agent
  message and is not a Session-global ordering field.
- Preserve direct versus Workflow-node `AgentLocation` in protocol types.
- Treat SSE cursor as transport-only and never compare it with another Agent's message barrier.

## Utility-first 宪章

1. **`app/globals.css` 只定义系统，不实现页面。** 只允许依赖导入、shadcn 语义 Token、Tailwind `@theme` 映射、字体、基础元素样式和全局无障碍规则。
2. **具体样式写在组件的 Tailwind utilities 中。** 禁止用 `@apply` 把 utilities 包装成传统 CSS 类。
3. **复用依靠组件边界。** 重复模式提取为 `components/stratum/` 下的模块级 React 组件。
4. **颜色只消费语义 Token。** 不写 Hex、RGB 或同义颜色变量；状态优先用 `data-*` / ARIA variants。
5. **优先使用 Tailwind v4 标准能力。** 任意值仅用于 Token 无法表达的真实约束；重复的任意值提升为 `@theme` Token。
6. **React 结构必须可维护。** 不在组件函数内部声明子组件；可推导的值不另建 state；effect 依赖保持稳定；仅对真实昂贵计算 memoize。
7. **外部组件保持隔离。** `components/ui/`、`components/react-bits/`、`components/assistant-ui/` 的适配通过 props、utility class、CSS 变量或包裹组件完成，不改供应组件内部实现。

## 设计上下文

- 修改界面前必须阅读本目录 `PRODUCT.md` 与 `DESIGN.md`（均来自 front-playground）。
- 产品有两个正式界面：`/conversation` 面向最终用户，`/studio` 面向编排者；`/excalidraw` 是白板能力。`/` 仍重定向到 `/conversation`，三个界面都使用真实产品状态。
- 浅色主题采用 warm-paper、实色、低阴影系统；不使用 glass、glow、WebGL 或装饰性页面转场。深色主题保留既有高对比与交互反馈。具体 token 与可借鉴/排除项以 `DESIGN.md` 为准。
- 禁止用主题化文案、无功能小字、伪技术参数制造产品感。

## 动效

- 所有动效必须提供 `prefers-reduced-motion` 最终态，不做装饰性循环或滚动劫持。

## 组件索引（先复用，后新增）

写 UI 前按下表顺序找组件；都没有再用 `pnpm dlx shadcn add ...` 扩 `components/ui/`；仍没有的才在 `components/stratum/` 新增。禁止手写基础组件的平行实现（根 `CONSTITUTION.md` §15）。

**基础组件 `components/ui/`（shadcn 官方底稿，只加不改）**

- 交互：button、input、input-group、textarea、select、checkbox、label、tooltip
- 浮层：dialog、popover、command（cmdk）、collapsible
- 表单：field（Field/FieldLabel/FieldDescription/FieldError/FieldGroup/FieldSet/FieldLegend）、separator
- 反馈：skeleton、avatar

**页面级共享原语 `components/stratum/studio/primitives.tsx`（跨表面复用，不只 Studio）**

- 外壳：PageShell（列表/表单页容器）、PageHeader（标题 + 返回 + 主操作）、SearchRow（列表页搜索行：放大镜即提交钮/回车提交，右侧紧跟图标化新建操作；新建入口不放 PageHeader 文字按钮）
- 资源展示：ResourceCard（squircle 标识 + 名称 + 状态 chip + 虚线 meta 行，可选 action 槽）、StatusChip（只编码真实 API 状态）
- 状态页：LoadingState（整页/整区加载的转圈指示；骨架只用于局部内容加载）、ErrorState（加载失败 + 重试）、NotFoundState（资源不存在 + 新建引导）、Pagination（列表分页，单页不渲染）
- 表单：Field（label 包裹 + 错误/说明）、FormSection（平面 fieldset 分组）、StudioInput / StudioTextarea / StudioSelect、SaveButton、InlineDelete、SettingsShell（设置区垂直导航）、controlClass

**领域组件 `components/stratum/`**

- 对话：prompt-input、model-selector、agent-selector、conversation/*
- 白板：excalidraw/*
- 本体：ontology/*（表单共享控件在 ontology/form-controls.tsx：FieldRow、CommitInput、CommitTextarea；画布 chrome 原语在 ontology/ontology-chrome.tsx：顶部 pill 族 ChromePill/PillIconButton/PillLinkButton/PrimaryPillButton/PillDivider + 卡片内动作族 CardIconButton/CardIconPopover/LinkTypeEditAction）
- Studio：studio/*（编辑器、tools-select、parameter-fields）
- 页面数据缓存：`lib/page-cache.ts`（SWR 语义：重访先渲染缓存、后台刷新；写操作按前缀失效）

**react-bits（含 Pro）**

- `components/react-bits/`（site-nav、border-glow）是引入后的改造产物。需要装饰/动画类组件时可以从 ReactBits（含 Pro 库）引入，但必须遵守同一改造规则：数据驱动、只消费语义 token、reduced-motion 终态；禁止直贴默认实现。

## 验证

- 不得在 `stratum-web` 下新增前端测试文件。
- 前端变更至少运行 `pnpm typecheck` 与 `pnpm build`；提交前跑 `pnpm lint`。
