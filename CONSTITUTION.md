# CONSTITUTION.md — Stratum 项目宪法

> 本文件为项目级 AI 编码与审查规范，是 `constitution-review` skill 的审查依据。
> 技术栈：Rust workspace（edition 2024, rust 1.88）+ Axum（仅 stratum-api）+ Tokio + tracing + NATS/文件存储。
> 前端（stratum-web）规范不在本文件范围，见 `PRODUCT.md` 与 `stratum-web/DESIGN.md`。
> Rust 编码的深度规则（265 条）以 `.agents/skills/rust-skills/` 为参考；本宪法收录其中必须强制执行的子集。
> 审查时遇到本文件未覆盖的 Rust 细节问题，参照 rust-skills 检视并归入 `suggestion` 或 `constitution-gap`。

---

## 1. 架构与 crate 分层（强制）

项目按能力分层，依赖方向必须保持 DAG，禁止下层依赖上层：

- **核心层**：`stratum-core`（+ `stratum-macros`）——领域类型、ID newtype、事件、错误、trait 定义；不得依赖任何其他 stratum crate。
- **能力层**：`stratum-filesystem`、`stratum-infra`、`stratum-llm`、`stratum-tools`、`stratum-config`——单一能力，只依赖核心层。
- **组合层**：`stratum-store`、`stratum-agent`——编排能力层，不得被能力层依赖。
- **装配层**：`stratum-agent-builtin`（内置实现）、`stratum-api`（HTTP/进程入口）——最上层；`stratum-api` 是唯一含 `main.rs` 的 crate，`main.rs` 必须保持薄，可复用逻辑放 `lib.rs`。

### crate 内规则

- trait 定义与具体实现分文件；error 定义独立 `error.rs`，使用 `thiserror` derive。
- 共享依赖版本一律走 workspace dependency inheritance，禁止在 crate 内写裸版本号。
- Cargo features 必须是 additive，禁止 feature 互斥或改变已有行为。
- 禁止跨 crate 循环依赖（cargo metadata 可验证）。

---

## 2. 错误处理（强制）

### 分层策略
- library crates：使用 `thiserror` 定义类型化错误，集中在 crate 内 `error.rs`；禁止手写字符串型错误，禁止把 error enum 混在 trait 或实现文件里。
- binary（`stratum-api`）：顶层可用 `anyhow`，但穿过 crate 边界的错误必须是类型化错误。

### 铁律
- 可恢复失败返回 `Result<T, E>`；生产代码禁止 `unwrap()`；`expect()` 仅用于表示程序员错误的不变量。
- 用 `#[source]` 或 `From` 转换保留错误来源链；错误消息小写开头、不加句号。
- 可失败的公共函数必须在文档中写 `# Errors`。
- HTTP 边界（stratum-api）：错误统一映射为 `IntoResponse`；5xx 记录 `tracing::error!`，4xx 记录 `tracing::warn!`；响应体禁止暴露内部路径、堆栈、存储细节。

---

## 3. API 与协议（强制）

- API 文档以 **utoipa 生成的 OpenAPI** 为唯一权威；`docs/PROTOCOL.md` 废弃，不再作为协议依据。
- 每个 Handler 必须有 `#[utoipa::path(...)]` 注解；每个 DTO 必须 `#[derive(ToSchema)]`；每个响应状态码必须有描述和 `body` 类型。
- 事件流端点（`/events`）的 envelope 类型同样必须纳入 `ToSchema`。
- HTTP API 统一 `/v1` 前缀；资源用名词复数（`/v1/agents/{agent_id}`）。
- 非 CRUD 的生命周期操作允许动词子路径（`/v1/agents/{id}/resume`、`/cancel`）；禁止为 CRUD 造动词路径（如 `/agents/create`）。
- Handler 输入/输出必须是独立 DTO，禁止直接暴露领域类型；领域 ID 使用 newtype（`SessionId`、`AgentId`、`TurnId` 等），禁止 stringly typed 穿越边界。

### REST 设计

- 动作用 HTTP 方法表达：GET 读取、POST 创建、PUT/PATCH 更新、DELETE 删除；禁止用 GET 产生副作用。
- 状态码语义正确：创建返回 201，无内容返回 204，错误按 §2 映射为 4xx/5xx。
- 分页统一：`?page=1&per_page=20`，响应包 `{"data": [...], "pagination": {"page", "per_page", "total"}}`；大数据量流式场景允许 cursor 式（`?cursor=...&limit=...`），同一资源二选一并在 OpenAPI 中保持一致。
- 排序统一：`?sort=-created_at`（`-` 降序，无前缀升序）。
- 生命周期动词子路径（`resume`、`cancel`）是上述规则的唯一例外。

---

## 4. 日志与可观测（强制）

- 统一使用 `tracing`；library crate 只通过 tracing/log facade 发事件，禁止安装全局 subscriber（subscriber 只在 `stratum-api` 的 main 中安装）。
- 关键异步操作（turn 执行、LLM 调用、store 读写、hook 调用）使用 `#[tracing::instrument]`，上下文字段带 `session_id` / `agent_id` / `turn_id`。
- 字段命名 snake_case，上下文放 structured fields，禁止拼进消息字符串。
- 敏感数据（LLM API key、token、用户凭据）绝对禁止进入 span 字段和日志消息。
- 错误只在真正处理它的边界记录一次，禁止逐层重复 log。
- 禁止 `println!` / `eprintln!`（CLI 输出场景除外）。
- **Metrics（强制，分阶段落地）**：`metrics` facade 尚未引入仓库（平台任务，见 TODO.md）。facade 落地前，新增关键路径必须（SHALL）先以 tracing 事件覆盖；facade 就绪后，关键业务操作（turn 执行、LLM 调用、hook 调用、store 读写失败）必须记录 counter / histogram；指标名 snake_case，label 禁止高基数值（如 session_id、用户输入）。
- **OpenTelemetry（强制）**：`stratum-api` 必须接入 OTLP exporter，trace 须贯通 HTTP 请求 → turn 执行 → LLM 调用链路。

---

## 5. 存储与事件总线（强制）

- 状态/定义持久化必须经 `stratum-store`，业务 crate 禁止直接读写存储介质。
- 文件系统访问必须经 `stratum-filesystem`，业务 crate 禁止直接使用 `std::fs` / `tokio::fs`。本条约束 agent 可见的业务文件操作；耐久存储后端（`stratum-infra` / `stratum-store`）作为基础设施可以直接使用 `std::fs` / `tokio::fs`，并自行保证崩溃一致性。
- 事件发布/订阅必须经 `stratum-infra` 的 event bus 抽象，业务代码禁止直连 `async-nats`。
- 本节所称"业务 crate"指核心层、能力层、组合层；装配层（`stratum-api`、`stratum-agent-builtin`）只允许在启动装配阶段（加载配置、创建目录、依赖接线）直连基础设施，运行期请求路径上禁止。
- 文件写操作必须崩溃一致：临时文件 + 原子 rename，或 store 层提供的等效保证。append-only 日志在同时满足以下条件时视为等效：写入后做文件与目录双 fsync，读取器容忍截断尾行（含落在多字节 UTF-8 字符中间的撕裂写，按字节解析并丢弃尾部残缺行）。
- 持久化载荷（事件流、Hook journal decision 等）按对话级敏感数据处理：secret、token、用户凭据永远不得写入持久流；保留时间与清理策略由存储后端定义并在其 crate 文档归档。
- 从持久层读回的 `#[non_exhaustive]` 枚举，`_` 分支必须返回错误（fail closed），禁止提供默认值——尤其禁止向放宽权限的方向默认。
- NATS subject / bucket 命名集中定义，禁止散落字符串字面量。
- 持久化 shape 变更必须与协议兼容策略一致：不支持的旧 shape 显式报错，禁止静默吞掉或猜测性迁移。

---

## 6. 安全（强制）

- 密钥（LLM API key、token）在内存中以 `secrecy::Secret` 承载，禁止 `Debug` / `Display` 明文暴露；真实密钥禁止提交入库——仓库只保留 `config.example.toml`，真实值走本地配置或环境变量。
- 所有 HTTP 外部输入在边界校验（utoipa schema + 反序列化校验）；边界数据尽量在反序列化时完成校验。
- CORS 必须显式配置白名单，禁止 `allow_any_origin()` 上生产。
- LLM 出站调用必须显式设置超时；禁止把用户凭据拼入 URL、headers 日志或错误消息。
- 工具执行（`stratum-tools` 沙箱）必须在容器/沙箱边界内运行，禁止在宿主机直接执行 agent 生成的命令。
- 依赖安全：CI 必须运行 `cargo audit` 与 `cargo deny check`；`Cargo.lock` 必须提交到版本控制。

---

## 7. 测试（强制）

- 单元测试放 `#[cfg(test)] mod tests`，位于被测生产代码之后（通常文件末尾）；跨 crate 集成测试放对应 crate 的 `tests/` 目录。
- 测试命名必须描述被验证的行为；结构保持 arrange / act / assert 清晰。
- 异步测试使用 `#[tokio::test]`，禁止 `block_on`。
- agent、LLM、tool、MCP 相关测试使用 mock provider 和基于 trait 的依赖，禁止真实调用外部服务。
- parser、validator、graph scheduling、schema conversion 优先 property tests。
- 需要真实外部依赖（容器）的集成测试必须标记 `#[ignore]`；普通 `cargo test --workspace --all-targets` 不得依赖容器。
- 每个 crate 的容器集成测试使用独立的 `docker-compose.test.yml`，compose project name 用 crate 名加 `-test`；本地经 crate 内 `Makefile` 运行，默认 `podman compose`，需要 Docker 时用 `COMPOSE="docker compose"` 覆盖。
- 禁止为测试方便在生产 API 中添加函数；测试辅助逻辑放测试模块、`tests/` helper 或 fixture。

---

## 8. 部署与运维（强制）

- 镜像构建统一走根 `Dockerfile.rust` 多阶段构建（`PACKAGE` / `BIN` 参数化）；禁止为单个 crate 另写 Dockerfile。
- `stratum-api` 必须实现优雅关闭：处理 SIGTERM / SIGINT，停止接收新请求并 drain 进行中的 turn。
- 必须提供 liveness 健康检查端点；依赖（store、NATS）未就绪时 readiness 必须返回不可用。
- 运行时配置经 `stratum-config` 加载并校验；禁止在代码中硬编码环境相关值（地址、凭据、超时）。
- CI 的 fmt / clippy / test / image 门禁必须保持通过后才允许合入。

---

## 9. 代码风格与 lint（强制）

- lint 配置集中在 workspace `Cargo.toml` 的 `[workspace.lints]`：correctness 为 deny，suspicious / style / complexity / perf 为 warn。
- 提交前必须 `cargo fmt`；CI 运行 `cargo fmt --all -- --check` 与 `cargo clippy --workspace --all-targets -- -D warnings`。
- 禁止无理由 silence lint；确需 `#[allow]` 必须附简短原因注释。
- 命名规范：模块/文件 snake_case，类型 PascalCase，函数 snake_case，常量 SCREAMING_SNAKE_CASE，错误类型 `XxxError`。
- 转换方法前缀：`as_` 为免费借用转换、`to_` 为昂贵转换、`into_` 消耗所有权；布尔方法用 `is_` / `has_` / `can_` 前缀；简单 getter 省略 `get_` 前缀；缩写按单词处理（`HttpServer` 而非 `HTTPServer`）。
- 公共类型在合适时实现 `Debug` / `Clone` / `PartialEq` / `Eq` / `Hash` / `Serialize` / `Deserialize`；转换优先 `From` / `TryFrom` / `FromStr`；可能扩展的公共 struct/enum 使用 `#[non_exhaustive]`；builder 方法加 `#[must_use]`。
- serde 命名匹配外部 payload（通常 `rename_all = "snake_case"`）；可选字段 `#[serde(default)]`；空 optional 用 `skip_serializing_if`；严格配置格式拒绝未知字段。
- 窄化整数转换禁止使用 `as`，使用 `TryFrom`；算术溢出行为必须显式（`checked_` / `saturating_` / `wrapping_`），禁止依赖默认 panic 或静默环绕。
- 关键领域 enum 的 `match` 必须穷尽全部变体，禁止用 `_` 吞掉未来新增变体（匹配外部 `#[non_exhaustive]` 枚举除外）。

---

## 10. Ownership 与内存（强制）

- 优先借用，避免不必要的 `clone`；参数接收 `&str` 而非 `&String`，接收 `&[T]` 而非 `&Vec<T>`。
- 已知容量时使用 `with_capacity` 预分配。
- 热路径中复用 collection（clear 后重用），避免重复分配。
- 热路径中避免不必要的 `format!`，能直接写入或使用字面量就直接使用。
- enum 的大变体明显增大整体尺寸时，考虑用 `Box` 装箱大变体。
- 跨线程共享所有权使用 `Arc<T>`（并发规则见 §11）。

---

## 11. Async 与并发（强制）

- async runtime 统一使用 Tokio。
- 禁止在 `.await` 期间持有 std `Mutex` / `RwLock` guard；确需跨 `.await` 串行化时使用 `tokio::sync::Mutex`（其 guard 为跨 await 持有而设计）。
- 队列和背压使用 bounded channels。
- 运行取消和优雅关闭使用 `CancellationToken`；动态任务集合使用 `JoinSet` 管理。
- CPU-heavy 或 blocking 工作使用 `spawn_blocking`。
- `tokio::select!` 分支必须满足 cancellation-safe。
- 跨线程共享所有权使用 `Arc<T>`。

---

## 12. Unsafe 代码（强制）

- 除非有清晰且可衡量的必要性，否则禁止使用 `unsafe`。
- 每个 `unsafe` block 前必须有 `// SAFETY:` 注释说明不变量。
- 每个 `unsafe fn` 必须有 `# Safety` 文档。
- `unsafe` 作用域越小越好，只标记必须 unsafe 的操作。
- 禁止使用 `mem::uninitialized()`；禁止对有有效性约束的类型使用无效的 `mem::zeroed()`。

---

## 13. 克制设计与依赖纪律（强制）

- 默认选择能工作的最小设计；禁止为"以后可能需要"提前增加 wrapper / adapter / facade / manager 层或 snapshot 机制。
- 一个 trait 至少要有真实的多实现需求，单实现优先具体类型；配置项必须有真实使用场景，不会被改变的值不配置化。
- 优先复用标准库、已有 crate 内部函数和已引入依赖；新增依赖必须说明理由（含许可证与安全性）。
- 明显的扩展点先记 TODO 或注释，需求出现后再实现。
- 公共 API 必须有 `///` 文档；crate / module 意图用 `//!` 说明；示例避免 `unwrap()`。
- 实现完成后、PR 合入前，把最终设计约定归档到相关 crate 的 `AGENTS.md`。

---

## 附录：禁止清单（Red Flags）

以下代码在 Review 中必须一票否决：

- [ ] `unwrap()` 出现在非测试代码中；`expect()` 仅限 §2 允许的程序员错误不变量，且必须附不变量注释
- [ ] `println!` / `eprintln!` 出现在非 CLI 生产代码中（测试代码豁免）
- [ ] 密钥、token、用户凭据进入日志、span 字段或错误消息
- [ ] 领域 ID 以裸字符串（stringly typed）穿越 crate 或 HTTP 边界
- [ ] 业务 crate 直连 `std::fs` / `tokio::fs` 或 `async-nats`（耐久存储后端 crate——`stratum-infra` / `stratum-store`——的文件 IO 除外，见 §5）
- [ ] 在宿主机直接执行 agent 生成的命令
- [ ] std `MutexGuard` / `RwLock` guard 持有跨越 `.await` 点（为跨 await 串行化而持有 `tokio::sync::Mutex` guard 是允许的，见 §11）
- [ ] `let _ = ...` 吞掉 `Result` 且无注释说明原因（测试清理代码豁免）
- [ ] `unsafe` block 无 `// SAFETY:` 注释，或 `unsafe fn` 无 `# Safety` 文档
- [ ] crate `Cargo.toml` 中写裸版本号依赖（不走 workspace inheritance）
- [ ] 生产环境 `allow_any_origin()`
- [ ] 真实密钥 / 凭据提交入库
- [ ] 生产环境 `RUST_LOG=debug` 或 `trace`
