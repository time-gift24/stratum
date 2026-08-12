# api-documentation Specification

## Purpose

定义 Stratum HTTP/事件协议文档的权威来源：utoipa 从代码注解生成 OpenAPI，覆盖全部端点、DTO、wire 类型与错误响应；Swagger UI 与 openapi.json 随服务提供。
## Requirements
### Requirement: OpenAPI 为协议文档唯一权威

The project SHALL generate its HTTP/event protocol documentation from utoipa annotations. `docs/PROTOCOL.md` 已随本 change 删除，不再存在任何手写协议文档；OpenAPI 输出是唯一权威。所有 HTTP 端点 MUST 有 `#[utoipa::path]` 注解，所有穿越边界的请求、响应、错误、SSE frame 与 product event 类型 MUST 有 `ToSchema` schema。

OpenAPI 必须（SHALL）只把运行态资源记录为 AgentRuntime：`POST /v1/agent-runtimes`、`GET /v1/agent-runtimes/{agent_runtime_id}`、`POST /v1/agent-runtimes/{agent_runtime_id}/messages`、`GET /v1/agent-runtimes/{agent_runtime_id}/history`、`GET /v1/agent-runtimes/{agent_runtime_id}/events`、`POST /v1/agent-runtimes/{agent_runtime_id}/resume`、`POST /v1/agent-runtimes/{agent_runtime_id}/cancel` 与 `POST /v1/agent-runtimes/{agent_runtime_id}/approvals/{approval_id}`。连同 `GET /v1/agent-templates`、`GET /v1/models`、`GET /health/live` 与 `GET /health/ready`，文档必须（SHALL）覆盖全部 12 个端点。`/v1/agents/{agent_id}` 只保留给未来 immutable template-definition resource，本 change 不得（SHALL NOT）把它文档化为已实现 endpoint。

create 的 `201 Created` schema 必须（SHALL）是不可变 `AgentRuntimeCreated`，只包含 `agent_runtime_id`、pinned `agent_id`、`agent_name`、`agent_version` 与 runtime `created_at`，并声明 `Location: /v1/agent-runtimes/{agent_runtime_id}`。view 的 `200 OK` schema 必须（SHALL）是 `AgentRuntimeView`，显式包含 runtime identity、pinned definition identity/metadata、status、`model_config`、nullable Session/current Turn、`snapshot_event_seq`、`telemetry_floor_event_seq`、pending approvals、latest usage 与 advisory `resume_required`。message 与 new resume 的 `202 Accepted` response 必须（SHALL）包含 exact `agent_runtime_id`、pinned `agent_id`、`session_id` 与 `turn_id`；cancel signal 的 `202` 以及 already-hosted/starting resume、exact already-cancelled 和 approval first/same retry 的 `204` 必须（SHALL）显式声明空 body `()`。所有公开 event sequence 字段必须（SHALL）以十进制字符串记录在 schema 中。

#### Scenario: 新增端点

- **WHEN** 新增或修改 HTTP handler
- **THEN** 该 handler 有 `#[utoipa::path]`，其请求/响应/错误类型均有 `ToSchema`，OpenAPI JSON 同步反映 exact AgentRuntime route、成功码和 body

#### Scenario: AgentRuntime DTO 不混淆 Template Identity

- **WHEN** 调用方查看 create、view 或 command schema
- **THEN** `agent_runtime_id` 标识 runtime aggregate，`agent_id` 只标识 immutable `agents` row，schema 中不存在独立的 definition-version ID，也不把 AgentId 当运行态 route key

#### Scenario: 不存在手写协议文档

- **WHEN** 任何人查找 `docs/PROTOCOL.md`
- **THEN** 该文件不存在，协议以 `/api-docs/openapi.json` 与 Swagger UI 为准

### Requirement: OpenAPI 可访问性

The service SHALL expose the generated OpenAPI document at `/api-docs/openapi.json` and an interactive Swagger UI at `/swagger-ui`. 两者 SHALL 随 router 装配自动可用，不需要额外进程。

#### Scenario: 获取 OpenAPI JSON

- **WHEN** 服务运行中请求 `GET /api-docs/openapi.json`
- **THEN** 返回覆盖上述全部 12 个端点及其 AgentRuntime request、success 与 error schemas 的 OpenAPI 3.x JSON

#### Scenario: AgentRuntime 事件 Frame 可查阅

- **WHEN** 查看 `GET /v1/agent-runtimes/{agent_runtime_id}/events` 的文档
- **THEN** `text/event-stream` 响应以 `AgentRuntimeStreamFrameV1` 为 data 帧 schema，control/durable/telemetry 变体与 `AgentRuntimeProductEventV1` 在 components 中可查；所有 frame 都声明 exact `agent_runtime_id` 与 pinned `agent_id`，Turn-scoped frame 还声明 Session/Turn identity

### Requirement: 错误响应文档化

Each documented endpoint SHALL declare the error status codes it can actually return, with `ErrorResponse` as the body schema. Error response 必须（SHALL）使用安全 envelope `{"error":{"code":"...","message":"..."}}`，不得（SHALL NOT）暴露 SQL、NATS subject、filesystem host path、prompt、Tool arguments/result、provider 正文或 credential。空成功响应不得（SHALL NOT）错误引用 `ErrorResponse` 或 JSON body。

AgentRuntime routes 的 missing resource 必须（SHALL）记录为 `404 agent_runtime_not_found`，create catalog lookup 为 `404 agent_template_not_found`，approval lookup 为 `404 approval_not_found`。文档必须（SHALL）按实际 handler 映射记录 `409 stale_turn`、`agent_runtime_busy`、`agent_version_conflict`、resume/session/turn/approval/runtime conflicts，`410 cursor_expired`，`413 request_too_large`，`422 invalid_agent_version` 及 invalid template/model parameters，`500 durable_state_corrupt`/internal error，以及 `503` store/runtime/realtime/shutdown unavailable。存在 `agent_states` row 但 pinned `agents` row 缺失、metadata 不一致或 definition 严格解码失败时，schema 和示例必须（SHALL）表达 `500 durable_state_corrupt`，不得（SHALL NOT）伪装成任何 404。

#### Scenario: Handler 错误映射

- **WHEN** 某 handler 可返回 404、409、410、413、422、500 或 503 中的任一状态
- **THEN** 其 `#[utoipa::path]` responses 包含实际状态码、稳定 typed code 与 `ErrorResponse` body，不声明该 handler 不可能产生的错误

#### Scenario: Durable Definition Pin 损坏

- **WHEN** AgentRuntime view、command 或 recovery 发现 state pin、snapshot AgentId或loaded definition不一致，或durable row属于错误AgentRuntime/Session/Turn
- **THEN** OpenAPI 记录 `500 durable_state_corrupt`，且 error body 不泄露 durable payload 或 template 内容
