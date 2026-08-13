## Why

Stratum 需要一个独立、可复用的 Ontology 元数据后端，先为画布提供稳定协议，后续再由 Memory 等模块按 ID 消费。当前领域模型和 HTTP 契约已经完成决策，需要把它们收敛为可实施、可验证的 OpenSpec 变更。

## What Changes

- 新增 Ontology 聚合的创建、列表、读取、整图条件替换与条件删除 API，使用强 ETag 和 `If-Match` 防止静默覆盖。
- 新增 Object Type、其独占 Property、二元 Link Type 与 Canvas Position 的完整候选图校验和 PostgreSQL 原子持久化。
- 新增以 Object Type 为原点、按 0–5 跳双向遍历的持久化邻域查询，供画布聚焦和后续非编辑器调用方使用。
- 新增稳定的错误码、RFC 6901 violation 路径、资源限制及 utoipa OpenAPI 定义。
- 补齐生产进程的 liveness 与依赖感知 readiness，使 NATS、Agent Store 或 Ontology PostgreSQL 不可用时可被编排平台摘流。
- 新增独立的 `stratum-ontology` crate；`stratum-api` 仅负责 HTTP DTO、协议映射和进程装配。
- **BREAKING**：`stratum-api` 启动配置新增必需的 Ontology PostgreSQL URL；示例、Docker 与测试配置需要同步提供该字段。
- 本 change 只交付后端，不实现或验收画布。现有 `add-ontology-list-canvas-frontend` change 继续独立推进并消费本 change 固定的 API 契约，不被本 change 取代；此前也不存在需要取代的 Ontology 后端 change。
- 非目标包括对象实例、Memory 集成、版本/历史/分支、认证授权、多租户、共享 Property、Interface、Action、Group、物理数据绑定、实时协作及独立微服务进程。

## Capabilities

### New Capabilities

- `ontology-metadata-management`: 定义 Ontology 完整文档、生命周期 API、确定性校验、条件写入、持久化一致性及协议错误行为。
- `ontology-neighborhood-query`: 定义从指定 Object Type 出发的 N 跳只读持久化子图查询。

### Modified Capabilities

- `api-documentation`: 移除 OpenAPI 覆盖端点数量的硬编码，要求文档覆盖 router 中全部已挂载 operation，以容纳新增 Ontology API。

## Impact

- 新增 `crates/stratum-ontology` 及其 PostgreSQL migration、查询、事务、单元测试和独立容器集成测试资产。
- 修改 workspace 依赖、`stratum-config` 的 Ontology 数据库配置、`stratum-api` 的状态装配、router、handler、错误映射和 utoipa schema。
- OpenAPI 将新增 `/v1/ontologies` 资源端点与邻域端点，并成为前端实现的协议权威。
- PostgreSQL 新增五张规范化表；不新增 JSONB 镜像、Repository trait、RPC/微服务边界或版本快照。
