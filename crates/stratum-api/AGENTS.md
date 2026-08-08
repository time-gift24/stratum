# stratum-api 约定

- `stratum-api` 是唯一装配 crate：装配/编排层，拥有 process registry、Postgres 编排、
  approval handler/resolver、NATS bridge/dispatcher 与 SSE。`main.rs` 必须保持薄，可复用
  逻辑放 `lib.rs`。
- 只有在 Agent/Session/Turn 的必要状态、runtime snapshot 和输入已持久化后，创建或消息接口才能返回已接受；失败不得留下可被接受为成功的半成品。
- Postgres durable ledger 是 agent 状态、消息历史和启动恢复的唯一持久化真相源；NATS 只负责短期 Agent tail 分发，不能代替 Postgres。`sqlx` 只允许出现在 `stratum-postgres`，本 crate 只调用其 concrete command/query 接口。
- hosting 是进程内 exact `(AgentId, TurnId)` registry 的易失观察，永不持久化；registry 的锁只保护内存映射访问，Postgres、NATS、provider 和 agent 的异步工作必须在锁外完成。
- 持久化顺序固定为 Postgres commit 先于 NATS 发布；per-Agent dispatcher 按 `event_seq` 从已提交 PG row 发布 product event。NATS 发布/通知失败只记录一次安全错误，不回滚 PG、不改变 command 或 kernel 结果。
- 审批完全从 durable ledger 派生：approval Handler 是普通 `decide_tool_call` Hook handler，resolver 事务复用 `agent_state` 行锁线性化；resolve 与 resume 是分离的 endpoint，unhosted resolve 不隐式 resume。
- Postgres 决定核心 readiness；NATS 不可用时 SSE 返回稳定的 `realtime_unavailable`，Web 降级为 PG reconcile，核心 command 继续可用。
- SSE 使用不透明 NATS cursor：cursor 不得与 `event_seq`/`telemetry_seq` 比较或持久化为业务状态，过期必须显式报错；建流后的 buffer overflow 发送 `stream_reset` 并关闭连接。
- `AppState` 持有共享 shutdown token 与 admission gate。shutdown 关闭 admission 后结束 SSE，在独立固定时限内 drain 已准入请求，再有界等待终态持久化；超时保留 durable `running`，由显式 resume 接管。进程 shutdown 绝不转化为业务 cancel/failed：managed turn 的 CancellationToken 在 shutdown 时从不被 signal，超时后未完成任务由 runtime 回收，PG 中保持 `running`。
- create、message 和 resume 在任何持久化或 provider I/O 前必须取得 atomic admission RAII，并在 pending Postgres/NATS 工作中观察 shutdown token。关闭后的新 durable work 返回安全稳定的 503。
- HTTP 最终错误边界只记录一次安全的结构化 operational error；span 可记录 Agent/Session/Turn/cursor 等 ID，不得记录 message、prompt、tool args、secret、SQL 或 host path。
- 错误映射合同：library errors 用 `thiserror`，HTTP 统一映射为安全 envelope
  `{"error":{"code":"...","message":"..."}}` 与约定的 400/404/409/410/413/422/500/503；
  响应体不暴露 SQL、NATS subject、host path、prompt、Tool arguments/result、provider 正文或 credential。
- API 文档以 utoipa 生成的 OpenAPI 为唯一权威：每个 handler 必须有 `#[utoipa::path]`，DTO 与 wire 类型必须有 `ToSchema`；错误响应只声明该 handler 经 `error_response()` 实际可达的状态码；SSE 端点以 `text/event-stream` 与 API-owned `AgentStreamFrameV1` 描述。`docs/PROTOCOL.md` 已废弃。

## 模块与实现约定（重写后归档）

- 模块布局：`state.rs`（AppState + admission gate）、`registry.rs`（exact
  `(AgentId, TurnId)` claim + compare-and-remove）、`sink.rs`（per-turn
  `DurableEventSink`/`TelemetryEventSink` adapter + admission oneshot）、`baseline.rs`
  （历史基线物化 + 7.10 规范化，纯函数 `assemble` 可单测）、`provenance.rs`（committed-context
  来源 seq lineage，供 compaction retained pointer 解析）、`dispatcher.rs`（per-agent
  有序 dispatcher，`DispatcherIo` trait 的真实实现是 PG scan + NATS publish）、
  `approval.rs`（decide 相位的审批 HookHandler + 进程内 waiter）、`turn.rs`（runtime
  重建与 managed task spawn）、`templates.rs`（只读热 catalog）、`frames.rs`
  （`AgentStreamFrameV1`/`AgentProductEventV1`）、`dto.rs`、`error.rs`（`ErrorKind` →
  status/code 映射表）、`host_error.rs`（启动错误）、`http/`（router + handlers + utoipa）。
- 审批 Handler 的 `HookInvocationId` 不由 kernel 传入：kernel 保证 Pending 先提交，Handler 通过
  `stratum-postgres` 的 `read_open_hook_invocation`（point + iteration + call_id exact 地址）
  找到自己的 open invocation，再以它为键 reuse/创建 Requested。resume 重放 Pending 时同一地址
  命中同一 invocation，天然复用既有 ApprovalId。
- `TurnRuntimeSnapshot` 的六字段在 message admission 时构造：`extension_set_version_id` 必须取
  自建 `ChainHookRuntime` 的计算值（与 kernel 写入 `LoopStarted` payload 的值一致）；
  `skill_set_version_id` 固定为 nil UUID；hook 版本列表当前只有审批 Handler 的固定 UUID 常量
  （行为变更必须换版本号）。resume 重建 runtime 时校验 provider/model 可用、tool fingerprint
  与 extension set version 一致，不一致即 503 `runtime_unavailable`。
- resume 的 replay window = 当前 `LoopStarted` + 基线消息（作为 MessageAppended）+
  current-Turn 后缀（含 hook journal 与 approval facts），按 event_seq 序； resumed sink 的
  lineage 必须与 kernel 重建的 context 对齐（基线 origins + 后缀消息/压缩逐个应用）。
- dispatcher 的 telemetry 采用到即发：kernel 单 sender FIFO（telemetry 先于同 call 的 final
  message commit）加上 dispatcher 单命令队列即满足"telemetry 先于 final durable frame"；durable
  一律按 receipt high-water 从 PG 扫描发布，writer 醒来顺序不影响 NATS 顺序。
- 测试：单元测试在各模块 `#[cfg(test)]`；容器集成测试在 `tests/api.rs` +
  `tests/common/mod.rs`（`#[ignore]`，`make test-integration`，compose project
  `stratum-api-test`，pg 45433 / nats 44228，与其它 crate 端口错开）。
- OTLP 由 `telemetry.rs::init_telemetry()` 按环境激活：设置 `OTEL_EXPORTER_OTLP_ENDPOINT` 时安装 OTLP span exporter（HTTP/protobuf，reqwest-blocking，无 tonic）与 `tracing-opentelemetry` layer，未设置时与纯 fmt 行为完全一致；进程退出前必须经 `TelemetryGuard::shutdown()` flush。collector 端点仅支持 `http://`。
- approval waiter 使用 RAII registration guard（Drop 按精确注册身份注销）；`register` 返回 guard+receiver，早命中/读错/cancel 均不泄漏。
- DispatcherHub 条目自清理（task 退出时按 `task.id()` 身份删除自己的 entry），进程关闭时 `abort_all`；dispatcher task 不允许 detached。
- resume 的六字段 snapshot 校验含 `skill_set_version_id` 与有序 `hook_handler_versions`，由 `turn.rs` 的 pinned 常量单一来源同时供写入与校验。
- message admission 的 expected-current-turn 判定先于 status 判定：expected 不匹配即 `stale_turn`（即使 running）；`agent_busy`/`resume_required` 只对 expected 匹配的 running 请求。
- 进程关闭语义：managed task 一旦 spawn 即拥有 claim，`take_tasks` 必须能看到它的 JoinHandle；shutdown 只关 admission 并 drain，绝不 signal turn token。
