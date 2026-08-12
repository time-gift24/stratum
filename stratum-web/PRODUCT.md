# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Users

与仓库根 `PRODUCT.md` 一致：主要使用者通过对话委托真实任务；配置 Agent 的开发者与管理员是第二类使用者。Web 前端分别以对话和 Studio 服务两类任务，信息架构与概念必须保持边界。

## Product Purpose

Stratum（Rust-first Agent Runtime + 工作流编排系统）的 Web 前端。`/conversation` 承载任务执行，`/excalidraw` 承载独立白板，`/studio` 承载 Provider、Model 与 Agent definition 管理；`/` 仍直接重定向到对话。

成功标准：发起任务、理解反馈、恢复会话和管理真实 Agent 配置都清晰可信；Studio 的 revision、校验、引用 blocker 和 secret 状态必须如实呈现。

## Positioning

对话是 Agent Runtime 面向人的入口。Web 层的价值在于把真实后端状态（REST + SSE）准确、克制地呈现出来：真实数据、真实状态、按需透明。不做演示壳，不伪造数据，不把 Runtime 术语推给用户。

视觉世界继承自 front-playground 时期的设计实验（token 体系、绿 primary、Geist 字体族），但那个时期的 showcase 页面（首页组件展、canvas、markdown）及其机制已全部移除。

## Operating Context

- Next.js 16 + React 19 + Tailwind v4 + streamdown + pnpm；`@/*` alias 映射本目录根。命令：`pnpm dev` / `pnpm lint` / `pnpm typecheck` / `pnpm build`。
- 后端交互层（核心资产）：`lib/stratum/`（REST client + SSE 订阅 + 协议类型）、`features/agent-conversation/`（事件流 → 会话状态 reducer + recovery）、`hooks/use-agent-conversation.ts`（自包含 hook）。
- 后端 base URL：`NEXT_PUBLIC_STRATUM_API_BASE_URL ?? "http://127.0.0.1:18080"`。
- 会话恢复 = subscribe-before-snapshot：SSE 等待 `stream_ready` 后以 PG `snapshot_event_seq` 为 barrier 冷启动，按需向上分页；SSE cursor 只存页面内存，仅最近会话列表持久化在 localStorage。

## Capabilities and Constraints

- 路由分工：`/conversation` 面向最终用户，`/excalidraw` 是沉浸白板，`/studio` 与其子路由面向开发者/管理员；`app/(site)/page.tsx` 只做 `redirect("/conversation")`。
- 数据必须真实：消息、流式草稿、运行/失败状态、连接错误、会话 404 都有可见的对应状态；禁止 mock 数据与演示文案。
- Studio 首期只管理 Provider、Model 和 Agent definition。Agent-first 仪表盘不显示 Agents 页签、说明区、Prompt 摘要、假指标、健康灯或监控占位；资源管理仅从右上设置进入。
- 外部/底稿组件隔离：`components/ui`（shadcn 官方）、`components/react-bits`、`components/assistant-ui`（CLI 底稿，只读参考）不改内部实现；适配经 props、组合、包裹层和 token 完成。
- 组件获取一律走 shadcn CLI；fork 产物落 `components/stratum/`，数据全部走 props。
- 模型配置 schema 驱动：模型列表与 Thinking 等级来自 `GET /v1/models` 的 `parameters_schema`，UI 严禁硬编码等级（见 `lib/stratum/model-config.ts` 的 `thinkingLevels`）。
- reasoning / tool calls / approvals 渐进式透明渲染：reasoning 三态折叠展示（`components/stratum/conversation/reasoning.tsx`），工具调用默认折叠、待决审批强制展开可操作（`tool-call.tsx` / `tool-group.tsx`）；更深的过程数据（LLM 事件流等）仍只在 state 保留，不为此提前造 UI。
- 界面文案以中文为主；交互必须键盘可达、屏幕阅读器可辨。

## Brand Commitments

- 语言直接、克制、可信；不用拟人化 AI 文案、伪技术参数或主题化小字。
- 空态、错误、加载文案说人话，描述真实过程（如"连接出错：…"、"会话不存在或已被删除"）。
- 视觉规则由本目录 `DESIGN.md` 管理；token 唯一来源是 `app/globals.css`。

## Evidence on Hand

- 后端协议与交互行为以代码为真相：`lib/stratum/api.ts`、`features/agent-conversation/recovery.ts`。
- 视觉体系真相：`app/globals.css`（token）、`components/stratum/`（组件实现）。
- 产品定位上游：仓库根 `PRODUCT.md`；本文件与之对齐，冲突时以根文件为准。
- 没有可公开使用的客户案例、用量数据或市场声明，页面不得虚构。

## Product Principles

1. **界面各守任务边界。** 对话完整承载执行；Studio 完整承载配置，不互相泄漏概念。
2. **真实状态胜过演示感。** 一切数据来自真实后端；加载、错误、空态如实描述过程。
3. **渐进式透明。** 思考与工具执行默认折叠、按需展开；待决审批例外，必须直接可见可操作。
4. **schema 驱动配置。** 模型能力（含 Thinking 等级）由后端 schema 决定，UI 只解析、不假设。
5. **底稿隔离，组合扩展。** 外部组件只经 props / token / 包裹适配，定制落在 `components/stratum/`。

## Accessibility & Inclusion

主流程键盘可达（composer 提交、模型选择器内搜索/上下选择/Enter/Esc）；正文对比度 ≥ 4.5:1；所有动效提供 `prefers-reduced-motion` 最终态。
