## Context

本 change 把已确认的 Ontology application metamodel 落成独立后端：Ontology 是完整保存的 schema 聚合，包含 Object Type、其独占 Property、二元 Link Type 与 Canvas Layout；对象实例、Memory 与物理数据绑定属于后续模块。前端在独立 `ontology-frontend` worktree 中按固定 HTTP 契约推进，不参与本 change 的实现或验收。

当前 workspace 没有 Ontology crate。`stratum-api` 是唯一进程与 HTTP 入口，现有 `api-documentation` 规范要求 utoipa OpenAPI 覆盖 router 中全部 operation。主线已将 Agent Runtime 执行持久化收敛到 `stratum-postgres`；本 change 明确 `stratum-ontology` 是只管理自身五张表的唯一独立领域例外。

设计受以下约束：使用 PostgreSQL 作为唯一 canonical representation；最大文档 2 MiB、500 Object Types、10,000 Properties、2,000 Link Types；整图保存必须原子且通过 ETag/CAS 防止覆盖；最大图完整读取和 depth-5 邻域的数据库侧 p95 目标为 100 ms；MVP 保持最少层次和依赖。

## Goals / Non-Goals

**Goals:**

- 提供稳定、严格、由 OpenAPI 描述的六个 Ontology HTTP operation。
- 用五张规范化表保存一个或多个 Ontology，数据库和 Rust 共同维护已确认的不变量。
- 以完整 Candidate 为事务边界，实现确定性多 violation 校验、强 ETag 条件写与原子硬删除。
- 提供从 Object Type 出发、双向、0–5 跳且返回诱导子图的持久化邻域读取。
- 让 `stratum-ontology` 成为职责完整的 library-only 深模块，并给未来独立部署保留一个小而具体的调用边界。

**Non-Goals:**

- 不实现前端、画布验收、对象实例、Memory 接入或物理数据源绑定。
- 不实现版本、历史、snapshot、branch/proposal、审计、soft delete、schema version 或兼容层。
- 不实现认证、RBAC、多租户、Shared Property、Interface、Action、Group、推理或继承。
- 不实现 JSONB canonical document、重复投影、diff/upsert、COPY、staging table、缓存或搜索索引。
- 不增加 Repository trait、port/adapter、facade/manager、RPC、第二个 binary、服务发现或客户端 SDK。
- 不实现 OT/CRDT、operation log、WebSocket presence 或自动离线合并。

## Decisions

### 1. 一个 library crate 拥有完整 Ontology 后端

新增 `crates/stratum-ontology`。它拥有类型化 UUIDv7 ID、领域模型、Candidate 校验、稳定 violation、五张表的 embedded migrations、具体 PostgreSQL 查询/事务、revision CAS 与邻域遍历。公开边界是一个具体 `OntologyStore`，直接提供 `list`、`create`、`get`、`replace`、`delete` 与 `neighborhood`；内部 row、SQL、revision 算术与组图逻辑保持私有，错误独立放在 `error.rs`。

`stratum-api` 拥有 HTTP DTO、path/query/header 解析、ETag 编解码、utoipa annotation、状态码映射与进程装配。它把 DTO 转成领域值并进程内调用 `OntologyStore`；`stratum-ontology` 不依赖 `stratum-api`、`stratum-postgres` 或装配层。`stratum-config` 新增必需的 `[ontology].database_url`，在外部依赖初始化前验证 PostgreSQL URL，示例、Docker 和测试配置同步更新；MVP 拒绝 URL query 参数，避免 SQLx 对未知键值发出含敏感值的 warning。URL 的 `Debug` 表示必须脱敏，且值不进入日志或错误响应。

Agent execution 与 Ontology 可以共用一个 PostgreSQL server/container，但必须使用独立 database。两者各自拥有 embedded SQLx migration 集合，隔离 database 可避免共享 `_sqlx_migrations` 导致 foreign migration version 启动失败；生产与 API integration compose 均通过 fresh-volume init SQL 确定性创建 Ontology database。

选择具体类型而不是 trait，是因为 MVP 只有 PostgreSQL 一个实现和一个调用方。未来需要独立部署时再用进程包装这六个方法并定义网络协议，不提前承诺 RPC 兼容。

这些公开领域 struct 与 scalar enum 表示闭合的 MVP 元模型和精确持久化转换，不是为第三方任意扩展的开放协议。新增字段或变体必须作为协调后的 schema/API change 显式演进；因此不为假设性兼容增加 `#[non_exhaustive]`、builder 或第二套构造 API。

### 2. 五张规范化表是唯一事实来源

首次 migration 创建以下最小 row shape：

| Relation | Columns | Key constraints |
| --- | --- | --- |
| `ontologies` | `id uuid`, `name`, `display_name`, nullable `description`, `revision bigint`, `created_at timestamptz`, `updated_at timestamptz` | PK `id`; unique `name`; positive revision |
| `ontology_object_types` | `id uuid`, `ontology_id uuid`, names/text, `sort_order integer` | PK `id`; FK Ontology; unique `(ontology_id, name)` and `(ontology_id, id)` |
| `ontology_properties` | `id uuid`, `object_type_id uuid`, names/text, `value_type text`, `required boolean`, `sort_order integer` | PK `id`; FK owner; unique `(object_type_id, name)`; value-type check |
| `ontology_link_types` | `id uuid`, `ontology_id uuid`, names/text, source/target IDs, two cardinality texts, `sort_order integer` | PK `id`; FK Ontology; unique `(ontology_id, name)`; composite same-Ontology endpoint FKs; cardinality checks |
| `ontology_canvas_positions` | `object_type_id uuid`, `x double precision`, `y double precision`, `sort_order integer` | PK/FK Object Type; finite-coordinate checks |

Root ownership foreign keys cascade only when a complete Ontology is deleted. Replacement still deletes Link Types、Properties、Positions、Object Types in explicit dependency order. Composite Link endpoint foreign keys include `ontology_id`, so跨 Ontology link 在数据库中不可表示。为已确认的访问路径增加窄索引：Object Types 按 `(ontology_id, sort_order, id)`；Properties 按 `(object_type_id, sort_order, id)`；Links 分别按 `(ontology_id, source_object_type_id)`、`(ontology_id, target_object_type_id)` 及 presentation order；Positions 通过 Object Type join 获取。

`value_type` 与 cardinality 使用带 `CHECK` 的 `text`，避免 PostgreSQL enum 的演进负担。文本长度、名称 regex、图引用与计数由 Rust 汇总校验，关键所有权、唯一性、端点与 scalar domain 由数据库再次兜底。不增加 identity registry：typed UUIDv7 的主键在每种实体类型的所有现存 Ontology 中全局唯一；服务生成的新 Ontology ID 不复用，客户端负责为新子实体生成新 ID，硬删除后的 API 不承诺检测客户端故意复用历史 ID。

拒绝一行 JSONB 的原因是用户已要求 Object Type、Property、Link Type 为独立 canonical rows；拒绝 hybrid 是因为没有第二份事实来源能证明其额外一致性成本有收益。

### 3. API 文档与领域模型分离

HTTP 完整文档不包含 `revision`、timestamps 或存储字段；列表摘要单独返回 `created_at`/`updated_at`。所有集合在 wire 上必需，`description` 只能省略而不能为 `null`。DTO 使用 `deny_unknown_fields`，结构/类型/UUIDv7/enum/header 解析失败映射为 `400 invalid_request`；结构正确但违反领域规则的输入映射为 `422 invalid_ontology_schema`。

纯 validator 一次遍历 Candidate，使用 map/set 检查 ID、名称、owner、endpoint、位置与数量限制，按 specs 固定的 code/path 表收集 typed violation，再按 JSON Pointer `path`、`code` 排序。Unicode 长度按 scalar value 计数。Candidate 内部重复 ID 属于 422；某个 typed ID 已由另一现存 Ontology 占用时，表主键直接兜底并由已知 constraint 映射成 `409 ontology_entity_id_conflict`，不增加易竞态的预扫描。成功写入只保存协议有意义的字段存在性、语义值与数组顺序，不注入默认值或做规范化排序；`sort_order` 直接取数组下标。

每个稳定 violation code 由一个 enum 产生并用 table-driven tests 锁定。共享 `ErrorResponse.error` 增加一个仅在 Ontology 422 中序列化的 optional `violations` 字段，其他现有错误的 JSON 不变。不得把 PostgreSQL constraint 名、SQLSTATE detail、数据库 URL 或 Candidate 内容暴露给客户端。

### 4. 整图替换使用根 revision CAS

`ontologies.revision` 从 1 开始，只在成功 PUT 时递增，不进入 JSON。API 为当前 Ontology ID/revision 生成 canonical 强 ETag。`If-Match` 按 HTTP entity-tag 的原始 bytes 校验，不能先要求 UTF-8；HTTP 语法无效、weak、`*` 或列表映射 400，包含合法 `obs-text` 的单一强 tag 仍是语法正确的 opaque validator。任一语法正确的单一强 tag 若不是路径资源的当前 canonical 值（包括其他资源 tag 或任意 opaque 值）都映射 412。服务只解析自己的 ASCII canonical 格式以取得 expected revision，不保存已发放 tag、不签名也不增加密钥；不能解析为当前路径 canonical 格式的强 tag 作为“永不匹配”的 expectation 进入同一 missing/stale 判定，因此资源不存在仍返回 404。

PUT 先完成纯 Candidate 校验，然后开启 Read Committed transaction：

1. `UPDATE ontologies ... SET revision = revision + 1, updated_at = clock_timestamp() WHERE id = $id AND revision = $expected RETURNING revision`；零行时查询并区分 404 与 412。
2. 按 Link、Property、Position、Object Type 顺序删除旧 child rows。
3. 按 Object Type、Property、Link、Position 顺序批量插入 Candidate；使用 `sqlx::QueryBuilder` 分块，避免 10,000 Properties 超过 PostgreSQL bind 参数上限。已知 typed-ID 主键冲突回滚整个 transaction 并映射 409。
4. commit 后由 API 返回 `204 No Content` 与新 ETag。

任一步失败都回滚 revision 与 child rows。即使 Candidate 与当前语义内容相同，成功 PUT 仍推进 revision 和 `updated_at`，避免增加 diff 路径。DELETE 用带 revision 条件的 root delete 并依赖 aggregate cascades，返回相同的 missing/stale 区分。不同 Ontology 锁不同 root row；不使用 `SELECT FOR UPDATE`、advisory lock 或全局 mutex。

在提交响应不确定时，客户端按既定共享文档模型重新 GET 对账；MVP 不增加 save-attempt 表或 idempotency key。

### 5. 读取使用普通 typed queries 与数据库 snapshot

列表在 Read Committed 下用一个 SQL statement 同时返回真实 total 和当前 page；分页超界时仍返回 total。排序字段由 enum 映射到固定 SQL，绝不拼接任意用户输入；同值以 ID 升序打破平局，文本排序显式使用 PostgreSQL `C` collation 以保持部署间确定性。

完整读取在 `REPEATABLE READ READ ONLY` transaction 中分别读取 root、Object Types、Properties、Link Types 与 Positions，全部按 `sort_order, id` 排序后在 Rust 组装；不让 SQL 生成 API JSON。

邻域也使用 `REPEATABLE READ READ ONLY`。depth 1–5 先用一个固定 SQL 同时确认 Ontology/origin、读取 Object Type 数量和最多 2,000 个有序 Link rows，随后在 Rust 中维护 `visited`/`frontier` 完成双向 BFS，并复用同一批 Link rows 构造诱导子图；depth 0 只确认存在性并查询 origin 上的 self-links。实现只用标准集合和 SQLx，不引入图算法依赖或递归 CTE。完整读取与邻域的最多 10,000 个有序 Property rows 由 PostgreSQL 聚合为一组对齐的 typed column arrays，再在 Rust 中校验长度并组装，以避免逐行协议开销；五张 canonical relations 和返回语义不变。

最大 fixture 的完整图与 worst-case depth-5 查询用 crate 内 ignored PostgreSQL integration test 预热后重复采样，并在本地/CI 专用容器运行时断言数据库侧 p95 不超过 100 ms。depth-5 fixture 必须分层且没有跨层捷径，使 origin 到第五层确实执行五轮 frontier expansion，而不是第一跳选中全图。只有该测量失败才允许讨论 CTE、缓存或额外索引。

2026-08-12 在 crate 自有 Podman/PostgreSQL 套件中对无跨层捷径、额外 links 分布于前四层的最大夹具连续验证三轮，并在最终显式聚合排序后再次运行全套；最终完整读取 p95 为 `46.456791 ms`，depth-5 邻域 p95 为 `54.868834 ms`。优化依据是查询级测量确认串行 frontier 往返与 10,000 个 Property rows 的逐行传输为主要差值；未增加缓存、递归 CTE、索引或 benchmark 依赖。

### 6. SQLx、迁移、错误与观测保持最小

workspace 新增 `sqlx`，仅开启 Tokio/Rustls、Postgres、UUID、chrono、migration 与 macro 所需 features。`OntologyStore::connect(&str)` 解析 URL、建立固定默认 pool 并运行 `sqlx::migrate!()`；不为未出现的调优需求增加 pool 配置项。初始连接或 migration 失败使进程启动失败；运行期连接失败映射 503。

模块用 `thiserror` 定义 typed errors并保留 source chain，包括已知唯一约束映射；PostgreSQL 连接类和 shutdown/startup-unavailable SQLSTATE 映射为运行期 unavailable，未知数据库失败留在内部 error。API 在最终处理边界记录一次 tracing error。每个 operation 建立 span，记录 operation、typed Ontology ID、depth、结果分类与耗时，不记录名称、description、Candidate、ETag、expected revision、URL 或凭据。

生产 router 继续满足进程级运维基线：liveness 只表示进程可响应；readiness 在一个 `[api].readiness_timeout_ms` 总时限内同时确认 execution PostgreSQL 与 Ontology PostgreSQL 可查询，任一不可用即返回 unavailable。NATS 仅影响返回体中的 realtime capability，不禁用 Postgres-backed commands。探针只执行非变更型依赖检查，不写执行事件或 Ontology 数据。handler 错误继续在共享 `ApiError` HTTP 边界记录一次，不暴露存储或候选内容。

### 7. 验证覆盖协议、领域与真实 PostgreSQL

纯单元测试覆盖 typed ID、validator 全量 violation/排序、name scope、Property owner、Link 引用、限制、ETag parser 与组图/BFS。`stratum-api` router tests 覆盖六个 operation 的状态/headers/body、2 MiB route-specific limit、412/422 不变性及 OpenAPI tag/schema/status；现有 64 KiB 限制继续用于其他 route。

`stratum-ontology/tests` 通过 crate 自有 `docker-compose.test.yml` 与 `Makefile` 运行默认 `#[ignore]` 的 PostgreSQL 集成测试，至少证明 migration、CRUD、约束、硬删除、CAS 并发只有一个成功、中途错误完整回滚、不同 Ontology 独立写入、并发替换期间完整读取和邻域的跨查询 snapshot 一致性、双向/环/self-link 邻域及最大 fixture 性能。`stratum-api` 的 PostgreSQL router suite 也通过自身 Makefile 运行；CI 分别执行两套容器 suite 并无条件 `down -v`。普通 `cargo test --workspace --all-targets` 不依赖容器。

实现完成后必须运行 fmt、workspace clippy/test、OpenSpec strict validation，并派发独立子代理按根 Constitution 分条款审查完整 diff；所有 red-flag/violation 修复后，才能勾选归档准备。最终约定同步到 `crates/stratum-ontology/AGENTS.md`。

## Risks / Trade-offs

- [整图 delete/insert 会产生 WAL、dead tuples 与较长事务] → 用已确认最大 fixture 测 p95；仅在超过 100 ms 后考虑优化，不预建 diff/staging/COPY。
- [多条 SELECT 可能读到混合 revision] → 完整读取与邻域均固定为 Repeatable Read read-only transaction，并测试并发保存期间的一致性。
- [HTTP DTO 与 normalized rows 可能发生顺序或字段漂移] → `sort_order` 源自输入下标，round-trip tests 比较字段存在性、语义值与数组顺序。
- [CAS 零行同时可能表示 missing 或 stale] → 在同一 transaction 中执行额外 existence read并固定 404/412 映射。
- [客户端在 commit 边界丢失响应] → 客户端保留 in-flight Candidate 并 GET 对账；MVP 接受额外 round trip，不增加写入日志。
- [PostgreSQL 变为 stratum-api 的必需启动依赖] → 配置 fail-fast、migration 启动时运行；部署先准备数据库再切换 binary。
- [客户端提交已由另一 Ontology 使用的 typed ID] → 数据库 PK 是无竞态的唯一性 backstop；按已知 constraint 名映射 409，整事务回滚且不解析错误文本。
- [显式 `C` collation 的 Unicode 排序不是语言学排序] → MVP 优先部署一致性；未来真实本地化排序需求需新增明确 API，而不是依赖数据库默认 locale。

## Migration Plan

1. 在 feature worktree 中先提交 Constitution 窄化与 ADR，再新增 workspace dependency、`stratum-ontology` skeleton 和 crate `AGENTS.md`。
2. 增加初始 forward-only migration、配置字段与容器测试资产；在空测试数据库运行 migration 和集成测试。
3. 实现领域/存储后接入 `stratum-api`，更新所有示例、Docker、测试配置和 OpenAPI 覆盖断言。
4. 部署时先创建/授权 PostgreSQL database，再部署包含必需 `[ontology]` 配置的新 binary；启动 migration 完成后才接收流量。MVP 没有旧 Ontology 数据需要 backfill。
5. 回滚 binary 时同时回滚配置文件；保留新增空表或已有 Ontology 数据，不自动执行 destructive down migration。若确认从未写入且必须清理，由运维显式删除五张表。

## Open Questions

无。MVP 的元模型、命名范围、存储形状、事务、read isolation、ETag、邻域语义、限制与模块边界均已在 proposal/specs 中固定；新需求进入后续 change。
