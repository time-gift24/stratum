# Proposal: add-utoipa-openapi

## Why

项目宪法 §3 已裁定：API 文档以 utoipa 生成的 OpenAPI 为唯一权威，`docs/PROTOCOL.md` 废弃。当前代码无任何 utoipa 依赖与注解，协议面（11 个端点 + SSE 事件 envelope）只靠代码和一份已废弃的文档描述。本次把现有协议面完整写成 OpenAPI，使宪法条款从"目标态"变为"现状"。

## What Changes

- workspace 引入 `utoipa`（axum_extras + uuid + chrono features）与 `utoipa-swagger-ui`（axum feature）。
- 协议 wire 类型在定义方 crate derive `ToSchema`：`stratum-core`（StreamEnvelope / RuntimeEvent / AgentEvent / SessionEvent / NodeEvent / LlmEvent / ChatMessage 族 / ModelConfig / TokenUsage / HistoryPage / AgentLocation / ApprovalDecision / ID newtypes 等）、`stratum-store`（AgentStatus）、`stratum-llm`（ModelDescriptor）。
- `stratum-api`：11 个 handler 全部加 `#[utoipa::path]`（含 400/404/409/410/413/422/500/503 错误响应映射 `ErrorResponse`）；本 crate DTO derive `ToSchema`；SSE 端点以 `text/event-stream` + StreamEnvelope body 描述。
- router 挂载 `SwaggerUi`（`/swagger-ui`）与 OpenAPI JSON（`/api-docs/openapi.json`）。
- `docs/PROTOCOL.md` 头部标注废弃并指向 OpenAPI 输出（文件暂留，后续 change 再删）。

非目标：
- 不修复 dogfood 发现的其他违规（4xx warn、SIGTERM、分页、单数 templates 路径等）——各自独立 change。
- 不改任何 wire shape / handler 行为，纯注解与文档装配。
- 不为内部概念（TurnRuntimeSnapshot、hook journal）造文档——无 HTTP 面的不进 OpenAPI。

## Capabilities

### New Capabilities

- `api-documentation`: utoipa 生成的 OpenAPI 是 HTTP/事件协议的唯一权威文档；所有端点与 wire 类型必须有 schema；Swagger UI 与 openapi.json 可访问。

### Modified Capabilities

（无——`runtime-event-protocol` 的 wire shape 本身不变，仅新增其文档表达）

## Impact

- 新增依赖：`utoipa`、`utoipa-swagger-ui`（MIT，RustSec 无已知问题；utoipa 系 OpenAPI 主流方案）。
- 修改：`Cargo.toml`（workspace）、`crates/stratum-core`、`crates/stratum-store`、`crates/stratum-llm`、`crates/stratum-api`、`docs/PROTOCOL.md`（废弃标注）。
- CI 无需变更（新增测试随 `cargo test --workspace` 运行）。
