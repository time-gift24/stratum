# ontology-neighborhood-query Specification

## Purpose

定义供 Ontology 画布聚焦使用的持久化、只读邻域查询 API，并固定双向遍历、诱导子图、顺序与快照一致性语义。

## Requirements

### Requirement: 按跳数读取持久化邻域

系统 SHALL 提供 `GET /v1/ontologies/{ontology_id}/object-types/{object_type_id}/neighborhood`。`depth` MUST 默认为 1，并且只接受 0–5 的整数。

#### Scenario: 使用默认深度

- **WHEN** 邻域请求省略 `depth`
- **THEN** 服务按 `depth=1` 查询并在响应中返回该值

#### Scenario: 深度无效

- **WHEN** `depth` 不是整数或不在 0–5 范围内
- **THEN** 服务返回 `400 invalid_request`

### Requirement: 邻域采用双向遍历与诱导子图

系统 SHALL 从 origin Object Type 开始，将每个 Link Type 视为两个方向均可遍历，并返回至多 `depth` 跳可达的全部 Object Types。返回的 Link Types MUST 是该 Object Type 集合上的完整诱导子图。

#### Scenario: 反向遍历 Link Type

- **WHEN** origin 是某 Link Type 的 target
- **THEN** 该 Link Type 的 source 仍可在允许深度内被访问

#### Scenario: 图中存在环或多条路径

- **WHEN** 一个 Object Type 可通过多条路径或环到达
- **THEN** 响应只包含它一次且遍历不超过指定深度

#### Scenario: 返回诱导 Link Type 子图

- **WHEN** 两个已选 Object Types 之间存在保存的 Link Type
- **THEN** 该 Link Type 被返回，即使它不是发现节点时使用的边

#### Scenario: 深度为零

- **WHEN** `depth=0`
- **THEN** 响应只包含 origin、它的全部 Properties、可选 Canvas Position 与 origin 上的全部 self-Link Types

### Requirement: 邻域是完整有序的只读投影

邻域响应 SHALL 包含 `origin_object_type_id`、`depth`、必需的 `object_types`、`link_types` 与 `canvas.positions`。每个 Object Type MUST 包含其全部 Properties；仅返回已保存的位置。整个响应 MUST 来自同一份持久化 Ontology 状态，各数组 MUST 保留完整资源中的相对顺序。该投影 MUST 无资源 ETag，且不得被表示为可 PUT 的完整 Candidate。

#### Scenario: 返回一致的持久化投影

- **WHEN** 邻域查询成功
- **THEN** 所有节点、Properties、Links 与 Positions 来自同一个已持久化 revision
- **AND** 不包含客户端未保存的 Candidate 修改

#### Scenario: 保持保存顺序

- **WHEN** 响应选择了部分 Object Types、Properties、Link Types 与 Positions
- **THEN** 每个数组保持其在完整已保存 Ontology 中的相对顺序

#### Scenario: 空集合字段仍存在

- **WHEN** 某类邻域结果为空
- **THEN** 对应数组为空但不省略
- **AND** `canvas` 始终存在

#### Scenario: 投影不可作为并发保存基线

- **WHEN** 邻域查询成功
- **THEN** 服务返回 `200 OK` 且不返回资源 `ETag`
- **AND** 响应不包含 Ontology 顶层元数据或 revision

### Requirement: 邻域查询具有稳定错误与 OpenAPI 描述

邻域 handler SHALL 位于 utoipa OpenAPI 的 `Ontology` tag 下，并 MUST 声明 depth 约束、成功投影与全部实际错误响应。

#### Scenario: Ontology 不存在

- **WHEN** 路径 Ontology 不存在
- **THEN** 服务返回 `404 ontology_not_found`

#### Scenario: Origin 不属于该 Ontology

- **WHEN** Ontology 存在但 origin Object Type 不属于它
- **THEN** 服务返回 `404 object_type_not_found`

#### Scenario: 检查生成的邻域 OpenAPI

- **WHEN** 客户端读取生成的 OpenAPI 文档
- **THEN** 邻域 operation、depth 默认值与范围、响应 DTO 及错误 schema 均可查阅
