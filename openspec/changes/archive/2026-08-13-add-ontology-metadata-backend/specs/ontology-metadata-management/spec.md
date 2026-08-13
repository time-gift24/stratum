## ADDED Requirements

### Requirement: Ontology 使用完整聚合文档

系统 SHALL 使用完整 JSON 文档表示一个 Ontology。wire shape MUST 由以下字段组成；除标为可选的 `description` 外，字段均为必需且不得为 `null`：

| Shape | Fields |
| --- | --- |
| Ontology | `id`, `name`, `display_name`, optional `description`, `object_types`, `link_types`, `canvas` |
| Object Type | `id`, `name`, `display_name`, optional `description`, `properties` |
| Property | `id`, `name`, `display_name`, optional `description`, `value_type`, `required` |
| Link Type | `id`, `name`, `display_name`, optional `description`, `source_object_type_id`, `target_object_type_id`, `source_to_target`, `target_to_source` |
| Canvas | `positions` |
| Canvas Position | `object_type_id`, `x`, `y` |

`object_types`、每个 `properties`、`link_types` 与 `canvas.positions` MUST 存在，即使数组为空。完整资源 MUST NOT 暴露 revision、timestamps、sort order 或存储字段。系统 MUST 保持 Property 由一个 Object Type 独占，Link Type 两端与 Canvas Position 只引用同一文档内的 Object Type，并且不在该能力中存储对象实例。

Ontology、Object Type、Property 与 Link Type MUST 使用各自的 UUIDv7 类型化 ID。服务生成 Ontology ID；客户端生成子实体 ID，服务 MUST 原样保留而不重映射。Object Type、Property 与 Link Type ID MUST 分别在全部现存 Ontology 中全局唯一。Ontology `name` 在部署内唯一；Object Type 与 Link Type `name` 在各自独立的 Ontology-local 命名空间唯一；Property `name` 仅在所属 Object Type 内唯一。

#### Scenario: 返回空 Ontology

- **WHEN** 客户端读取一个新创建的 Ontology
- **THEN** 响应包含空的 `object_types`、`link_types` 和 `canvas.positions`
- **AND** `canvas` 与这些集合均不被省略

#### Scenario: 保存并读取完整文档

- **WHEN** 客户端成功保存一个合法完整文档
- **THEN** 后续读取保留提交的子实体 ID、语义值、可选字段省略状态与数组顺序
- **AND** 服务不注入 schema 默认值、不改变值且不重排数组

#### Scenario: Property 名称按所有者限定

- **WHEN** 两个不同 Object Type 各自声明同名 Property
- **THEN** 候选文档可以通过名称唯一性校验

#### Scenario: 子实体 ID 已被另一 Ontology 使用

- **WHEN** 候选文档中的 Object Type、Property 或 Link Type ID 已属于另一现存 Ontology
- **THEN** 服务返回 `409 ontology_entity_id_conflict`
- **AND** 不改变任一 Ontology

### Requirement: 创建和读取 Ontology

系统 SHALL 提供 `POST /v1/ontologies` 与 `GET /v1/ontologies/{ontology_id}`。创建输入 MUST 只包含 `name`、`display_name` 与可选 `description`；服务 MUST 创建空聚合。

#### Scenario: 创建 Ontology

- **WHEN** 客户端提交合法创建请求且名称未被使用
- **THEN** 服务返回 `201 Created`、新的 Ontology UUIDv7 与完整空聚合
- **AND** 响应包含资源 `Location` 与当前强 `ETag`

#### Scenario: 创建重名 Ontology

- **WHEN** 创建请求违反部署范围内的 Ontology name 唯一性
- **THEN** 服务返回 `409 ontology_name_conflict`
- **AND** 不创建资源

#### Scenario: 读取现有 Ontology

- **WHEN** 客户端读取一个现有 Ontology
- **THEN** 服务返回 `200 OK`、完整聚合与当前强 `ETag`

#### Scenario: 读取不存在的 Ontology

- **WHEN** 路径 Ontology 不存在
- **THEN** 服务返回 `404 ontology_not_found`

### Requirement: 分页列出 Ontology

系统 SHALL 提供 `GET /v1/ontologies`，并返回 `{ "data": [...], "pagination": { "page", "per_page", "total" } }`。每项 `data` MUST 只包含 `id`、`name`、`display_name`、可选且省略空值的 `description`、`created_at` 与 `updated_at`。`page` MUST 默认为 1；`per_page` MUST 默认为 20 且只接受 1–100；`sort` MUST 默认为 `-updated_at`，并只接受 `name`、`display_name`、`created_at`、`updated_at` 及其 `-` 降序形式。相同排序值 MUST 按 `id` 升序稳定排序。

#### Scenario: 使用默认分页与排序

- **WHEN** 列表请求未提供查询参数
- **THEN** 服务使用 `page=1`、`per_page=20` 与 `sort=-updated_at`
- **AND** 返回摘要、真实 `total` 与 RFC 3339 UTC 时间戳

#### Scenario: 使用显式分页与排序

- **WHEN** 列表请求提供合法页码、每页数量及支持的排序值
- **THEN** 服务返回对应稳定页面且 `-` 前缀表示降序

#### Scenario: 请求结果范围之外的页面

- **WHEN** `page` 超过结果范围
- **THEN** 服务返回 `200 OK`、空 `data` 与真实 `total`

#### Scenario: 查询参数无效

- **WHEN** 页码、每页数量或排序值不符合约束
- **THEN** 服务返回 `400 invalid_request`

### Requirement: 条件式原子替换 Ontology

系统 SHALL 通过 `PUT /v1/ontologies/{ontology_id}` 将请求体作为完整目标状态处理。请求 MUST 提供恰好一个强 `If-Match`；其值 MUST 与该路径资源当前响应的 canonical ETag 相同，路径 ID 与文档 ID MUST 相同。服务 MUST 在写入前校验完整 Candidate，并将 schema 与 Canvas Layout 原子替换。成功替换 MUST 永久删除 Candidate 中省略的旧子实体，每次成功 PUT（包括语义内容未变化的 PUT）MUST 推进内部 revision 与 `updated_at`。

#### Scenario: 成功替换

- **WHEN** 路径 ID 与文档 ID 一致、ETag 当前且 Candidate 合法
- **THEN** schema 与 Canvas Layout 作为一个整体保存
- **AND** 服务返回无响应体的 `204 No Content` 与新的强 `ETag`

#### Scenario: 永久删除省略的子实体

- **WHEN** 合法 Candidate 省略旧 Property、Link Type、Canvas Position 或 Object Type
- **THEN** 成功替换后这些实体被永久删除

#### Scenario: Candidate 保留悬空引用

- **WHEN** Candidate 删除 Object Type 但仍由 Link Type 或 Canvas Position 引用它
- **THEN** 服务返回 `422 invalid_ontology_schema`
- **AND** 聚合与 ETag 均不改变

#### Scenario: 缺少条件头

- **WHEN** PUT 缺少 `If-Match`
- **THEN** 服务返回 `428 ontology_precondition_required`

#### Scenario: 条件头格式无效

- **WHEN** `If-Match` 不符合 HTTP entity-tag 语法、是弱 ETag、通配符或多个值
- **THEN** 服务返回 `400 invalid_request`

#### Scenario: 条件已过期

- **WHEN** 一个格式正确的单一强 ETag 与路径资源当前 canonical ETag 不同，包括来自其他资源或非 canonical opaque 值
- **THEN** 服务返回 `412 ontology_precondition_failed`
- **AND** 聚合、revision 与 ETag 均不改变

#### Scenario: 路径与正文 ID 不同

- **WHEN** 路径 Ontology ID 与正文 ID 不一致
- **THEN** 服务返回 `400 invalid_request`

#### Scenario: 替换导致 Ontology 名称冲突

- **WHEN** 替换会违反部署范围内的 Ontology name 唯一性
- **THEN** 服务返回 `409 ontology_name_conflict`
- **AND** 原聚合与 ETag 均不改变

#### Scenario: 重复保存相同文档

- **WHEN** 客户端使用当前 ETag 再次 PUT 与已保存状态语义相同的合法文档
- **THEN** 服务返回 `204 No Content` 与不同的新强 `ETag`

### Requirement: 条件式永久删除 Ontology

系统 SHALL 通过 `DELETE /v1/ontologies/{ontology_id}` 永久删除完整聚合，并采用与 PUT 相同的强 `If-Match` 规则。

#### Scenario: 成功删除

- **WHEN** Ontology 存在且 `If-Match` 当前
- **THEN** 服务返回无响应体的 `204 No Content`
- **AND** Ontology、全部子实体与 Canvas Layout 均不可再读取

#### Scenario: 删除条件缺失或过期

- **WHEN** DELETE 的 `If-Match` 缺失或已过期
- **THEN** 服务分别返回 `428 ontology_precondition_required` 或 `412 ontology_precondition_failed`
- **AND** 不删除资源

#### Scenario: 删除不存在的 Ontology

- **WHEN** 请求具有格式正确的 `If-Match` 但路径 Ontology 不存在
- **THEN** 服务返回 `404 ontology_not_found`

### Requirement: 候选图校验与资源限制

系统 SHALL 拒绝未知字段。结构上无法解析的 JSON、缺失的必填字段、显式 `null` 可选字段、错误 JSON 类型、非法 UUIDv7 或 enum MUST 返回 `400 invalid_request`。结构正确但违反名称、文本、引用、坐标、唯一性或数量规则的请求 MUST 返回 `422 invalid_ontology_schema`。

`name` MUST 匹配 `^[a-z][a-z0-9_]{0,63}$`；`display_name` MUST 包含 1–200 个 Unicode scalar values；存在的 `description` MUST 包含 1–2,000 个 Unicode scalar values。Property `value_type` MUST 是 `string`、`integer`、`number`、`boolean`、`date` 或 `date_time`；两个 Link cardinality MUST 是 `one` 或 `many`。一个 Ontology MUST 不超过 500 个 Object Types、每个 Object Type 100 个 Properties、总计 10,000 个 Properties、2,000 个 Link Types与 500 个 Canvas Positions。每个 Object Type MUST 至多一个位置，且坐标 MUST 为有限数。

领域 violation code 与 JSON Pointer 归属 MUST 固定如下。`{i}`/`{j}` 是 Candidate array index；重复项的 violation 指向按 Candidate 顺序出现的第二项及后续项：

| Code | Trigger | Path template |
| --- | --- | --- |
| `invalid_ontology_name` | Ontology name 不符合规则 | `/name` |
| `invalid_object_type_name` | Object Type name 不符合规则 | `/object_types/{i}/name` |
| `invalid_property_name` | Property name 不符合规则 | `/object_types/{i}/properties/{j}/name` |
| `invalid_link_type_name` | Link Type name 不符合规则 | `/link_types/{i}/name` |
| `invalid_display_name` | 任一 display name 长度不合法 | 对应实体的 `/display_name` |
| `invalid_description` | 任一存在的 description 长度不合法 | 对应实体的 `/description` |
| `too_many_object_types` | Object Type 数量超过 500 | `/object_types` |
| `too_many_properties` | 单个 Object Type 的 Property 数量超过 100 | `/object_types/{i}/properties` |
| `too_many_total_properties` | Ontology 的 Property 总数超过 10,000 | `/object_types` |
| `too_many_link_types` | Link Type 数量超过 2,000 | `/link_types` |
| `too_many_canvas_positions` | Position 数量超过 500 | `/canvas/positions` |
| `duplicate_object_type_id` | Candidate 内重复 Object Type ID | `/object_types/{i}/id` |
| `duplicate_property_id` | Candidate 内重复 Property ID | `/object_types/{i}/properties/{j}/id` |
| `duplicate_link_type_id` | Candidate 内重复 Link Type ID | `/link_types/{i}/id` |
| `duplicate_object_type_name` | Ontology 内重复 Object Type name | `/object_types/{i}/name` |
| `duplicate_property_name` | 同一 owner 内重复 Property name | `/object_types/{i}/properties/{j}/name` |
| `duplicate_link_type_name` | Ontology 内重复 Link Type name | `/link_types/{i}/name` |
| `unknown_link_source_object_type` | Link source 不在 Candidate | `/link_types/{i}/source_object_type_id` |
| `unknown_link_target_object_type` | Link target 不在 Candidate | `/link_types/{i}/target_object_type_id` |
| `duplicate_canvas_position` | 同一 Object Type 有多个 Position | `/canvas/positions/{i}/object_type_id` |
| `unknown_canvas_object_type` | Position owner 不在 Candidate | `/canvas/positions/{i}/object_type_id` |
| `non_finite_canvas_x` | x 不是有限数 | `/canvas/positions/{i}/x` |
| `non_finite_canvas_y` | y 不是有限数 | `/canvas/positions/{i}/y` |

#### Scenario: Candidate 同时违反多项规则

- **WHEN** 结构正确的 Candidate 存在一个或多个领域 violation
- **THEN** 服务返回 `422` 与 `{ "error": { "code": "invalid_ontology_schema", "message": "ontology schema is invalid", "violations": [...] } }`
- **AND** 每项包含稳定 `code`、RFC 6901 JSON Pointer `path` 与安全 `message`
- **AND** 列表按 `path`、再按 `code` 排序
- **AND** 不产生写入且 ETag 不变

#### Scenario: 请求体超过限制

- **WHEN** POST 或 PUT 请求体超过 2 MiB
- **THEN** 服务在 JSON 解码前返回 `413 ontology_payload_too_large`

#### Scenario: JSON 边界输入无效

- **WHEN** 请求包含未知字段、缺失字段、非法 JSON 类型、非法 ID 或非法 enum
- **THEN** 服务返回 `400 invalid_request`

#### Scenario: 创建输入违反值约束

- **WHEN** POST 正文结构正确但名称或文本长度违反领域规则
- **THEN** 服务返回 `422 invalid_ontology_schema` 与确定性 violation
- **AND** 不创建 Ontology

### Requirement: 错误协议与依赖失败

系统 SHALL 扩展现有共享 `ErrorResponse`：所有错误使用 `{ "error": { "code", "message" } }`；只有 `422 invalid_ontology_schema` MUST 额外包含必需的 `violations` array，其他响应 MUST 省略该字段。安全可展示的 `message` MUST NOT 成为客户端控制流契约。现存子实体 ID 冲突 MUST 映射为 `409 ontology_entity_id_conflict`；持久化依赖不可用 MUST 映射为 `503 ontology_store_unavailable`；未映射内部失败 MUST 映射为 `500 internal_error`，且不得暴露数据库细节、凭据或候选内容。

#### Scenario: 非校验错误省略 violations

- **WHEN** 服务返回 400、404、409、412、413、428、500 或 503
- **THEN** 响应使用共享 `ErrorResponse` 且 `error.violations` 不存在

#### Scenario: PostgreSQL 不可用

- **WHEN** Ontology 持久化依赖无法完成请求
- **THEN** 服务返回 `503 ontology_store_unavailable`

#### Scenario: 未预期内部失败

- **WHEN** 发生未映射的内部错误
- **THEN** 服务返回 `500 internal_error`
- **AND** 响应不包含内部敏感信息

### Requirement: 元数据端点具有完整 OpenAPI 描述

每个 Ontology handler SHALL 位于 utoipa OpenAPI 的 `Ontology` tag 下。所有请求、成功响应、错误 envelope、violation、分页值、enum、类型化 ID 与响应 header MUST 在生成的 OpenAPI 中声明；`204` 响应 MUST 不声明 body。

#### Scenario: 检查生成的 Ontology OpenAPI

- **WHEN** 客户端读取生成的 OpenAPI 文档
- **THEN** 创建、列表、读取、替换与删除 operation 及其实际状态码均存在
- **AND** `Location`、`ETag`、约束与 schema 均可查阅
