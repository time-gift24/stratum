# stratum-api 约定

- `stratum-api` 是唯一装配 crate：作为装配/编排层，拥有进程注册表、Postgres 编排、
  审批处理器/解析器、NATS 桥接器/分发器与 SSE。`main.rs` 必须保持薄，可复用
  逻辑放 `lib.rs`。
- 只有在 Agent/Session/Turn 的必要状态、运行时快照和输入已持久化后，创建或消息接口才能返回已接受；失败不得留下可被接受为成功的半成品。
- Postgres 持久账本是 AgentRuntime 状态、消息历史和启动恢复的唯一持久化真相源；NATS 只负责短期 AgentRuntime 尾流分发，不能代替 Postgres。`sqlx` 只允许出现在 `stratum-postgres`，本 crate 只调用其具体命令/查询接口。
- 托管状态是进程内精确 `(AgentRuntimeId, TurnId)` 注册表的易失观察，永不持久化；注册表的锁只保护内存映射访问，Postgres、NATS、提供器和 Agent 的异步工作必须在锁外完成。
- 持久化顺序固定为先提交 Postgres，再发布 NATS；每个 AgentRuntime 的分发器按 `event_seq` 从已提交的 PG 行发布产品事件。NATS 发布/通知失败只记录一次安全错误，不回滚 PG、不改变命令或内核结果。
- 审批完全从持久账本派生：审批处理器是普通的 `decide_tool_call` Hook 处理器，解析器事务复用 `agent_states` 行锁实现线性化；解析与恢复是分离的端点，未托管状态下的解析不会隐式恢复。
- 执行、Ontology 与 Studio 三个 PostgreSQL 共同决定核心就绪性；NATS 不可用时 SSE 返回稳定的 `realtime_unavailable`，Web 降级为 PG 对账，核心命令继续可用。
- `stratum-ontology` 通过具体 `OntologyStore` 进程内装配到 `AppState`；HTTP DTO、ETag/`If-Match`、错误信封、OpenAPI 与 2 MiB route body limit 只属于 `http/ontology.rs`，不得回流领域 crate。执行存储与 Ontology 使用同一 PostgreSQL 服务中的独立 database，隔离各自 SQLx migration history。
- liveness 不探测外部依赖；readiness 在 `[api].readiness_timeout_ms` 的一个总时限内同时探测执行、Ontology 与 Studio PostgreSQL。NATS 状态只作为 `realtime` capability 报告，不决定 readiness 结果。
- Studio PostgreSQL 是 Provider credential、Model 与当前 Agent definition 的唯一运行时目录；API 启动必须连接并校验它，每次新的 LLM work 从事务一致的 Studio 数据组装短生命周期 Provider snapshot，不维护进程热缓存。即使 `management_enabled = false` 也不得回退到 boot config、环境变量或 template 文件；该布尔值只控制 loopback management routes 与对应 OpenAPI fragment。
- Provider adapter 的官方 endpoint 与显式出站 timeout 是二进制拥有的受信策略，不由 Studio 请求或部署配置覆盖；credential 仅从 StudioStore 以 `SecretString` 进入 adapter 装配，不得记录或返回。
- `ProviderFactory` 只复用进程级 `reqwest::Client` 传输池；每次 LLM work 仍从 Studio DB 重读 credential 与 Model membership 并重建 manager/adapter。共享连接池不得演变成 Provider catalog 或 credential cache。
- Model create 的 adapter 校验与 parameter schema 必须在 Studio mutation 前从当时的 DB credential snapshot 组装；Store commit 后只能使用已取得的 schema 与 transaction 内物化的 `Versioned` 构造响应，不得再次读取 Studio 或重建 Provider manager。
- AgentRuntimeView 对外同时编码十进制字符串 `snapshot_event_seq` 与
  `telemetry_floor_event_seq`；后者是在同一 MVCC 屏障内从账本派生出的最新 `assistant`
  `MessageAppended` 序号，用于冷恢复时拒绝首屏之外、最终事件之前的旧遥测，
  不是第二个高水位或持久化投影。
- 历史与 SSE 持久帧共享完整、严格解码的 `AgentRuntimeProductEventV1` 联合类型：
  `LoopStarted`、消息、审批请求/解析、压缩、迭代与三类终止事件。
  Hook 日志、`ToolExecutionStarted` 等内部事实只占用持久序号，不公开投影。
- SSE 使用绑定 `AgentRuntimeId + JetStream 流代次 + 流序号` 的不透明 NATS 游标：游标不得与 `event_seq`/`telemetry_seq` 比较，也不得持久化为业务状态；跨 AgentRuntime、旧代次与保留期过期必须在发送响应头前显式报错；建流后的缓冲区溢出会发送 `stream_reset` 并关闭连接。
- `AppState` 持有共享关闭令牌、准入门与进程所有的 `JoinSet`。Turn、分发器、SSE 泵全部进入该集合；关闭中间件在令牌触发时丢弃尚未返回响应的处理器异步任务并返回稳定的 503，再以 `[api].shutdown_drain_timeout_seconds` 的单个总截止时间依次收敛 Axum 服务器、准入门与任务集合，超时则中止并等待结束。未完成的 Turn 在 PG 中保留持久 `running` 状态以供显式恢复；进程关闭绝不转化为业务取消/失败，Turn 取消令牌不因进程退出而触发。
- 创建、消息和恢复在任何持久化或提供器 I/O 前必须取得原子准入 RAII 守卫，并在等待中的 Postgres/NATS 工作中观察关闭令牌。关闭后的新持久工作返回安全稳定的 503。
- HTTP 最终错误边界只记录一次安全的结构化操作错误；span 可记录 Agent/Session/Turn/游标等 ID，不得记录消息、提示词、Tool 参数、密钥、SQL 或宿主机路径。
- 错误映射合同：库错误使用 `thiserror`，HTTP 统一映射为安全信封
  `{"error":{"code":"...","message":"..."}}` 与约定的 400/404/409/410/412/413/422/428/500/502/503；
  响应体不暴露 SQL、NATS 主题、宿主机路径、提示词、Tool 参数/结果、提供器正文或凭据。运行时路由的 404 固定为 `agent_runtime_not_found`；目录名称缺失为 `agent_template_not_found`；状态存在但固定的定义缺失或损坏时，必须以 `durable_state_corrupt` 故障关闭。
- API 文档以 utoipa 生成的 OpenAPI 为唯一权威：每个处理器必须有 `#[utoipa::path]`，DTO 与传输类型必须有 `ToSchema`；每个状态码都显式声明响应体类型（空成功响应使用 `body = ()`）；错误响应只声明该处理器经 `error_response()` 实际可达的状态码；SSE 端点以 `text/event-stream` 与 API 自有的 `AgentRuntimeStreamFrameV1` 描述。`docs/PROTOCOL.md` 已废弃。

## 模块与实现约定（重写后归档）

- 模块布局：`state.rs`（AppState + 准入门）、`registry.rs`（精确
  `(AgentRuntimeId, TurnId)` 占用 + 比较后移除）、`sink.rs`（每个 Turn 的
  `DurableEventSink`/`TelemetryEventSink` 适配器 + 准入一次性通道）、`baseline.rs`
  （历史基线物化 + 7.10 规范化，纯函数 `assemble` 可单测）、`provenance.rs`（已提交上下文的
  来源序号血缘，供压缩保留指针解析）、`dispatcher.rs`（每个 AgentRuntime 的
  有序具体 PG+NATS 分发器）、
  `approval.rs`（决策阶段的审批 HookHandler + 进程内等待器）、`turn.rs`（运行时
  重建与受管任务生成）、`frames.rs`
  （`AgentRuntimeStreamFrameV1`/`AgentRuntimeProductEventV1`）、`dto.rs`、`error.rs`（`ErrorKind` →
  状态码/代码映射表）、`host_error.rs`（启动错误）、`http/`（路由器 + 处理器 + utoipa）。
- 审批处理器的 `HookInvocationId` 不由内核传入：内核保证先提交 `Pending`，处理器通过
  `stratum-postgres` 的 `read_open_hook_invocation`（`point` + `iteration` + `call_id` 精确地址）
  找到自己的开放调用，再以它为键复用/创建 `Requested`。恢复时重放 `Pending`，同一地址
  命中同一调用，天然复用既有 ApprovalId。
- 当前 production 组合只接受 `shell` 与 `apply_patch`，拒绝包括 `echo` 在内的其他
  Tool 名称。`AppState` 启动时校验并固定 `[tools].workspace_root`，两者共享该 root：
  `shell` 将它作为默认 cwd，`apply_patch` 将它作为显式注入的虚拟文件系统 root。
  两者继续复用既有 `ApprovalHandler` 与 `RequireApproval`，不得增加第二个 broker。
  `ShellTool` 只负责一次性进程语义，不拥有 sandbox；production 部署必须在外层容器/
  sandbox 边界中运行。
- `TurnRuntimeSnapshot` 以 `agent_id: AgentId` 固定不可变定义，并在消息准入时构造：`extension_set_version_id` 必须取
  自新建 `ChainHookRuntime` 的计算值（与内核写入 `LoopStarted` 载荷的值一致）；
  `skill_set_version_id` 固定为全零 UUID；Hook 版本列表当前只有审批处理器的固定 UUID 常量
  （行为变更必须换版本号）。恢复时重建运行时需校验提供器/模型可用、Tool 指纹
  与扩展集版本一致，不一致即返回 503 `runtime_unavailable`。
- 恢复的重放窗口 = 当前 `LoopStarted` + 基线消息（作为 MessageAppended）+
  当前 Turn 后缀（包含 Hook 日志与审批事实），按 `event_seq` 排序；恢复后的接收器
  血缘必须与内核重建的上下文对齐（基线来源 + 逐个应用后缀消息/压缩）。
- 恢复先完成不依赖已绑定接收器的持久切片、定义/提供器/Tool 指纹、血缘
  与类型化窗口前置检查，再确保分发器存在，并组装接收器/循环来执行纯计算的 `prepare_resume`；准备
  失败不得写入持久真相或调用外部能力。成功后以短时状态行锁同时重验预期的 Agent 固定值、
  Session/当前 Turn/`running`，再安装受管任务。
- 分发器中心的 `ensure(AgentRuntimeId)` 不接收调用方前沿；它在该运行时专属的
  门内读取已提交 PG 高水位、安装代次并取得活跃句柄。每个持久写入器
  必须在事务前取得句柄、持有至提交完成，提交后才以同一句柄提交回执；已托管 Turn
  把句柄交给已绑定接收器/受管任务，并持有到 Turn 退出。`ensure` 只做 PG 读取和本机注册，
  不等待 NATS 发布。
  分发器的原子高水位保存已知持久回执；每个 `DurableWake` 固定自己的
  `through`，每条遥测在入队时固定 `durable_before`，出队时只能刷出该命令的屏障，
  禁止旧遥测读取未来最终事件的目标值。该冻结值同时以十进制字符串
  `durable_before_event_seq` 写入遥测 v1 帧；它只是 PG 排序水位，不改变
  `(llm_call_id, telemetry_seq)` 身份。遥测队列满时允许丢失并保留缺口，但持久
  回执不得等待 NATS/队列容量：队列满时只合并进不回退的原子高水位。
  合并刷出必须先对目标拍摄快照，再确认已接收命令队列为空，且只能刷出这个旧快照；
  拍摄快照后推进的目标留给下一次排空/空闲循环，空闲退休前必须最终追平。禁止先声明
  队列为空，再读取可能已包含未来最终事件的目标。
  PG 扫描必须从前沿连续推进到目标，出现缺洞/乱序时故障关闭；产品事件序列化/发布失败
  不得推进前沿，并抑制该屏障后的遥测，重复的持久 `event_seq` 由客户端去重。
  只有显式列出的内部持久变体可在不产生帧的情况下推进前沿；未来的
  `DurableAgentEvent`/`ApprovalDecision` 必须保留类型化错误并故障关闭，禁止默认按内部事件、批准
  或阻断处理。
  映射表保留发送端的强引用，保证同一 AgentRuntime 只有一个分发器；只有已配置的空闲间隔到期、无外部
  句柄且已接收队列为空时，才可在映射表锁内以“移除并关闭”原子操作退休，禁止双分发器竞态
  或吞掉已接收命令。
- 正常退休与 NATS 持续失败后的降级放弃必须和 `ensure` 在同一中心门内
  线性化；存在任一活跃生产者句柄时不得退休或丢弃。句柄为零且有界重试耗尽时，只可
  丢弃易失队列/目标，不得修改 PG 真相；下一次 `ensure` 从当时已提交的高水位建立新
  代次，不在进程重启或代次退休后重新灌入旧历史。
- 测试：单元测试位于各模块的 `#[cfg(test)]`；容器集成测试源码位于 `tests/api.rs`、
  `tests/ontology_api.rs`、`tests/studio_db_only.rs` + `tests/common/mod.rs`，通过 `lib.rs` 的
  `#[cfg(test)]` path modules 编译（Cargo `autotests = false`），从而只让 crate-private、test-only
  `AppState` mock Provider 注入边界服务这些测试，不形成 production API。容器用例保持
  `#[ignore]`，由 `make test-integration` 运行（Compose 项目 `stratum-api-test`）。完整集成命令让 Docker/Podman 动态发布 loopback host ports，
  再把实际 PG/NATS endpoint 注入测试进程，避免 CI runner 服务或 ephemeral client
  socket 占用固定端口；手动 `make test-up` 仍默认 PG 45433 / NATS 44228，也可通过
  `STRATUM_API_TEST_PG_HOST_PORT` / `STRATUM_API_TEST_NATS_HOST_PORT` 覆盖。
- OTLP 由 `telemetry.rs::init_telemetry()` 按环境激活：设置 `OTEL_EXPORTER_OTLP_ENDPOINT` 时安装 OTLP span 导出器（HTTP/protobuf、`reqwest-blocking`、不使用 `tonic`）与 `tracing-opentelemetry` 层，未设置时与纯 `fmt` 行为完全一致；进程退出前必须经 `TelemetryGuard::shutdown()` 刷出数据。采集器端点仅支持 `http://`。
- 审批等待器使用 RAII 注册守卫（`Drop` 时按精确注册身份注销）；`register` 返回守卫+接收端，提前命中/读取错误/取消均不泄漏。
- 审批处理器必须注册后立即读取 PG，并在通知、取消与内部固定上限节拍
  之间进行选择；每次唤醒/节拍都重新读取持久真相。轮询节拍不得暴露为用户配置。
- DispatcherHub 任务由进程 `JoinSet` 所有，条目按代次比较后移除；关闭
  超时后由 `JoinSet::abort_all` 中止再等待结束，任何分发器/SSE/Turn 任务都不得游离。
- 恢复的六字段快照校验包含 `skill_set_version_id` 与有序 `hook_handler_versions`，由 `turn.rs` 的固定常量作为单一来源，同时供写入与校验。
- 消息准入的预期当前 Turn 判定先于状态判定：预期值不匹配即 `stale_turn`（即使状态为 `running`）；`agent_busy`/`resume_required` 只用于预期值匹配的 `running` 请求。
- 进程关闭语义：受管 Turn 一旦插入进程 `JoinSet` 即拥有占用；注册表只保存精确
  占用状态/令牌，不保存 JoinHandle。关闭只关停准入并排空，或中止并等待结束，绝不触发
  Turn 令牌。
- 准入排空使用先启用并固定的 `Notify::notified()`，再复查进行中计数；最后一个
  RAII 守卫的 `notify_waiters()` 不得落在加载/注册窗口而丢失。
