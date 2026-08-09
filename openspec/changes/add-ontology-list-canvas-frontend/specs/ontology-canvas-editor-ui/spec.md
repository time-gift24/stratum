# Spec: ontology-canvas-editor-ui

Ontology 画布编辑器：基于 `@xyflow/react` 的 schema 图编辑，实现 `docs/ontology/API.md` 规定的整文档保存、ETag 乐观并发与保存状态机（`acknowledged` / `candidate` / `in_flight`）。

## ADDED Requirements

### Requirement: 资源加载与 ETag 跟踪

系统 SHALL 在进入编辑器时通过 `GET /v1/ontologies/{id}` 加载完整资源文档，将响应文档与 `ETag` 记录为 `acknowledged`，并以其深拷贝初始化 `candidate`。加载失败时 MUST 展示错误态与重试入口。404 `ontology_not_found` MUST 展示明确的不存在提示。

#### Scenario: 加载成功

- **WHEN** 用户进入存在的 Ontology 编辑器路由
- **THEN** 画布按文档渲染所有 object type 节点、link type 边与已存画布位置，无位置的节点按确定性布局放置

#### Scenario: 资源不存在

- **WHEN** GET 返回 404 `ontology_not_found`
- **THEN** 系统展示不存在提示与返回列表入口

### Requirement: Object Type 与 Link Type 编辑

系统 SHALL 支持在画布上新增、编辑、删除 Object Type（含 `name`、`display_name`、可选 `description`、`properties[]`）与 Link Type（含 `source_object_type_id`、`target_object_type_id`、`source_to_target`、`target_to_source` ∈ `one|many`）。Property 的 `value_type` MUST 限定为 `string|integer|number|boolean|date|date_time` 之一。所有编辑 MUST 只修改 `candidate`，不直接触发写请求。新建 Object Type、Property、Link Type 的 ID MUST 由客户端生成 UUIDv7。`name` 字段 MUST 在客户端按 `^[a-z][a-z0-9_]{0,63}$` 先行校验。

#### Scenario: 新增 Object Type

- **WHEN** 用户在画布上新增 Object Type 并填写合法名称
- **THEN** 节点出现在画布上，candidate 新增该 object type，其 ID 为客户端生成的 UUIDv7

#### Scenario: 删除被引用的 Object Type

- **WHEN** 用户删除一个仍被 Link Type 引用的 Object Type
- **THEN** 系统在删除前提示关联的 Link Type 将一并移除，确认后 candidate 同时移除这些 link type

#### Scenario: 连接两个节点

- **WHEN** 用户从一个节点连线到另一个节点并选择双向 cardinality
- **THEN** candidate 新增一条 link type，画布渲染对应有向边及 one/many 标注

### Requirement: 画布布局持久化

系统 SHALL 将节点拖拽产生的位置写入 candidate 的 `canvas.positions`（`{ object_type_id, x, y }`），并随整文档保存一并提交。被删除 Object Type 对应的位置 MUST 同时从 candidate 移除。

#### Scenario: 拖拽节点

- **WHEN** 用户拖拽节点到新位置
- **THEN** candidate 的 canvas.positions 更新该节点坐标，画布立即反映

### Requirement: 整文档保存与 ETag 并发

系统 SHALL 通过 `PUT /v1/ontologies/{id}` 提交完整 candidate 文档，并 MUST 携带 `If-Match: <acknowledged ETag>`。PUT 发起时系统 MUST 将当前 candidate 快照与 base ETag 记录为 `in_flight`；飞行期间的用户编辑 MUST 继续写入 candidate，不得阻塞。PUT 成功（204）时系统 MUST 仅确认 `in_flight` 文档与返回的新 ETag：acknowledged 更新为该快照与新 ETag；若 candidate 已继续前进，MUST 保持 candidate 不变并以下一次保存使用新 ETag。系统 MUST NOT 在 412 后静默用新 ETag 重试旧 candidate。

#### Scenario: 保存成功且无飞行期间编辑

- **WHEN** PUT 返回 204 且 candidate 等于 in_flight 快照
- **THEN** acknowledged 更新为新 ETag，编辑器进入干净（无未保存更改）状态

#### Scenario: 保存成功但飞行期间有新编辑

- **WHEN** PUT 返回 204 但 candidate 相对 in_flight 已有新编辑
- **THEN** acknowledged 更新为 in_flight 快照与新 ETag，candidate 保留新编辑，编辑器保持未保存状态

#### Scenario: 并发冲突 412

- **WHEN** PUT 返回 412 `ontology_precondition_failed`
- **THEN** candidate 保持原样，系统重新读取最新资源与 ETag，向用户展示调和界面（保留本地版本 / 采用远端版本），由用户显式选择

### Requirement: 422 校验违例映射

系统 SHALL 在 PUT 返回 422 `invalid_ontology_schema` 时保持 candidate 不变，并 MUST 按 RFC 6901 JSON Pointer 将每条 violation 的 `path` 映射到对应的 object type、property、link type 或 canvas 位置，在画布节点与编辑面板上内联展示 `message`；无法映射到具体实体的 violation MUST 在全局错误区展示。violations MUST 按 path、code 排序展示（与响应一致，不重排）。

#### Scenario: 属性级违例

- **WHEN** PUT 返回 422 且 violation path 为 `/object_types/1/properties/0/name`
- **THEN** 系统在对应节点的对应属性行展示该违例消息，candidate 不变

#### Scenario: 文档级违例

- **WHEN** violation path 指向文档级位置（如 `/link_types` 超限）
- **THEN** 系统在全局错误区展示该违例，不附着到任何节点

### Requirement: 崩溃恢复草稿

系统 SHALL 在 candidate 变化时将 `{ ontology_id, base_etag, candidate }` 持久化到 IndexedDB 作为恢复草稿。编辑器加载时若存在对应 ontology_id 的草稿且其内容与 acknowledged 不同，MUST 向用户提供恢复或丢弃的选择。PUT 成功确认后 MUST 清除或更新该草稿。超时或响应丢失后，系统 MUST 先重新读取资源以判断 in_flight 是否已提交，再决定重试或标记未保存。

#### Scenario: 崩溃后恢复

- **WHEN** 用户重新打开编辑器且 IndexedDB 中存在该 ontology 的草稿
- **THEN** 系统提示发现未保存草稿，用户选择恢复则 candidate 还原为草稿内容，选择丢弃则删除草稿

#### Scenario: 保存响应丢失

- **WHEN** PUT 请求超时或连接中断
- **THEN** 系统重新 GET 资源：若远端内容等于 in_flight 快照则按成功处理并更新 acknowledged，否则保留 candidate 并标记为未保存

### Requirement: Neighborhood 只读聚焦视图

系统 SHALL 提供基于 `GET /v1/ontologies/{id}/object-types/{otid}/neighborhood?depth=0-5` 的只读聚焦视图，展示持久化图的邻域（默认 depth 1，上限 5）。该视图 MUST NOT 提供编辑操作，MUST NOT 写入 candidate。编辑器画布内的聚焦 MUST 由本地 candidate 计算邻域，使未保存编辑可见，不依赖该端点。

#### Scenario: 查看持久化邻域

- **WHEN** 用户在只读视图中选择某个 object type 与 depth 2
- **THEN** 系统请求 neighborhood 端点并渲染返回的 object types、link types 与 canvas 位置

#### Scenario: 邻域原点不存在

- **WHEN** neighborhood 请求返回 404 `object_type_not_found`
- **THEN** 系统展示原点不存在的提示，不改变当前视图状态

#### Scenario: 编辑器内未保存编辑的聚焦

- **WHEN** 用户在编辑器中对未保存的 candidate 使用聚焦功能
- **THEN** 邻域由本地 candidate 计算，未保存的新节点与连线可见

### Requirement: MVP 限制提示

系统 SHALL 在接近或达到契约 MVP 上限时提示用户：每个 Ontology 500 个 Object Type、每个 Object Type 100 个 Property、总计 10000 个 Property、2000 个 Link Type、500 个画布位置。超限编辑 MUST 在客户端阻止并提示，避免产生必然被 422 拒绝的 candidate。

#### Scenario: 超出 Object Type 上限

- **WHEN** candidate 已有 500 个 object type 且用户尝试再新增
- **THEN** 系统阻止新增并提示上限，candidate 不变
