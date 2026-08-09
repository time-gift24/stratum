<!-- BEGIN:nextjs-agent-rules -->

# 这不是你熟悉的 Next.js

这个版本包含破坏性变更——API、约定和文件结构都可能与训练数据不同。编写代码前必须阅读 `node_modules/next/dist/docs/` 中的相关指南，并遵守弃用提示。
<!-- END:nextjs-agent-rules -->

# Stratum Web 开发约定

本目录是基于 `~/projects/front-playground` 整体重写的前端（Next.js 16 + React 19 + Tailwind v4 + pnpm）。旧版 React Router 前端已废弃，只保留与后端的交互层。

## 后端交互层（核心资产，勿随意改写）

Postgres 优先的 Agent 运行时协议（OpenSpec 变更 `complete-postgres-agent-runtime`）：

- `lib/stratum/api.ts`——REST 客户端和全部协议类型（`AgentRuntimeView` / `AgentRuntimeProductEventV1` / `AgentRuntimeDurableRecordV1` / `AgentRuntimeStreamFrameV1` / `LlmTelemetryEventV1` / `ChatMessage` / `HistoryPage`）。基础 URL 为 `process.env.NEXT_PUBLIC_STRATUM_API_BASE_URL ?? "http://127.0.0.1:18080"`。所有事件序号都是无符号十进制字符串，`compareEventSeq` 使用 `BigInt` 比较，禁止转换为 JavaScript `number`。
- `lib/stratum/event-stream.ts`——SSE 解析、`AgentRuntimeStreamFrameV1` 校验（未知 `protocol_version`、`kind` 或事件变体一律拒绝），以及 `subscribeToAgentRuntimeEvents`（没有 cursor 时从新的短尾流开始；页面内续传只使用 `after_cursor` 查询参数）。
- `lib/stratum/model-config.ts`、`lib/stratum/recent-agents.ts`——模型配置辅助函数与最近 AgentRuntime（会话）的 `localStorage` 持久化；最近记录的键必须是 `AgentRuntimeId`，不能使用可被多个运行实例复用的 `AgentId`。SSE cursor 只保存在当前页面内存中（hook 的 `Map`），禁止写入 `localStorage`。模型参数（如 Thinking 等级）一律从各模型自己的 `parameters_schema` 动态解析（`thinkingLevels` / `currentThinkingLevel` / `withThinkingLevel`），禁止在界面或 hook 中硬编码等级。
- `features/agent-conversation/{types,reducer,recovery}.ts`——负责把事件流归约为会话状态，以及恢复流程（冷启动、短尾流续传、增量收敛、向上分页），不包含界面逻辑。
- `hooks/use-agent-conversation.ts`——自包含 hook（不依赖任何外壳）：拉取模板和模型、管理最近 AgentRuntime 与页面内存 cursor、驱动 reducer/recovery，并暴露 `state`、`createConversation`、`sendMessage`、`cancel`、`resume`、`resolveApproval`、`reconnect`、`loadOlderHistory`。
- `/conversation` 是唯一连接真实后端的页面：状态到 `ConversationItem[]` 的映射在该页完成，会话组件保持数据驱动且不接入运行时。
- 推理过程与 Tool call/approval 在消息正文上方渲染：`components/stratum/conversation/reasoning.tsx`（三态折叠 + GSAP 手风琴）、`tool-call.tsx` / `tool-group.tsx`（默认折叠）。审批操作入口是输入区上方的浮层 `approval-dock.tsx`（GSAP 进出场，按钮调用 hook 的 `resolveApproval`，页面负责提交中和已决终态）；内联审批区只读，卡片内容与浮层共享 `approval-card.tsx`。历史消息中的 Tool 结果从 `state.tools[callId]` 配对，无法配对时只渲染名称和参数。
- 消息列条目（`ConversationItem`）由普通消息、`compaction-marker.tsx`（`TranscriptCompacted` 的可折叠“上下文已压缩”标记，展开显示完整摘要，不伪装为系统消息）和 `terminal-marker.tsx`（安全的失败/取消标记）组成。`notices.tsx` 承载输入区上方的恢复提示（`resume_required` 只是建议状态，必须显式点击，绝不自动恢复）与实时降级提示。
- 模型/Thinking 选择器为 `components/stratum/model-selector.tsx`（基于 assistant-ui model-selector 底稿的数据驱动分支，不接入其 runtime/ModelContext）：搜索、provider 筛选项、分组列表和 Thinking 分段行经 `composerConfiguration` 与 hook 接线，并通过 `PromptInput` 的 `trailing` 插槽挂载。

## 运行时协议投影

- 持久化身份是 `(agentRuntimeId, eventSeq)`，其中 `eventSeq` 是十进制字符串。每个帧的 `agent_runtime_id` 必须匹配当前运行实例，固定的 `agent_id` 必须匹配当前 `AgentRuntimeView`；任一不匹配都要关闭流，并进行无 cursor 冷启动。新视图仍不一致时，停止自动重连并报告协议身份错误。可见序号允许有间隔（内部 Hook/Tool 事件不发布），不能据此判断丢帧。遥测身份是 `(agentRuntimeId, llmCallId, telemetrySeq)`：低于期望值表示重复并忽略，高于期望值则把草稿标为不完整并等待持久化终稿收敛。每条遥测另带 `durable_before_event_seq` 顺序水位；PG 先收敛 assistant 最终消息 F 时，丢弃水位低于 F 的旧短尾流，但允许水位不低于 F 的下一次调用建立草稿。有 `acceptedTurnId` 时只接收该精确 Turn 的遥测；否则只接收运行中 `AgentRuntimeView` 的 `current_turn_id`，上一 Turn 的 NATS 积压不得复活草稿。
- 冷启动顺序固定为：建立 SSE 缓冲区并等待 `stream_ready` → 获取 AgentRuntimeView 和 `through=snapshot_event_seq` 的最新历史页 → 应用快照，并使用受 barrier 管理的 `AgentRuntimeView.telemetry_floor_event_seq` 初始化已收敛的 assistant 最终消息下限（不能只依赖最新历史页）→ 只把 `event_seq > barrier` 的已缓冲持久化帧放入未确认映射 → 丢弃全部已缓冲遥测 → 提交最新 cursor 并进入实时模式。AgentRuntimeView/历史读取与 SSE 共用 reset 和 AgentRuntime 切换的 abort 链，旧 generation 的结果不得写入新会话。
- SSE cursor 是不透明的 NATS 传输位置：只保存在页面内存中，不与 `event_seq`/`telemetry_seq` 比较，也不跨刷新持久化。收到 410 或 `stream_reset` 后，必须丢弃缓冲区、草稿与 cursor，并从无 cursor 冷启动重来。
- 浏览器冷启动的持久化缓冲区与 SSE 单帧解析器都必须有硬上限；溢出时必须 fail closed 并走 reset/冷恢复，禁止无界积累。
- `pgConfirmedEventSeq` 只能由 PG 快照/收敛推进，NATS product 只进入按 `event_seq` 索引的未确认映射。reconcile 固定读取完整公开 product `(B,T]`，以 view@T 为基线，再从当前映射重放全部 `>T` 帧，并按重建后的精确 Turn、状态和最终消息下限处理遥测，完成后才原子提交。reconcile 必须单飞；定时器、窗口聚焦和命令完成只合并一次补跑，不得取消当前慢分页。只有 AgentRuntime 切换、硬重置或卸载可以取消；向上分页请求仍可在 AgentRuntime 切换时取消。
- 命令合同：create 只向 `/v1/agent-runtimes` 发送 `agent_name` 与可选的完整 `model_config`，并携带客户端 UUID `Idempotency-Key`。结果不确定时，待定意图复用同一 key；key 命中后无条件返回原运行实例，且不重读模板。message 携带显式可空的 `expected_current_turn_id` CAS（`stale_turn` 只触发 reconcile，绝不静默创建第二个 Turn）。若响应不确定，同一 AgentRuntime、原文和完整模型配置的待定消息重试必须复用原 `expected_current_turn_id`，任一输入改变才形成新意图。message 的 202 响应所携精确 Turn 必须保留，直到 AgentRuntimeView 或同一 Turn 的精确持久化 `loop_started`/terminal 帧提供证明，并驱动 ready 后立即 reconcile 和低频轮询。cancel 202 只显示“取消请求已发送”；approval resolve（204）先移除待审批项再 reconcile；resume 与 resolve 是独立命令。
- 实时降级（503 `realtime_unavailable`，或已经建立 PG 快照/实时身份后的 SSE EOF/error）：显示克制的降级提示，保持 ready，并由 PG reconcile 收敛；核心命令不受影响。冷启动完成前的普通连接失败仍进入 `connection_error`。

## 工具类优先宪章

1. **`app/globals.css` 只定义系统，不实现页面。** 只允许依赖导入、shadcn 语义 Token、Tailwind `@theme` 映射、字体、基础元素样式和全局无障碍规则。
2. **具体样式写在组件的 Tailwind 工具类中。** 禁止用 `@apply` 把工具类包装成传统 CSS 类。
3. **复用依靠组件边界。** 重复模式提取为 `components/stratum/` 下的模块级 React 组件。
4. **颜色只消费语义 Token。** 不写 Hex、RGB 或同义颜色变量；状态优先使用 `data-*` / ARIA 变体。
5. **优先使用 Tailwind v4 标准能力。** 任意值仅用于 Token 无法表达的真实约束；重复的任意值提升为 `@theme` Token。
6. **React 结构必须可维护。** 不在组件函数内部声明子组件；可推导的值不另建 state；effect 依赖保持稳定；仅对真正昂贵的计算使用 memoize。
7. **外部组件保持隔离。** `components/ui/`、`components/react-bits/`、`components/assistant-ui/` 的适配通过 props、工具类、CSS 变量或包裹组件完成，不修改供应组件内部实现。

## 设计上下文

- 修改界面前必须阅读本目录 `PRODUCT.md` 与 `DESIGN.md`（均来自 front-playground）。
- 产品是单对话页应用：`/` 重定向到 `/conversation`（`app/(site)/page.tsx` 调用 `redirect`），`/conversation` 是唯一连接真实后端的页面；展示页面（首页、canvas、markdown）及其专用组件已全部删除。
- 禁止用主题化文案、无功能小字、伪技术参数制造产品感。

## 动效

- 所有动效必须提供 `prefers-reduced-motion` 最终态，不做装饰性循环或滚动劫持。

## 验证

- 协议层（`lib/stratum/` 与 `features/agent-conversation/`）允许新增 Vitest 单元测试（`*.test.ts`，纯 Node.js 环境、离线 mock），通过 `pnpm test` 运行；仍禁止新增 UI/组件测试文件。
- 前端变更至少运行 `pnpm typecheck` 与 `pnpm build`；提交前跑 `pnpm lint` 与 `pnpm test`。
