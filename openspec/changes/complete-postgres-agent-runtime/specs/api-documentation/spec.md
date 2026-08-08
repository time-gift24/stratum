# api-documentation Specification

## MODIFIED Requirements

### Requirement: OpenAPI 为协议文档唯一权威

The project SHALL generate its HTTP/event protocol documentation from utoipa annotations. `docs/PROTOCOL.md` 已随本 change 删除，不再存在任何手写协议文档；OpenAPI 输出是唯一权威。所有 HTTP 端点 MUST 有 `#[utoipa::path]` 注解，所有穿越边界的 DTO 与 wire 类型 MUST 有 `ToSchema` schema。

#### Scenario: 新增端点

- **WHEN** 新增或修改 HTTP handler
- **THEN** 该 handler 有 `#[utoipa::path]`，其请求/响应/错误类型均有 `ToSchema`，OpenAPI JSON 同步反映

#### Scenario: 不存在手写协议文档

- **WHEN** 任何人查找 `docs/PROTOCOL.md`
- **THEN** 该文件不存在，协议以 `/api-docs/openapi.json` 与 Swagger UI 为准

### Requirement: OpenAPI 可访问性

The service SHALL expose the generated OpenAPI document at `/api-docs/openapi.json` and an interactive Swagger UI at `/swagger-ui`. 两者 SHALL 随 router 装配自动可用，不需要额外进程。

#### Scenario: 获取 OpenAPI JSON

- **WHEN** 服务运行中请求 `GET /api-docs/openapi.json`
- **THEN** 返回覆盖全部 12 个端点的 OpenAPI 3.x JSON

#### Scenario: 事件 frame 可查阅

- **WHEN** 查看 Agent SSE 端点的文档
- **THEN** `text/event-stream` 响应以 `AgentStreamFrameV1` 为 data 帧 schema，其 control/durable/telemetry 变体与 `AgentProductEventV1` 在 components 中可查
