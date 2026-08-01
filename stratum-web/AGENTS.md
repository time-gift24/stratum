<!-- BEGIN:nextjs-agent-rules -->
# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` before writing any code. Heed deprecation notices.
<!-- END:nextjs-agent-rules -->

# Stratum Web 开发约定

本目录是基于 `~/projects/front-playground` 整体重写的前端（Next.js 16 + React 19 + Tailwind v4 + pnpm）。旧 React Router 前端已废弃，只保留了与后端的交互层。

## 后端交互层（核心资产，勿随意改写）

- `lib/stratum/api.ts` — REST client + 全部协议类型（`AgentEvent` / `RuntimeEvent` / `StreamEnvelope` / `ChatMessage` / `LlmEvent`）。base URL 为 `process.env.NEXT_PUBLIC_STRATUM_API_BASE_URL ?? "http://127.0.0.1:18080"`。
- `lib/stratum/event-stream.ts` — SSE 解析与 `subscribeToAgentEvents`。
- `lib/stratum/model-config.ts`、`lib/stratum/recent-agents.ts` — 模型配置 helper、最近会话与 SSE cursor 的 localStorage 持久化。
- `features/agent-conversation/{types,reducer,recovery}.ts` — 事件流 → 会话状态的 reducer，以及 recovery（历史分页 + SSE replay + cursor 过期重试）。UI 无关。
- `hooks/use-agent-conversation.ts` — 自包含 hook（不依赖任何 shell）：拉取 templates/models，管理 recent agents 与 cursor，驱动 reducer/recovery，暴露 `state`、`createConversation`、`sendMessage`、`cancel`、`resume`、`resolveApproval`、`reconnect`。
- `/conversation` 页是唯一接真实后端的页面：state → `ConversationMessage[]` 的映射在该页完成，conversation 组件保持数据驱动、无 runtime。

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
- 产品是单对话页应用：`/` 重定向到 `/conversation`（`app/(site)/page.tsx` 调 `redirect`），`/conversation` 是唯一页面、接真实后端；showcase 页面（首页、canvas、markdown）及其专用组件已全部删除。
- 禁止用主题化文案、无功能小字、伪技术参数制造产品感。

## 动效

- 所有动效必须提供 `prefers-reduced-motion` 最终态，不做装饰性循环或滚动劫持。

## 验证

- 不得在 `stratum-web` 下新增前端测试文件。
- 前端变更至少运行 `pnpm typecheck` 与 `pnpm build`；提交前跑 `pnpm lint`。
