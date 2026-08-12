# Design: add-ontology-list-canvas-frontend

## Context

`docs/ontology/API.md` 已完整定义 Ontology HTTP 契约：整文档资源（object_types / link_types / canvas.positions）、强 ETag + `If-Match` 乐观并发、422 JSON Pointer 校验报告、neighborhood 只读投影、以及前端保存状态机（acknowledged / candidate / in_flight）。后端端点尚未在 `crates/` 实现（spec-only）。前端 `stratum-web/` 是 Next.js 16 App Router，现有路由仅 `/conversation` 与 `/excalidraw`；API 层是手写 typed client `lib/stratum/api.ts`；状态管理惯例是 hooks + `useReducer`（见 `hooks/use-agent-conversation.ts` 与 `features/agent-conversation/`）；无图编辑库（仅 Excalidraw 白板）、无 IndexedDB 封装、无 UUID 库。

约束：PRODUCT.md / DESIGN.md 是产品与视觉权威（语义 token、GSAP only、真实数据、中文文案）；`components/ui|react-bits|assistant-ui` 只读，自有代码放 `components/stratum/`；DESIGN.md 声明的 legacy canvas tokens（`--canvas-grid`、`--edge`、`--port-*`、`--node-aurora`）不得复用。

## Goals / Non-Goals

**Goals:**

- 按契约实现列表页（`/ontologies`）与画布编辑器页（`/ontologies/[id]`）。
- 保存状态机严格符合契约：in_flight 快照语义、412 不静默重试、422 violations 按 JSON Pointer 映射。
- IndexedDB 崩溃恢复草稿与响应丢失后的先读后判。
- 所有客户端逻辑（reducer、JSON Pointer 映射、UUIDv7、草稿读写）可单测，不依赖后端。

**Non-Goals:**

- 后端端点实现（后续独立 change；前端按契约对接，联调待后端落地）。
- 实时协同、presence、OT/CRDT、离线自动合并。
- 实例（instance）数据管理；Ontology 仅为纯 schema。
- 自动生成 OpenAPI client（维持手写 client 惯例）。

## Decisions

### D1: 画布库选 `@xyflow/react`

结构化 schema 图需要受控节点/边模型、连线交互、缩放平移、minimap——与 `object_types` / `link_types` / `canvas.positions` 一一对应。Excalidraw 是自由白板，节点/边语义与 422 定位都需 hack；手写 SVG 需自实现全部交互，成本最高。两者否决。xyflow 的样式通过其 CSS variable 主题能力接入 DESIGN.md 语义 token，不复用 legacy canvas tokens。`@xyflow/react`（运行时依赖）与 `vitest`（开发依赖）均为 MIT 许可证，均为维护活跃的主流包，供应链风险低，符合宪法对新增依赖的许可证与安全性说明要求。

### D2: 状态管理用 `useReducer`，不引入 zustand

acknowledged / candidate / in_flight 是典型状态机，`features/agent-conversation/reducer.ts` 已有同构先例。zustand 虽在依赖里但自有代码未使用，不为此改变惯例。结构：

- `features/ontology-editor/types.ts`：Ontology 文档 DTO、Violations、SaveState。
- `features/ontology-editor/reducer.ts`：candidate 编辑 action、save 生命周期（`saveStarted(snapshot, etag)` / `saveSucceeded` / `saveConflict` / `saveInvalid(violations)`）、draft 恢复 action。
- `features/ontology-editor/recovery.ts`：IndexedDB 草稿读写。
- `hooks/use-ontology-editor.ts`：reducer + API 副作用编排。

### D3: 扩展 `lib/stratum/api.ts`，不改既有形状

新增 ontology 方法组（list / create / get / replace / delete / neighborhood），遵循 `createStratumApi({ baseUrl, fetcher })` 模式。ETag 通过响应头读取并随 DTO 返回（如 `{ document, etag }`）；`ApiError` 扩展可选 `violations` 字段承载 422 envelope。`fetcher` 注入点保证全部契约行为可用 mock fetcher 单测，无需真实后端——这同时满足 PRODUCT.md「真实数据、无 mock 内容」：mock 只存在于测试。

### D4: IndexedDB 用原生 API 薄封装，不加 `idb` 依赖

草稿模型只有单表单条记录（key = ontology_id，value = `{ ontology_id, base_etag, candidate }`），原生 IndexedDB promise 封装约几十行；按克制设计原则不为此新增依赖。

### D5: UUIDv7 与 RFC 6901 JSON Pointer 手写，不加依赖

UUIDv7 生成基于 `crypto.getRandomValues`（约 20 行）；JSON Pointer 解析为段数组（`~0`/`~1` 反转义，约 30 行）。两者均为纯函数，便于 property-style 单测。

### D6: 路由与组件归属

- `app/(site)/ontologies/page.tsx`（列表）、`app/(site)/ontologies/[id]/page.tsx`（编辑器）——薄页面，数据经 props 下发。
- `components/stratum/ontology/`：列表组件、画布组件（节点/边自定义渲染）、编辑面板、调和对话框、neighborhood 视图。
- 导航在 `components/chrome/site-chrome.tsx` 增加入口（使用方组件内适配，不动 ui/* 内部）。

### D7: 无位置节点的确定性布局

candidate 中无 `canvas.positions` 的节点按稳定顺序（文档数组序）做网格布局放置，拖拽后落回 positions。保证同一文档渲染结果确定，避免抖动。

### D8: PRODUCT.md / DESIGN.md 同步更新

产品范围从「单对话页」扩展为「对话 + 白板 + Ontology 管理」，按 impeccable 流程更新两份文档；范围措辞保持「前端按契约实现，后端联调待落地」的诚实表述。

## Risks / Trade-offs

- [后端端点不存在，联调阻塞] → client 层全部经 `fetcher` 注入可测；契约类型从 API.md 单一来源手抄并在类型测试中锁定；后端 change 落地后仅联调验证，不需重构。
- [契约演进导致前端类型漂移] → API.md 注明「utoipa 生成的 OpenAPI 在实现后成为权威」；联调时以 OpenAPI 为准做一次类型对账。
- [xyflow 默认样式与 DESIGN.md token 系统冲突] → 仅通过 CSS variables 主题化并封装在 `components/stratum/ontology/` 内；画布动效遵守 GSAP / prefers-reduced-motion 约束（xyflow 自身过渡保留其默认，不额外加装饰动画）。
- [整文档 PUT 在超大 Ontology（500 节点）下 payload 接近 2 MiB] → 契约已设上限；编辑器在上限处客户端阻止新增（见 spec「MVP 限制提示」），避免必然 422。
- [in_flight 语义实现错误导致丢编辑] → reducer 对该状态机做穷举单测（成功无新编辑 / 成功有新编辑 / 412 / 422 / 超时先读后判）。

## Migration Plan

纯新增路由与依赖，无数据迁移。回滚 = 移除路由入口与依赖。验证方式：reducer / pointer / uuid / recovery 单测 + 列表与画布的人工走查（后端就绪后联调）。

## Open Questions

- neighborhood 只读视图的入口形态（编辑器内切换 vs 独立路由）——实现时按「编辑器内只读模式」先做，如交互臃肿再拆。
- 列表页是否需要搜索/排序 UI——契约支持 `sort` 参数，MVP 仅默认 `-updated_at`，搜索待需求出现。
