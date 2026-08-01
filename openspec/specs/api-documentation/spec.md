# api-documentation Specification

## Purpose

定义 Stratum HTTP/事件协议文档的权威来源：utoipa 从代码注解生成 OpenAPI，覆盖全部端点、DTO、wire 类型与错误响应；Swagger UI 与 openapi.json 随服务提供。

## Requirements


### Requirement: OpenAPI 为协议文档唯一权威

The project SHALL generate its HTTP/event protocol documentation from utoipa annotations. `docs/PROTOCOL.md` MUST carry a deprecation notice pointing to the OpenAPI output. 所有 HTTP 端点 MUST 有 `#[utoipa::path]` 注解，所有穿越边界的 DTO 与 wire 类型 MUST 有 `ToSchema` schema。

#### Scenario: 新增端点

- **WHEN** 新增或修改 HTTP handler
- **THEN** 该 handler 有 `#[utoipa::path]`，其请求/响应/错误类型均有 `ToSchema`，OpenAPI JSON 同步反映

#### Scenario: PROTOCOL.md 废弃标注

- **WHEN** 任何人打开 `docs/PROTOCOL.md`
- **THEN** 文件头部明确标注已废弃，并指向 OpenAPI 输出位置

### Requirement: OpenAPI 可访问性

The service SHALL expose the generated OpenAPI document at `/api-docs/openapi.json` and an interactive Swagger UI at `/swagger-ui`. 两者 SHALL 随 router 装配自动可用，不需要额外进程。

#### Scenario: 获取 OpenAPI JSON

- **WHEN** 服务运行中请求 `GET /api-docs/openapi.json`
- **THEN** 返回覆盖全部 11 个端点的 OpenAPI 3.x JSON

#### Scenario: 事件 envelope 可查阅

- **WHEN** 查看 SSE 端点的文档
- **THEN** `text/event-stream` 响应以 `StreamEnvelope` 为 data 帧 schema，事件类型（RuntimeEvent / AgentEvent 等）在 components 中可查

### Requirement: 错误响应文档化

Each documented endpoint SHALL declare the error status codes it can actually return, with `ErrorResponse` as the body schema.

#### Scenario: handler 错误映射

- **WHEN** 某 handler 可返回 404 / 409 等错误（依 `error_response()` 映射）
- **THEN** 其 `#[utoipa::path]` responses 中包含对应状态码与 `ErrorResponse` body
