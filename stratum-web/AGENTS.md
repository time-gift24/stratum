<!-- BEGIN:nextjs-agent-rules -->

# 这不是你熟悉的 Next.js

这个版本包含破坏性变更——API、约定和文件结构都可能与训练数据不同。编写代码前必须阅读 `node_modules/next/dist/docs/` 中的相关指南，并遵守弃用提示。
<!-- END:nextjs-agent-rules -->

# Stratum Web 开发约定

本目录是基于 `~/projects/front-playground` 整体重写的前端（Next.js 16 + React 19 + Tailwind v4 + pnpm）。旧版 React Router 前端已废弃，只保留与后端的交互层。

## 硬性约定（用户拍板，禁止擅自重写）

以下是用户在评审中明确拍板的约定。任何会话不得为了匹配自己的代码改动而重写本节、`DESIGN.md` 或本文件其他约定段落；确需变更约定本身时，必须先向用户说明并获明确批准。

1. **整页/整区加载一律转圈**（`LoadingState`，`components/stratum/studio/primitives.tsx`），禁止在列表、仪表盘、编辑器冷启动使用骨架屏（`components/ui/skeleton`）；eslint 已机械禁止业务组件引入它。
2. **编排式动画双主题一致生效**：页面转场、设置区内容淡入、列表卡片级联等在 light 与 dark 下都播放，统一走 `lib/motion.ts` 的时长/缓动尺度；唯一的门控是 `prefers-reduced-motion`（瞬时），禁止按主题门控动画。
3. **删除操作统一 `DeleteAction`**：页面头部右上角幽灵图标钮 + Popover 确认，禁止页面底部大红色删除区块。
4. 用户在本会话或其他会话中拍板的偏好，由执行的会话负责在本节归档；发现本节与代码不一致时，以本节为准修复代码，而不是改写本节。

## 后端交互层（核心资产，勿随意改写）

Postgres 优先的 Agent 运行时协议（OpenSpec 变更 `complete-postgres-agent-runtime`）：

- `lib/stratum/api.ts`——REST 客户端和全部协议类型（`AgentRuntimeView` / `AgentRuntimeProductEventV1` / `AgentRuntimeDurableRecordV1` / `AgentRuntimeStreamFrameV1` / `LlmTelemetryEventV1` / `ChatMessage` / `HistoryPage`）。基础 URL 为 `process.env.NEXT_PUBLIC_STRATUM_API_BASE_URL ?? "http://127.0.0.1:18080"`。所有事件序号都是无符号十进制字符串，`compareEventSeq` 使用 `BigInt` 比较，禁止转换为 JavaScript `number`。
- `lib/stratum/event-stream.ts`——SSE 解析、`AgentRuntimeStreamFrameV1` 校验（未知 `protocol_version`、`kind` 或事件变体一律拒绝），以及 `subscribeToAgentRuntimeEvents`（没有 cursor 时从新的短尾流开始；页面内续传只使用 `after_cursor` 查询参数）。
- `lib/stratum/model-config.ts`、`lib/stratum/recent-agents.ts`——模型配置辅助函数与最近 AgentRuntime 的 `localStorage` 持久化；最近记录的键必须是 `AgentRuntimeId`。SSE cursor 只保存在当前页面内存中，禁止写入 `localStorage`。模型参数一律从各模型自己的 `parameters_schema` 动态解析，禁止在界面或 hook 中硬编码等级。
- `features/agent-conversation/{types,reducer,recovery}.ts`——负责把事件流归约为会话状态，以及恢复流程（冷启动、短尾流续传、增量收敛、向上分页），不包含界面逻辑。
- `hooks/use-agent-conversation.ts`——自包含 hook：拉取 definitions/models、管理最近 AgentRuntime 与页面内存 cursor、驱动 reducer/recovery，并暴露会话命令。
- `/conversation` 是面向最终用户的真实 runtime 页面；状态到 `ConversationItem[]` 的映射在该页完成，会话组件保持数据驱动且不接入运行时。
- 推理过程与 Tool call/approval 在消息正文上方渲染；连接、命令、缺失资源等运行错误不得伪装成 assistant 消息或写入正文。

## Studio 管理面

- `/studio` 是 Agent-first 仪表盘；Provider 从全局 product navigation 最右侧的设置入口进入，不增加 Agents tab、解释区、Prompt 摘要、假指标或监控占位。
- `/studio/agents/*` 管理 Agent definition；`/studio/settings/providers/*` 管理 DB-only Provider 资源，Model 挂在 Provider 下（编辑器内列表 + 每模型真实消息测试），Model 编辑器子页为 `/studio/settings/models/{id}`。所有数据必须来自真实 management API。
- management DTO、分页、错误 envelope 与 ETag helper 统一维护在 `lib/stratum/api.ts`；更新和删除携带最近一次读取的 `If-Match`，412 保留 draft，409 展示 blocker。
- Provider secret 只允许单向替换：永不回显已存值，留空表示保留；未保存的新凭据不得用于连接测试。
- Studio 状态机、raw/structured 转换与页面缓存放在 `features/studio-management/` 和 `lib/page-cache.ts`；后台刷新不得覆盖 dirty draft，失败时保留缓存并显示可重试错误。
- Agent definition 保存成功只影响之后新建的 AgentRuntime；Provider/Model 变更从下一次 LLM work / Turn 起生效，当前 in-flight Turn 保留捕获的 Provider。

## Ontology 管理

- `/ontologies` 与 `/ontologies/[id]` 连接真实 Ontology API；编辑候选、in-flight snapshot、ETag、422 violations 与 412 显式调和由 `features/ontology-editor/` 和对应 hooks 管理。
- 画布交互集中在 `components/stratum/ontology/`；light 主题使用平面实色和 hairline，glass、blur、黑色阴影与 aurora 只允许出现在 dark。

## 运行时协议投影

- 持久化身份是 `(agentRuntimeId, eventSeq)`，`eventSeq` 是十进制字符串；帧的 runtime/agent identity 必须与当前视图一致，不一致时 fail closed 并冷启动。
- 冷启动顺序固定为 subscribe-before-snapshot：等待 `stream_ready`，读取 view 与 barrier 历史，再应用 `event_seq > barrier` 的缓冲帧并进入实时模式。
- SSE cursor 是不透明的 NATS 传输位置，只保存在页面内存中，不与 `event_seq` / `telemetry_seq` 比较，也不跨刷新持久化。
- PG snapshot 是持久化真相；NATS product/telemetry 只做实时增量，reconcile 必须单飞并由 PG 原子收敛。
- create/message/cancel/approval/resume 必须保持各自 idempotency、CAS 与显式恢复语义；实时降级不得阻断核心 PG 命令。

## 工具类与组件纪律

1. `app/globals.css` 只定义全局 reset/base、设计 token、主题和真正跨页面规则；功能样式放使用方或 CSS Module。
2. 具体样式写 Tailwind utilities；重复模式提取到 `components/stratum/` 的有职责组件。
3. 颜色只消费语义 token；优先 Tailwind v4 标准能力，重复任意值提升为 token。
4. 不在组件函数内部声明子组件；可推导值不建 state；effect 只用于外部同步；无依赖异步工作并行启动。
5. `components/ui/`、`components/react-bits/`、`components/assistant-ui/` 只加不改；适配通过 props、utility class、CSS 变量或业务 wrapper 完成。
6. 重型且非首屏必需的模块使用静态可分析的动态 import；避免无收益的 barrel import 与整包加载。

## 设计上下文

- 修改界面前必须阅读根 `PRODUCT.md`、本目录 `PRODUCT.md` 与 `DESIGN.md`。
- 正式界面包括 `/conversation`、`/studio`、`/ontologies` 与 `/excalidraw`；根路由仍进入对话。
- light 使用 `rbp-portfolio` Sunlit Reading Room 的暖纸、实色、低阴影系统，不使用 glass、glow 或 WebGL；dark 保留既有高对比反馈。页面转场与编排式入场双主题一致播放（见顶部硬性约定第 2 条），light 的克制体现在材质而非省略动效。
- 禁止主题化文案、无功能小字、伪技术参数和产品 mock 数据。

## 组件索引（先复用，后新增）

- 基础控件：`components/ui/`（shadcn 官方底稿）。
- 页面/管理组合原语：`components/stratum/studio/primitives.tsx` 与 `components/stratum/studio/*`。
- 对话：`components/stratum/conversation/*`；白板：`components/stratum/excalidraw/*`；本体：`components/stratum/ontology/*`。
- react-bits 只作为受保护底稿；业务定制落在 `components/chrome/` 或 `components/stratum/`。

## 验证

- 协议层、纯逻辑 feature 与 hook 允许新增 Vitest 单元测试（`*.test.ts`，Node 环境、离线 mock）；仍禁止为视觉细节制造脆弱的组件快照测试。
- 前端变更必须运行 `pnpm lint`、`pnpm typecheck`、`pnpm test` 与 `pnpm build`。
