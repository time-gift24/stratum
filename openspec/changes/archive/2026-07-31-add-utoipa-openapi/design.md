# Design: add-utoipa-openapi

## Context

协议面现状（侦察结论）：11 个 axum handler 集中在 `crates/stratum-api/src/api.rs`，DTO 同文件定义；wire 类型（StreamEnvelope / RuntimeEvent / AgentEvent / HistoryPage / ID newtype 宏生成类型等）在 `stratum-core/src/lib.rs`、`stratum-store/src/state.rs`、`stratum-llm/src/definition.rs`。错误响应为手工 `(StatusCode, Json<ErrorResponse>)`，状态码→错误码映射在 `error_response()`（api.rs:667-832）。SSE 经 `Sse::new(stream)`，data 帧为 StreamEnvelope JSON，cursor 在 SSE `id` 字段。

## Goals / Non-Goals

**Goals:** 宪法 §3 从目标态变现状；OpenAPI 完整覆盖 11 端点 + 事件 envelope；`cargo test --workspace` 全绿。

**Non-Goals:** 修 dogfood 其他违规；改 wire shape；删 PROTOCOL.md（只标注废弃）。

## Decisions

### D1: ToSchema derive 放在类型定义方 crate

`ToSchema` 是 utoipa trait、类型属 core/store/llm——orphan rule 决定无法在 stratum-api 侧为它们实现。utoipa 作为纯注解依赖加入 core/store/llm 三个 crate（不引运行时行为）。被否决方案：api 侧镜像类型（双倍维护、必然漂移）；`#[schema(value_type=...)]` 逐字段覆盖（嵌套类型如 HistoryPage/RuntimeEvent 无法表达）。

### D2: 宏生成 ID 类型的 schema

`uuid_identity!` / `string_id!` 宏生成 `#[serde(transparent)]` newtype。在宏中同步生成 `impl ToSchema`（stratum-macros 输出 `utoipa` 路径的 impl，宏本身不依赖 utoipa——由调用方 crate 提供 utoipa）。newtype 映射为 `string`（uuid 格式或 plain）。若宏改动复杂，降级方案：在各 ID 类型旁手写 `impl ToSchema`，约 10 处。

### D3: SSE 端点的 OpenAPI 表达

utoipa 对 `text/event-stream` 支持弱。处理：`#[utoipa::path]` 的 200 响应 `content_type = "text/event-stream"`、`body = StreamEnvelope`，description 中说明 SSE 帧语义（id=cursor、event=内层事件类型名、data=StreamEnvelope JSON、Last-Event-ID/after_cursor/replay 参数、stream_error 合成事件）。

### D4: 错误响应

`ErrorResponse`/`ErrorBody` derive ToSchema；每个 handler 的 responses 按 `error_response()` 实际映射列状态码（400/404/409/410/413/422/500/503 中该 handler 可达的子集）。

### D5: Swagger UI 挂载

`SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi())`，merge 进现有 router；CORS/BodyLimit 不受影响。文档端点不进 `/v1` 前缀。

## Risks / Trade-offs

- [utoipa 版本与 axum 0.8 兼容性] → 选用 utoipa 5.x（axum_extras feature 支持 axum 0.8）；实现时先 `cargo add` 验证版本解析。
- [serde tag/content 枚举的 ToSchema 生成质量] → utoipa 5 支持 internally/adjacently tagged enum；若某类型生成失败，对该类型手写 `impl ToSchema`/`ToSchema::schema()`，不阻塞整体。
- [swagger-ui 静态资源增大二进制] → 接受，文档权威的必要成本。

## Migration Plan

纯增量注解，无行为变更，无需迁移。回滚 = revert。

## Open Questions

（无）
