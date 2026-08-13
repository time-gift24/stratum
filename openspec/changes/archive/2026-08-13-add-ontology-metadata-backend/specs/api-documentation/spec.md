## MODIFIED Requirements

### Requirement: OpenAPI 可访问性

The service SHALL expose the generated OpenAPI document at `/api-docs/openapi.json` and an interactive Swagger UI at `/swagger-ui`. 两者 SHALL 随 router 装配自动可用，不需要额外进程。

#### Scenario: 获取 OpenAPI JSON

- **WHEN** 服务运行中请求 `GET /api-docs/openapi.json`
- **THEN** 返回覆盖 router 中全部已挂载 HTTP operation 的 OpenAPI 3.x JSON

#### Scenario: 事件 envelope 可查阅

- **WHEN** 查看 SSE 端点的文档
- **THEN** `text/event-stream` 响应以 `StreamEnvelope` 为 data 帧 schema，事件类型（RuntimeEvent / AgentEvent 等）在 components 中可查
