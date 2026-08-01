# Tasks: add-utoipa-openapi

## 1. 依赖与基础设施

- [x] 1.1 workspace `Cargo.toml` 引入 `utoipa`（axum_extras、uuid、chrono features）与 `utoipa-swagger-ui`（axum feature）；core/store/llm/api 四 crate 接通依赖
- [x] 1.2 宏生成 ID 类型的 ToSchema：扩展 `stratum-macros` 的 `uuid_identity!` / `string_id!` 生成 `impl ToSchema`（string 类型），或按 D2 降级手写

## 2. Wire 类型 schema

- [x] 2.1 `stratum-core`：StreamEnvelope / RuntimeEvent / AgentEvent / SessionEvent / NodeEvent / LlmEvent / ChatMessage 族 / ModelConfig / TokenUsage / HistoryPage / AgentLocation / ApprovalDecision / EventCursor 等 derive `ToSchema`
- [x] 2.2 `stratum-store`：AgentStatus derive `ToSchema`；`stratum-llm`：ModelDescriptor derive `ToSchema`

## 3. stratum-api 注解与装配

- [x] 3.1 `api.rs` 全部 DTO derive `ToSchema`（含 ErrorResponse/ErrorBody）；11 个 handler 加 `#[utoipa::path]`，错误响应按 `error_response()` 实际可达状态码列全
- [x] 3.2 SSE 端点按 D3 描述（text/event-stream + StreamEnvelope + 帧语义说明）
- [x] 3.3 `ApiDoc` OpenApi struct + router merge SwaggerUi（`/swagger-ui` + `/api-docs/openapi.json`）
- [x] 3.4 `docs/PROTOCOL.md` 头部加废弃标注指向 OpenAPI

## 4. 验证与归档

- [x] 4.1 `cargo fmt` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo test --workspace --all-targets` 全绿
- [x] 4.2 启动服务或单测验证 `/api-docs/openapi.json` 输出覆盖 11 端点且 serde wire shape 不变
- [ ] 4.3 `openspec validate --all --strict`；sync spec 并归档；更新 `crates/stratum-api/AGENTS.md`（如存在）记录 OpenAPI 约定
