<!-- BEGIN:nextjs-agent-rules -->
# This is NOT the Next.js you know

This version has breaking changes — APIs, conventions, and file structure may all differ from your training data. Read the relevant guide in `node_modules/next/dist/docs/` before writing any code. Heed deprecation notices.
<!-- END:nextjs-agent-rules -->

# Stratum Web 开发约定

本目录是基于 `~/projects/front-playground` 整体重写的前端（Next.js 16 + React 19 + Tailwind v4 + pnpm）。旧 React Router 前端已废弃，只保留了与后端的交互层。

## 后端交互层（核心资产，勿随意改写）

Postgres-first agent runtime 协议（openspec change `complete-postgres-agent-runtime`）：

- `lib/stratum/api.ts` — REST client + 全部协议类型（`AgentView` / `AgentProductEventV1` / `AgentDurableRecordV1` / `AgentStreamFrameV1` / `LlmTelemetryEventV1` / `ChatMessage` / `HistoryPage`）。base URL 为 `process.env.NEXT_PUBLIC_STRATUM_API_BASE_URL ?? "http://127.0.0.1:18080"`。所有 event sequence 是无符号十进制字符串，`compareEventSeq` 用 BigInt 比较，禁止经 JS number。
- `lib/stratum/event-stream.ts` — SSE 解析、`AgentStreamFrameV1` 校验（未知 `protocol_version`/`kind`/event variant 一律拒绝）与 `subscribeToAgentEvents`（无 cursor 从新 tail 开始；页面内续传只用 `after_cursor` query param）。
- `lib/stratum/model-config.ts`、`lib/stratum/recent-agents.ts` — 模型配置 helper、最近会话的 localStorage 持久化。SSE cursor 只存当前页面内存（hook 的 Map），禁止写入 localStorage。模型参数（如 Thinking 等级）一律从每个模型自己的 `parameters_schema` 动态解析（`thinkingLevels` / `currentThinkingLevel` / `withThinkingLevel`），禁止在 UI 或 hook 里硬编码等级。
- `features/agent-conversation/{types,reducer,recovery}.ts` — 事件流 → 会话状态的 reducer，以及 recovery（cold bootstrap / 短 tail 续传 / 增量 reconcile / 向上分页）。UI 无关。
- `hooks/use-agent-conversation.ts` — 自包含 hook（不依赖任何 shell）：拉取 templates/models，管理 recent agents 与页面内存 cursor，驱动 reducer/recovery，暴露 `state`、`createConversation`、`sendMessage`、`cancel`、`resume`、`resolveApproval`、`reconnect`、`loadOlderHistory`。
- `/conversation` 页是唯一接真实后端的页面：state → `ConversationItem[]` 的映射在该页完成，conversation 组件保持数据驱动、无 runtime。
- reasoning 与 tool calls/approvals 在消息正文上方渲染：`components/stratum/conversation/reasoning.tsx`（三态折叠 + GSAP 手风琴）、`tool-call.tsx` / `tool-group.tsx`（默认折叠）。审批操作入口是 composer 上方的浮层 `approval-dock.tsx`（GSAP 进出场，按钮调 hook 的 `resolveApproval`，页面侧管 submitting/已决终态）；内联审批区只读，卡片内容与浮层共享 `approval-card.tsx`。历史消息工具结果从 `state.tools[callId]` 配对，配不上就只渲染 name + arguments。
- 消息列条目（`ConversationItem`）：普通消息 + `compaction-marker.tsx`（TranscriptCompacted 的可折叠"上下文已压缩" marker，展开显示完整 summary，不伪装 system 消息）+ `terminal-marker.tsx`（安全 failed/cancelled marker）。`notices.tsx` 承载 composer 上方的 resume（`resume_required` advisory，显式按钮、绝不自动 resume）与 realtime degraded 提示。
- 模型/Thinking 选择器为 `components/stratum/model-selector.tsx`（assistant-ui model-selector 底稿的数据驱动 fork，不接其 runtime/ModelContext）：搜索 + provider chips + 分组列表 + Thinking 分段行，经 `composerConfiguration` 与 hook 接线；通过 `PromptInput` 的 `trailing` 插槽挂载。

## Runtime protocol projection

- Durable identity 是 `(agentId, eventSeq)`，`eventSeq` 是十进制字符串；可见序号允许有间隔（内部 Hook/Tool 事件不发布），不是丢帧证据。Telemetry identity 是 `(llmCallId, telemetrySeq)`：低于期待值 = 重复忽略，高于期待值 = draft 标 incomplete 并等待 durable final 收敛。
- Cold bootstrap 固定顺序：SSE buffer + 等 `stream_ready` → AgentView + `through=snapshot_event_seq` 的最新 history page → 应用 snapshot → 只应用 `event_seq > barrier` 的 buffered durable frame → 丢弃全部 buffered telemetry → 提交最新 cursor 进 live mode。
- SSE cursor 是不透明 NATS transport position：只存页面内存，不与 event_seq/telemetry_seq 比较，不跨刷新持久化；410 或 `stream_reset` 后丢弃 buffer/draft/cursor，从无 cursor cold bootstrap 重来。
- Reconcile 是增量的：反向分页 history 直到越过已应用 barrier B，只合并 `(B,T]` 的可见 items 并替换 view 字段；running 时保留 active draft，view terminal 时执行与 terminal frame 相同的 draft/Tool 清理（无 result 的 Tool 标 interrupted，不伪造结果）。
- 命令合同：create 带客户端 UUID `Idempotency-Key`（pending intent 复用同一 key）；message 带显式可空 `expected_current_turn_id` CAS（stale_turn 只 reconcile，绝不静默创建第二个 Turn）；cancel 202 只显示"取消请求已发送"；approval resolve（204）先移除 pending 再 reconcile；resume 与 resolve 是独立命令。
- Realtime degraded（503 realtime_unavailable）：克制的降级提示 + PG reconcile 收敛，核心命令不受影响。

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
