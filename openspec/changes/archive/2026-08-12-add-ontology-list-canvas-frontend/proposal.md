# Proposal: add-ontology-list-canvas-frontend

## Why

Ontology 的 HTTP API 契约已经在 `docs/ontology/API.md` 中完整定义（资源文档、ETag 乐观并发、422 校验报告、neighborhood 投影、前端保存状态机），但 Web 端目前只有对话和白板两个页面，用户无法查看或编辑任何 Ontology。本 change 依据该契约实现前端入口：Ontology 列表页与画布编辑器，让契约第一次有了真实消费者。

## What Changes

- 新增 Ontology 列表页：分页列表（`GET /v1/ontologies`）、新建（`POST`）、进入编辑器、删除（`DELETE` + `If-Match`，带确认）。
- 新增 Ontology 画布编辑器页：基于 `@xyflow/react` 的图编辑画布，节点为 Object Type（含 properties），边为 Link Type（含 cardinality），拖拽位置即 `canvas.positions`。
- 实现契约规定的保存状态机：`acknowledged` / `candidate` / `in_flight`，整文档 `PUT` + `If-Match`；412 保留 candidate 并提示用户调和，不静默重试。
- 422 校验失败时按 RFC 6901 JSON Pointer 把 violations 映射到具体节点/属性上展示，candidate 保持原样。
- IndexedDB 崩溃恢复草稿 `{ ontology_id, base_etag, candidate }`；超时/响应丢失后先重读资源再决定是否重试。
- 只读 neighborhood 视图：基于 `GET .../neighborhood?depth=0-5` 的聚焦浏览（编辑器内的 neighborhood 仍由本地 candidate 计算）。
- 扩展 `lib/stratum/api.ts` 客户端：Ontology 端点、ETag 头处理、`violations[]` 错误 envelope。
- 新增依赖：`@xyflow/react`（画布）、IndexedDB 访问（原生 API 或轻量封装，见 design.md 决策）。
- 更新 PRODUCT.md / DESIGN.md：产品范围从"单对话页"扩展到包含 Ontology 管理界面（impeccable 管理的文档同步）。

**非目标**：后端 Ontology 端点实现（spec-only，后续独立 change）；实时协同、presence、OT/CRDT、离线自动合并（契约明确排除）；实例数据管理（Ontology 是纯 schema 聚合）。

## Capabilities

### New Capabilities

- `ontology-list-ui`: Ontology 列表页——分页浏览、新建、删除、进入编辑器，含加载/错误/空态与 409 名称冲突处理。
- `ontology-canvas-editor-ui`: Ontology 画布编辑器——object type 节点与 link type 边的增删改、画布布局持久化、acknowledged/candidate/in_flight 保存状态机、ETag 并发控制、422 violations 映射、IndexedDB 崩溃恢复、只读 neighborhood 聚焦视图。

### Modified Capabilities

（无——`openspec/specs/` 中无 ontology 相关既有 spec。）

## Impact

- **前端**：`stratum-web/` 新增路由 `/ontologies` 与 `/ontologies/[id]`；`lib/stratum/api.ts` 扩展；`features/` 新增 `ontology-editor/`；`components/stratum/` 新增画布与列表组件；导航新增入口。
- **依赖**：新增 `@xyflow/react`（及其样式）；IndexedDB 用原生 API 或新增 `idb`。
- **文档**：PRODUCT.md / DESIGN.md 范围更新。
- **后端**：无改动；前端按 `docs/ontology/API.md` 契约对接，联调依赖后端 change 落地。
- **既有能力**：对话页、白板页、API 客户端既有行为不受影响。
