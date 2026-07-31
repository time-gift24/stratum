# CONSTITUTION.md — Axum Web Service 项目宪法（示例模板）

> **这是 constitution-review skill 附带的起步示例，不是审查依据。**
> 复制到你的项目根目录、按项目实际技术栈与规范修改后，才会被审查流程读取。
> 技术栈：Rust + Axum + Tower + SQLx/SeaORM + Utoipa + Tracing + Metrics + OpenTelemetry

---

## 1. 架构分层（强制）

项目必须采用以下分层结构，禁止跨层调用：

```
crates/
├── api/              # HTTP 层：Handler + Router + Middleware + DTO
├── service/          # 业务层：用例编排、事务边界、领域逻辑
├── domain/           # 领域层：Entity、Value Object、Domain Error、Repository Trait
├── infrastructure/   # 基础设施层：DB 实现、外部 HTTP 客户端、消息队列、缓存
└── shared/           # 共享层：Error 类型、工具函数、常量
```

### 依赖规则（DAG）
- `api` → `service` → `domain` ← `infrastructure`
- `shared` 可被任何层依赖，但**不得**依赖其他层
- `domain` **零外部依赖**（除 `thiserror`、`serde`、`uuid` 等纯数据 crate）

### Axum 路由组织
```rust
// api/src/routes/mod.rs
pub fn router() -> Router<AppState> {
    Router::new()
        .nest("/api/v1/users", users::router())
        .nest("/api/v1/orders", orders::router())
        .merge(health::router())  // /health/* 不需要版本前缀
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
}
```

---

## 2. 错误处理（强制）

### 分层策略
| 层级 | 工具 | 用途 |
|------|------|------|
| `domain` | `thiserror` | 精确错误类型，带结构化字段 |
| `service` | `thiserror` | 业务错误，可包含 `#[from] DomainError` |
| `api` | `thiserror` + `IntoResponse` | 统一 HTTP 响应映射 |
| 临时/脚本 | `anyhow` | 仅允许在测试、CLI、迁移中使用 |

### 错误类型规范
```rust
// domain/src/error.rs
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("实体未找到: {entity} id={id}")]
    NotFound { entity: &'static str, id: Uuid },

    #[error("业务规则冲突: {rule}")]
    BusinessRuleViolated { rule: &'static str },

    #[error("并发冲突，请重试")]
    Conflict,
}

// api/src/error.rs — 唯一对外暴露的 HTTP 错误
#[derive(Debug, Error)]
pub enum AppError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error("验证失败: {0}")]
    Validation(String),

    #[error("内部服务器错误")]
    Internal(#[from] anyhow::Error),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            AppError::Domain(DomainError::NotFound { .. }) => {
                (StatusCode::NOT_FOUND, json!({"error": self.to_string()}))
            }
            AppError::Domain(DomainError::Conflict) => {
                (StatusCode::CONFLICT, json!({"error": self.to_string()}))
            }
            AppError::Validation(_) => {
                (StatusCode::UNPROCESSABLE_ENTITY, json!({"error": self.to_string()}))
            }
            _ => {
                tracing::error!(error = %self, "内部错误");
                (StatusCode::INTERNAL_SERVER_ERROR, json!({"error": "内部服务器错误"}))
            }
        };
        (status, Json(body)).into_response()
    }
}
```

### 铁律
- **禁止在 Handler 中使用 `unwrap()` / `expect()`**，必须 `?` 或显式匹配
- **禁止在错误消息中暴露内部路径、SQL、堆栈**（生产环境）
- **所有 5xx 错误必须记录 `tracing::error!`**，所有 4xx 记录 `tracing::warn!`

---

## 3. API 规范（强制）

### OpenAPI / Utoipa
- **每个 Handler 必须有 `#[utoipa::path(...)]` 注解**
- **每个 DTO 必须有 `#[derive(ToSchema)]`**
- **每个响应状态码必须有描述和 `body` 类型**

```rust
#[derive(Debug, Serialize, ToSchema)]
pub struct UserResponse {
    #[schema(example = "550e8400-e29b-41d4-a716-446655440000")]
    pub id: Uuid,
    #[schema(example = "zhangsan", min_length = 2, max_length = 32)]
    pub username: String,
    #[schema(example = "zhangsan@example.com")]
    pub email: String,
    #[schema(value_type = String, format = DateTime)]
    pub created_at: DateTime<Utc>,
}

#[utoipa::path(
    get,
    path = "/api/v1/users/{id}",
    params(
        ("id" = Uuid, Path, description = "用户 ID")
    ),
    responses(
        (status = 200, description = "用户详情", body = UserResponse),
        (status = 404, description = "用户不存在", body = ErrorResponse),
        (status = 500, description = "服务器内部错误", body = ErrorResponse),
    ),
    tag = "用户管理"
)]
pub async fn get_user(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<UserResponse>, AppError> {
    let user = state.user_service.find_by_id(id).await?;
    Ok(Json(user.into()))
}
```

### REST 设计
- URL 使用名词复数：`/users`、`/orders/{id}/items`
- 动作通过 HTTP 方法表达，禁止在 URL 中使用动词：`POST /users`（创建），非 `POST /users/create`
- 分页统一：`GET /users?page=1&per_page=20`，返回 `{"data": [...], "pagination": {"page":1,"per_page":20,"total":100}}`
- 排序：`GET /users?sort=-created_at`（`-` 降序，`+` 或无前缀升序）

---

## 4. 日志与追踪（强制）

### Tracing 规范
```rust
// 初始化（main.rs）
tracing_subscriber::registry()
    .with(
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "info,tower_http=debug,sqlx=warn".into()),
    )
    .with(
        fmt::layer()
            .json()
            .with_current_span(true)
            .with_span_list(true),
    )
    .with(
        tracing_opentelemetry::layer()
            .with_tracer(opentelemetry_otlp::new_pipeline().tracing().install_batch()?),
    )
    .init();
```

### Span 规范
- **每个 HTTP 请求自动创建 span**：`tower_http::trace::TraceLayer`
- **每个 Service 方法手动创建 span**：
```rust
#[tracing::instrument(skip(self), fields(user_id = %user_id), err(Debug))]
pub async fn create_order(&self, user_id: Uuid, cmd: CreateOrderCmd) -> Result<Order, DomainError> {
    // span 自动包含 user_id，err(Debug) 仅在 error 级别记录错误详情
}
```
- **字段命名**：snake_case，避免动态键名
- **敏感数据**：绝对禁止在 span 字段中记录 password、token、credit_card 等

### Metrics 规范
```rust
use metrics::{counter, histogram, gauge};

#[tracing::instrument(skip(self))]
pub async fn process_payment(&self, cmd: PaymentCmd) -> Result<Receipt, DomainError> {
    let start = Instant::now();

    counter!("payment_attempts_total", "currency" => cmd.currency.as_str()).increment(1);

    let result = self.gateway.charge(cmd).await;

    match &result {
        Ok(_) => counter!("payment_success_total", "currency" => cmd.currency.as_str()).increment(1),
        Err(e) => {
            counter!("payment_failures_total",
                "currency" => cmd.currency.as_str(),
                "reason" => e.classify()
            ).increment(1);
        }
    }

    histogram!("payment_duration_seconds").record(start.elapsed().as_secs_f64());

    result
}
```

---

## 5. 数据库（强制）

### SQLx / SeaORM 规范
- **使用 `sqlx::query_as!` 进行编译时检查**，禁止裸字符串 SQL
- **所有查询必须有 `#[tracing::instrument]`**
- **连接池配置必须显式**：
```rust
PgPoolOptions::new()
    .max_connections(20)
    .min_connections(5)
    .acquire_timeout(Duration::from_secs(3))
    .idle_timeout(Duration::from_secs(600))
    .max_lifetime(Duration::from_secs(1800))
    .connect(&database_url)
    .await?;
```
- **迁移使用 `sqlx migrate`**，禁止在生产环境使用 `auto_create`
- **事务边界在 Service 层**：
```rust
pub async fn transfer(&self, from: Uuid, to: Uuid, amount: Decimal) -> Result<(), DomainError> {
    let mut tx = self.pool.begin().await?;

    self.account_repo.debit(&mut tx, from, amount).await?;
    self.account_repo.credit(&mut tx, to, amount).await?;

    tx.commit().await?;
    Ok(())
}
```

---

## 6. 安全（强制）

### 依赖安全
- **CI 必须运行**：`cargo audit --deny warnings` + `cargo deny check`
- **`Cargo.lock` 必须提交到版本控制**
- **定期运行**：`cargo audit fix`（修复后必须跑完全部测试）

### 运行时安全
- **所有外部输入必须验证**：使用 `validator` crate + `#[derive(Validate)]`
- **密码/密钥使用 `secrecy::Secret<String>`**，禁止 `Debug` 暴露
- **CORS 必须显式配置白名单**，禁止 `allow_any_origin()` 上生产
- **Rate Limiting**：使用 `tower::limit::RateLimitLayer` 或 `governor` crate

```rust
#[derive(Debug, Deserialize, Validate)]
pub struct CreateUserRequest {
    #[validate(length(min = 2, max = 32), regex(path = "USERNAME_RE"))]
    pub username: String,
    #[validate(email)]
    pub email: String,
    #[validate(length(min = 8))]
    pub password: Secret<String>,
}
```

---

## 7. 测试（强制）

### 测试金字塔
```
单元测试（70%）→ 集成测试（20%）→ E2E（10%）
```

### 单元测试
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_total_calculates_correctly() {
        let order = Order::builder()
            .items(vec![
                OrderItem::new("SKU-001", 2, dec!(10.00)),
                OrderItem::new("SKU-002", 1, dec!(25.50)),
            ])
            .build();

        assert_eq!(order.total(), dec!(45.50));
    }
}
```

### 集成测试（`tests/` 目录）
```rust
// tests/user_api.rs
use my_app::test_helpers::{spawn_app, TestApp};

#[tokio::test]
async fn create_user_returns_201() {
    let app = spawn_app().await;

    let response = app.client
        .post("/api/v1/users")
        .json(&json!({"username":"test","email":"test@example.com","password":"secure123"}))
        .send()
        .await;

    assert_eq!(response.status(), 201);

    let user: UserResponse = response.json().await;
    assert_eq!(user.username, "test");
}
```

### 测试规范
- **每个测试必须有 Arrange-Act-Assert 三段注释**
- **测试数据使用工厂模式**，禁止硬编码大量重复数据
- **集成测试必须清理数据**：`#[sqlx::test]` 或 `testcontainers`
- **异步测试使用 `#[tokio::test]`**，禁止 `block_on`

---

## 8. 部署与运维（强制）

### 健康检查
```rust
pub fn health_routes() -> Router<AppState> {
    Router::new()
        .route("/health/live", get(|| async { StatusCode::OK }))   // Liveness
        .route("/health/ready", get(ready_check))                  // Readiness
        .route("/metrics", get(metrics_handler))                   // Prometheus
}

async fn ready_check(State(state): State<AppState>) -> StatusCode {
    match state.db_pool.acquire().await {
        Ok(_) => StatusCode::OK,
        Err(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}
```

### 优雅关闭
```rust
let listener = TcpListener::bind("0.0.0.0:8080").await?;
axum::serve(listener, app)
    .with_graceful_shutdown(shutdown_signal())
    .await?;

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("failed to install Ctrl+C handler");
    };
    let terminate = async {
        signal::unix::signal(SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };
    tokio::select! {
        _ = ctrl_c => tracing::info!("收到 Ctrl+C"),
        _ = terminate => tracing::info!("收到 SIGTERM"),
    }
    tracing::info!("开始优雅关闭...");
}
```

### Docker 多阶段构建
```dockerfile
# 阶段 1：构建
FROM rust:1.82-slim-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates/ ./crates/
RUN cargo build --release --bin api

# 阶段 2：运行
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/api /usr/local/bin/api
USER nobody
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/api"]
```

---

## 9. 代码风格（强制）

### 格式化
- `rustfmt.toml`：
```toml
edition = "2021"
max_width = 100
chain_width = 80
fn_params_layout = "Tall"
imports_granularity = "Crate"
group_imports = "StdExternalCrate"
```

### Clippy
- CI 中运行：`cargo clippy --all-targets --all-features -- -D warnings`
- 必须通过的 lint：
  - `clippy::all`
  - `clippy::pedantic`（可局部允许）
  - `clippy::cargo`
  - `unused_imports`, `dead_code`

### 命名规范
| 类型 | 规范 | 示例 |
|------|------|------|
| 模块/文件 | snake_case | `user_service.rs` |
| 结构体/枚举 | PascalCase | `CreateOrderCommand` |
| 函数/方法 | snake_case | `find_by_id` |
| 常量 | SCREAMING_SNAKE_CASE | `MAX_RETRY_COUNT` |
| 特征 | PascalCase + 形容词 | `Authenticatable`, `Serializable` |
| 错误类型 | PascalCase + Error | `PaymentError` |

---

## 10. AI 编码指令（执行优先级最高）

当基于本文件生成或修改代码时，AI 必须：

1. **先检查上下文**：确认当前修改属于哪个 crate/layer，禁止跨层引入依赖
2. **错误处理优先**：每个 `?` 必须确保错误类型已实现 `From<...>` 或已显式映射
3. **日志伴随**：每个 Service 方法必须带 `#[tracing::instrument]`；每个分支必须记录适当级别日志
4. **DTO 隔离**：Handler 的输入/输出必须是独立 DTO，禁止直接返回 Domain Entity
5. **OpenAPI 同步**：新增/修改 Handler 必须同步更新 `#[utoipa::path]` 和 DTO 的 `ToSchema`
6. **测试同步**：新增业务逻辑必须附带单元测试；新增 API 必须附带集成测试
7. **安全扫描**：引入新依赖时，必须说明其许可证和安全性（是否出现在 RustSec）
8. **文档注释**：所有 pub 项必须有 `///` 文档注释，复杂逻辑必须有 `//` 行内注释

---

## 附录：禁止清单（Red Flags）

以下代码在 Review 中必须一票否决：

- [ ] `unwrap()` / `expect()` 出现在非测试代码中
- [ ] `println!` / `eprintln!` 出现在非 CLI 代码中（必须用 `tracing`）
- [ ] 裸 SQL 字符串（无 `sqlx::query!` 编译时检查）
- [ ] 密码/Token 以 `String` 传递（必须用 `Secret`）
- [ ] Handler 直接调用 Repository（必须经 Service 层）
- [ ] 跨 crate 循环依赖
- [ ] `async fn` 中持有 `MutexGuard` 跨越 await 点
- [ ] 未处理的 `Result`（禁止 `let _ = ...` 除非显式注释原因）
- [ ] 在日志/错误消息中暴露敏感数据
- [ ] 生产环境使用 `RUST_LOG=debug` 或 `trace`
