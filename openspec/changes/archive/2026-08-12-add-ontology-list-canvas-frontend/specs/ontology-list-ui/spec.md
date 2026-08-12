# Spec: ontology-list-ui

Ontology 列表页：分页浏览、新建、删除、进入编辑器。数据契约遵循 `docs/ontology/API.md`。

## ADDED Requirements

### Requirement: 分页列表

系统 SHALL 通过 `GET /v1/ontologies` 以分页方式展示 Ontology 摘要行（`id`、`name`、`display_name`、`description`、`created_at`、`updated_at`），默认排序 `-updated_at`，每页数量 MUST 在 1–100 之间且默认为 20。系统 SHALL 提供翻页控件并展示 `pagination` 返回的总数信息。

#### Scenario: 打开列表页

- **WHEN** 用户进入 Ontology 列表路由
- **THEN** 系统请求第一页摘要并以列表展示，包含名称、显示名、描述与更新时间

#### Scenario: 翻页

- **WHEN** 用户点击下一页或上一页
- **THEN** 系统携带对应 `page` 与 `per_page` 重新请求并替换列表内容

#### Scenario: 列表为空

- **WHEN** 请求成功且 `data` 为空数组
- **THEN** 系统展示空态提示与新建入口，不展示虚构示例数据

#### Scenario: 列表请求失败

- **WHEN** 请求返回 5xx 或网络错误
- **THEN** 系统展示错误态与重试入口，保留当前路由不跳转

### Requirement: 新建 Ontology

系统 SHALL 提供新建入口，收集 `name`（MUST 匹配 `^[a-z][a-z0-9_]{0,63}$`，客户端先行校验）、`display_name` 与可选 `description`，通过 `POST /v1/ontologies` 创建。创建成功后系统 MUST 使用响应返回的资源文档与 `ETag` 初始化编辑器状态并跳转到编辑器页。

#### Scenario: 新建成功

- **WHEN** 用户提交合法的新建表单
- **THEN** 系统 POST 创建成功（201）后跳转至该 Ontology 的编辑器页，编辑器初始 acknowledged 即为响应文档与 ETag

#### Scenario: 名称格式非法

- **WHEN** 用户提交的 `name` 不匹配命名正则
- **THEN** 系统在表单内联提示，不发起请求

#### Scenario: 名称冲突

- **WHEN** POST 返回 409 `ontology_name_conflict`
- **THEN** 系统在名称字段展示冲突提示，表单内容保留

### Requirement: 删除 Ontology

系统 SHALL 在删除前要求用户显式确认，删除时 MUST 先读取目标资源的当前 `ETag`，再携带 `If-Match` 发起 `DELETE /v1/ontologies/{id}`。删除成功后 MUST 将该项从列表移除或刷新当前页。

#### Scenario: 确认删除

- **WHEN** 用户在确认对话框中确认删除且 DELETE 返回 204
- **THEN** 列表移除该 Ontology 并展示成功反馈

#### Scenario: 取消删除

- **WHEN** 用户在确认对话框中取消
- **THEN** 不发起任何写请求，列表保持不变

#### Scenario: 删除时资源已被修改

- **WHEN** DELETE 返回 412 `ontology_precondition_failed`
- **THEN** 系统提示资源已被他人修改，刷新列表后由用户重新决定

### Requirement: 导航入口

系统 SHALL 在站点导航中提供 Ontology 列表页入口，并从列表项提供进入对应画布编辑器的导航。对话页与白板页的既有导航行为 MUST 不受影响。

#### Scenario: 从导航进入列表

- **WHEN** 用户点击导航中的 Ontology 入口
- **THEN** 系统进入列表页，且对话与白板入口保持可用

#### Scenario: 从列表进入编辑器

- **WHEN** 用户点击某个列表项
- **THEN** 系统跳转到该 Ontology 的画布编辑器路由
