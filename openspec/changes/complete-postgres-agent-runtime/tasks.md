## 1. 宪法、领域与归档前置

- [x] 1.1 修订根 `CONSTITUTION.md` 的 crate DAG：删除 mandatory `stratum-store`/`stratum-agent-builtin`，确认 concrete `stratum-postgres` 由装配层调用且 kernel 不依赖 Postgres、HTTP、hosting、pagination 或 scheduler
- [x] 1.2 修订 `CONSTITUTION.md` 的持久化与 realtime 边界：Postgres durable ledger 是唯一执行真相，`stratum-infra` 只提供窄 NATS tail，user-authored conversation 与 runtime-managed credential 的安全边界分开处理
- [x] 1.3 修订 `CONSTITUTION.md` 的 readiness/shutdown 合同：Postgres 决定 core readiness，NATS 只决定 realtime degraded，进程 shutdown 不伪造成业务 cancellation
- [x] 1.4 更新 `ARCH.md`、`CONTEXT.md` 与领域地图：`AgentId = agents.id`、`AgentRuntimeId = agent_states.id`、Session/Turn、AgentRuntime-wide event sequence、四张复数表与 Postgres-before-NATS 边界
- [x] 1.5 更新 `TODO.md`：明确延期 scheduler lease/fencing/multi-instance/rolling takeover/automatic resume/durable cancel/Agent-Workflow 协调，以及 template CRUD/version 浏览/发布/提升/回滚与既有 runtime upgrade
- [x] 1.6 更新根目录和受影响 crate 的 `AGENTS.md`：记录 template version string tag、Agent/AgentRuntime identity、hub-owned dispatcher、完整 product history、PG-confirmed Web barrier 与 kernel restraint
- [x] 1.7 完成 proposal、design 与九份 delta specs 的交叉审查并取得一致；只批准 kernel 的 `AgentVersionId -> AgentId` 机械 pin 替换和既有 prepared-resume seam

## 2. 退役被取代的 Change

- [x] 2.1 将 `add-postgres-execution-storage` 作为 superseded 历史归档，不同步其双后端或旧三表 delta
- [x] 2.2 验证归档 change 不再可作为目标架构应用，并确认 `complete-postgres-agent-runtime` 是唯一活跃 implementation change

## 3. 领域类型、Template Tag 与破坏性数据库基线

- [x] 3.1 删除旧 beta execution migration，建立只创建 `agents`、`agent_states`、`durable_events`、`transcript_compactions` 的单一最终 baseline；不实现原地数据迁移
- [x] 3.2 定义 `agents(id,name,version,definition_schema_version,resolved_definition,created_at)`，使 `version` 为 `TEXT COLLATE "C"` 的作者字符串 tag，并以 `UNIQUE(name,version)` 与数据库 CHECK 固定合法边界
- [x] 3.3 定义 `agent_states(id,agent_id,idempotency_key,status,session_id,current_turn_id,model_config,last_event_seq,created_at,updated_at)`；`agent_id` 以 RESTRICT FK pin `agents.id`，不得增加 outcome/snapshot/usage/hosting/resume/approval/claim 字段
- [x] 3.4 将 `durable_events` 主键/FK/索引全部改为 `(agent_runtime_id,event_seq)` 与 `agent_states.id`，保留 exact Session/Turn、versioned payload、仅 LoopStarted snapshot 和 AgentRuntime-wide 无空洞 sequence
- [x] 3.5 将 `transcript_compactions` 固定为 `(agent_runtime_id,event_seq)`、`turn_id`、`compacted_iteration`、`upto`、`retained_from_event_seq`、`summary` 与 `created_at`；主键/FK绑定durable discriminator，summary shape由对应event_version治理且无独立version，discriminator payload为空且companion永久保留
- [x] 3.6 增加 lifecycle、Session-running partial unique、LoopStarted/terminal/approval uniqueness、payload/version、snapshot AgentId pin、compaction relation、overflow 与完整 public-product history partial indexes
- [x] 3.7 增加 ignored real-Postgres baseline 测试，证明四张复数表、PK/FK/CHECK/index 正确，旧 singular `agent_state` 与全部 forbidden projection/claim/message 表不存在
- [x] 3.8 在 shared domain 中新增 `AgentRuntimeId` UUIDv7 newtype与 validated `AgentVersionTag` string newtype；删除 `AgentVersionId`/`agent_version_id`，并保证两种 ID 不可混用
- [x] 3.9 将 runtime snapshot 的 immutable definition pin 机械改为 `agent_id: AgentId`，严格校验 state pin、snapshot pin 与加载的 definition 三方一致；Hook journal保持kernel-minimal且只依赖外层AgentRuntime/Session/Turn row identity，`AgentRuntimeId` 不进入 kernel event、snapshot、AgentLoop 或 prepared value
- [x] 3.10 用独立 `error.rs` 和 `thiserror` 固定 invalid tag、version conflict、runtime not found、stale/busy/hosting、runtime incompatible、durable corruption、approval、preamble 与 store/runtime/realtime unavailable 的 typed errors和source chain

## 4. Template Catalog 与 Key-only AgentRuntime 创建

- [x] 4.1 保持严格 `[agent].templates_root` 只读目录配置，删除 execution root/backend alias，并验证缺失/非目录/不可读启动失败而空目录合法
- [x] 4.2 扩展 template TOML 为必填 `version` tag：UTF-8 1..=128 bytes、无控制字符、无首尾空白、大小写敏感且不做 trim/casefold/Unicode normalization/SemVer/排序
- [x] 4.3 更新全部 template fixtures/examples 与 `GET /v1/agent-templates` DTO；catalog all-or-nothing 返回安全 name/version/model 信息且不暴露 prompt、tools、raw TOML、path 或 digest
- [x] 4.4 构造严格 canonical v1 `resolved_definition`：只含 prompt、ordered tools、template default model 与非敏感定义身份，不复制 name/version/runtime/effective model/fingerprint/credential
- [x] 4.5 在 `stratum-postgres` 实现 key-first create command：key hit 不读 template；key miss 在同一事务内 recheck key、对 exact `(name,version)` advisory-lock、same-definition reuse/same-tag conflict/different-tag new row，并原子插入 idle runtime state
- [x] 4.6 实现 `POST /v1/agent-runtimes`：只接受 `agent_name` 与可选完整 `model_config`，要求 UUID `Idempotency-Key`，返回固定不可变 `AgentRuntimeCreated` 与 runtime Location；不创建 Session/Turn/task/event
- [x] 4.7 增加 config/catalog/create 单元与 real-PG 测试，覆盖无效 tag、same pair reuse/conflict、different tag、同 definition 多 runtime、key-only different-body replay、并发 same-key、事务失败无 orphan version 与 template 热更新 pin

## 5. Concrete Postgres Runtime Commands、Queries 与 Strict Codec

- [x] 5.1 将 `stratum-postgres` concrete command/query DTO 和全部 SQL 统一到 `AgentRuntimeId`，从 `agent_states.agent_id` join immutable definition；不增加单实现 trait或泄露 pool/sqlx API
- [x] 5.2 实现 AgentRuntime-wide append transaction：事务内锁 exact state、校验 pinned Agent/Session/current Turn/status、checked 分配 sequence、写 row/可选 companion/state side effect，并只在 commit 后返回 receipt
- [x] 5.3 为 definition、event payload、runtime snapshot、approval wire shape 与 companion 实现严格 v1 decode/canonical round-trip；future version 返回 runtime-incompatible，known malformed/unknown field/identity mismatch 返回 durable-state-corrupt
- [x] 5.4 实现同一 MVCC snapshot 的 `AgentRuntimeView` 读取：join definition metadata、state、barrier、keyset-bounded telemetry floor、latest usage、pending approvals；`resume_required` 仅由 exact process registry 外层派生
- [x] 5.5 实现完整 `AgentRuntimeProductEventV1` history pagination，覆盖 LoopStarted、messages、approval request/resolve、compaction、iteration 与全部 terminal，排除 internal journal/ToolExecutionStarted，支持 fixed through、exclusive before、50/256 limit 与 1 MiB soft budget
- [x] 5.6 实现 compaction discriminator+companion 原子写入、latest valid companion fast path、pointer-only in-memory fallback 与 missing/malformed core companion fail-closed
- [x] 5.7 实现 exact AgentRuntime resume slice：固定 base/through、truth range连续性、outer AgentRuntime/Session/Turn identity、state/snapshot/loaded-definition pin、definition/runtime availability、historical baseline 与 current-Turn replay窗口
- [x] 5.8 实现 Tool-result reconciliation：唯一 completion 为 `MessageAppended(role=tool)`；严格校验 result 是assistant tool_calls的有序前缀，未知/重复/稀疏/乱序fail closed，缺失后缀以相同CallId按at-least-once重试，已提交result不重试；只对historical failed/cancelled terminal trailing group做内存normalization，current running不规范化且不改durable/history
- [x] 5.9 实现 AgentRuntime-scoped approval Requested/Resolved/Completed/terminal 派生查询与 resolver transaction，所有参与派生的 version/companion/identity row均strict fail-closed
- [x] 5.10 增加 storage/query/codec/error/recovery 测试，覆盖跨 runtime 隔离、rollback无gap、concurrent writers、完整 product window、barrier派生、compaction、approval race、foreign identity、version/corruption mapping、Tool有序前缀/缺失后缀与historical terminal normalization

## 6. API Runtime Orchestration、Turn、Resume、Cancel 与 Approval

- [x] 6.1 将 process registry、claim、CancellationToken、managed JoinSet 与 cleanup key改为 exact `(AgentRuntimeId,TurnId)`；compare-remove不得清理后来 claim或共享definition的其他runtime
- [x] 6.2 fresh/resume preflight按 AgentRuntimeId加载state与pinned Agent definition，校验 model/provider/tools/skills/extensions/hooks、snapshot AgentId与outer AgentRuntime/Session/Turn identity，并保持所有 durable mutation/外部动作前 fail-closed
- [x] 6.3 实现 `POST /v1/agent-runtimes/{agent_runtime_id}/messages` strict DTO、64 KiB limit、原样text、nullable exact current-Turn CAS、Session bind/reuse/单活与完整 model replacement
- [x] 6.4 保留 LoopStarted 与首条 user MessageAppended 两个 durable boundary；fresh admission在写前取得dispatcher handle并交给bound sinks/managed task持有到Turn退出，只有 managed task安装与第二次commit后返回含两类 Agent identity/Session/Turn 的202，model_config只在已接受message且变化时更新
- [x] 6.5 将 terminal append/status、started-only reconciliation、commit-uncertain reread、post-terminal next Turn、shutdown/admission drain 与 exact claim cleanup统一到 AgentRuntime identity，不持久化 outcome/usage/cancel intent
- [x] 6.6 实现 `POST /v1/agent-runtimes/{agent_runtime_id}/resume`：先完成不依赖bound sink的fixed preflight，再hub ensure、纯`prepare_resume`、短state-row-lock重验definition pin/Turn、202/204语义、started-only typed failure与不确定commit处理；prepare失败无durable write/外部动作，handle由API-owned sinks/managed task持有且不进入kernel event/prepared value
- [x] 6.7 实现 exact runtime/Turn cancel：仅signal hosted running token，starting/unhosted/stale/not-running typed response，202/204空body，不abort future、不持久化intent
- [x] 6.8 将 approval Handler 绑定 exact AgentRuntime ledger，执行 register-then-read-and-bounded-poll，按 cancellation/notify/内部固定上限tick 重读并在决定/取消/shutdown/error后注销；删除用户可配置的 `approval_poll_interval_seconds` 及config/docs/tests残留，approve/reject只映射普通 Hook decision
- [x] 6.9 实现 runtime-scoped approval endpoint与waiter：resolver写前ensure handle、state-row-lock线性化、same-decision 204、conflict/terminal typed error、commit-before-receipt/notify，resolve不隐式resume
- [x] 6.10 固定当前closed composition边界：仅注册无credential通道的Echo，arguments/result作为user-authored opaque conversation data，authorization仅含typed ToolKind/DangerLevel，result仍经过AfterToolCall；strict HTTP DTO拒绝专用credential字段，代码与TODO明确credential-aware Tool必须由后续独立PATCH提供reference/provider/fail-closed transform后才可注册，不宣称通用secret扫描或脱敏
- [x] 6.11 统一 API error envelope、4xx/5xx tracing边界、CORS Idempotency-Key、JSON body limit与 runtime/template/approval 404 区分，确保错误不泄漏 SQL/path/subject/prompt/tool/provider/credential
- [x] 6.12 更新 utoipa/OpenAPI 为 12 个最终 endpoints、`AgentRuntimeCreated`/`AgentRuntimeView`/command DTO、完整 product/frame union、decimal sequence、明确空body与实际 error schemas
- [x] 6.13 增加 API orchestration 测试，覆盖 create→message、两个preamble窗口、CAS/Session/model、shared definition多runtime、resume/cancel、approval polling/notify loss、Echo-only registry/approval/AfterToolCall边界与专用credential字段拒绝、terminal/cleanup/shutdown与OpenAPI合同

## 7. AgentRuntime NATS Tail、Dispatcher 与 SSE

- [x] 7.1 将 `stratum-infra` 窄tail API、subject、cursor validity与 retention 全部改为 AgentRuntimeId scope；保留有限 age/bytes/messages discard-old并删除旧Agent/Session transport naming
- [x] 7.2 定义 API-owned `AgentRuntimeStreamFrameV1` 和 `AgentRuntimeProductEventV1`，每帧携 exact runtime + pinned Agent 双 fence，durable/telemetry sequence为decimal string且unknown variant/version fail closed
- [x] 7.3 实现 hub-owned `ensure(AgentRuntimeId)`：无caller frontier，在per-runtime gate内线性化 committed PG high-water读取、generation安装、live handle获取、normal retirement与degraded abandonment；ensure只做PG读取和本机注册，不等待NATS publish
- [x] 7.4 实现 ordered dispatcher：所有 durable writer写前持handle跨commit、receipt只推进已有generation且不等待NATS/queue容量，满队列只合并单调high-water；按PG sequence扫描完整truth、跳过internal row、处理乱序receipt，并以“先snapshot target→确认accepted queue为空→只flush旧snapshot”保证coalesced顺序
- [x] 7.5 实现 live-handle retirement不变量和NATS持续失败的bounded retry/degraded abandonment；零handle后只丢volatile queue/target，下一ensure从最新PG high-water开始且永不跨重启补发旧history
- [x] 7.6 实现 AgentRuntime-bound TelemetryEventSink：call-local sequence、入队时冻结 `durable_before_event_seq`、final前队列顺序、bounded telemetry drop/gap与下一call隔离
- [x] 7.7 实现 `/v1/agent-runtimes/{agent_runtime_id}/events` current-tail/retained-cursor SSE：subscribe-before-ready、410 expiry、503 degraded、unknown query拒绝、bounded connection buffer与无id local stream_reset
- [x] 7.8 增加 dispatcher/infra/API 单元与 real-NATS integration tests，覆盖shared-definition runtime隔离、ensure/retire竞态、commit-before-publish、writer/receipt乱序、queue full、publish loss、abandon/restart、cursor与overflow

## 8. Web AgentRuntime Client、Recovery 与 History

- [x] 8.1 将 Web API/types/hooks 从 Agent runtime-as-AgentId 改为 AgentRuntimeId routes与 DTO，同时保留 pinned AgentId/name/version metadata；create pending intent使用key-only replay语义
- [x] 8.2 更新创建与选择流程：catalog展示作者tag，`POST /v1/agent-runtimes`成功后选择runtime；同template版本的多个runtime作为独立conversation，不把AgentId当recent conversation key
- [x] 8.3 严格解析 `AgentRuntimeStreamFrameV1`/完整 product union，验证runtime+definition双identity、positive safe protocol/version字段、decimal sequence和所有variant字段，reset无id；identity mismatch不得应用frame
- [x] 8.4 实现 subscribe-before-snapshot cold bootstrap：ready后读取view+latest page、只提交成功snapshot、PG barrier初始化、view telemetry floor、丢弃cold telemetry、cursor/reset/EOF竞态与无cursor重建；双identity mismatch必须关闭stream并cold bootstrap，新view仍不一致则protocol identity error且不循环重连
- [x] 8.5 reducer以 `(AgentRuntimeId,event_seq)` 管理durable product，以 `(AgentRuntimeId,LlmCallId,telemetry_seq)` 管理draft；NATS product只进入unconfirmed map，不推进PG-confirmed barrier
- [x] 8.6 实现完整 `(B,T]` reconcile与原子rebase：分页读取全部product、当前unconfirmed map去重、view@T后重放`>T` frames、single-flight/coalesced rerun与stale generation丢弃；timer/focus/command不得取消当前慢分页，只合并至多一次补跑
- [x] 8.7 实现 exact accepted/running Turn telemetry fence、durable watermark、assistant final replacement、late/old-Turn delta拒绝及所有terminal状态下draft/interrupted Tool清理
- [x] 8.8 实现 approval refresh/resolve 与 explicit resume分离、cancel待确认、realtime degraded低频PG reconcile、command/focus/ready后立即reconcile
- [x] 8.9 最新history首屏与fixed-barrier向上分页只渲染message/compaction/safe terminal marker，严格解码并推进完整product cursor但不让旧control fact覆盖current view
- [x] 8.10 增加 Web API/parser/reducer/recovery/message-intent/recent-runtime tests，覆盖双identity mismatch的cold-bootstrap/error闭环，并通过 typecheck、lint与production build

## 9. Kernel 克制与旧路径删除证明

- [x] 9.1 删除 `AgentVersionId` 及旧 runtime-as-AgentId 命名；kernel只用 `AgentId` pin immutable definition，API/storage/infra才持有 `AgentRuntimeId`
- [x] 9.2 删除 `stratum-store`、`stratum-agent-builtin`、filesystem execution/CAS、旧 generic EventBus、Session SSE、message_seq/per-Turn sequence、backend selector与全部生产fallback，不保留compatibility island
- [x] 9.3 审计 `stratum-agent` diff：除已批准的 AgentId pin机械替换、prepared-resume seam与旧transport删除外，不引入Postgres、AgentRuntimeId、Session hosting、event_seq、approval projection、pagination、scheduler或前端状态
- [x] 9.4 对 production code、manifest、config、tests与docs运行 deletion-proof `rg`，证明无 AgentVersionId、singular agent_state、旧runtime `/v1/agents`、旧frame/AgentView、旧AgentStore/filesystem/bus/sequence/replay/root/fallback 的活跃实现残留；只允许明确的删除说明与负向拒绝测试
- [x] 9.5 更新 Docker/config/examples/operator docs以说明破坏性PG与NATS重建、四表复数schema、string tag、AgentRuntime routes、NATS短tail、explicit resume/cancel at-least-once边界

## 10. 验证、故障注入、独立审查与归档准备

- [x] 10.1 运行全部 Rust 单元与 all-target tests，并运行 crate-local ignored real-Postgres suites验证schema/create/append/view/history/resume/approval/compaction/race
- [x] 10.2 重建 disposable NATS资源并运行 real-NATS/dispatcher/API integration suites，验证runtime隔离、cursor、overflow、publish loss与realtime degraded恢复
- [ ] 10.3 在真实 Postgres+NATS+API+Web 上验证 create → message → durable stream → Tool/approval → cancel或resume → process restart → refresh → upward pagination 端到端路径
- [ ] 10.4 自动化或按 `ALPHA_TEST.md` 人工记录两个preamble窗口、commit不确定、resolver/kernel竞态、NATS loss/slow/full/expiry/overflow、compaction pointer、approval crash、Tool at-least-once与cancel race的故障注入证据
- [x] 10.5 运行 `cargo fmt --all -- --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace --all-targets`，不得通过压制lint规避失败
- [x] 10.6 运行 Web full test、typecheck、lint、format与production build，并检查无未处理Promise/stream/reconcile竞态
- [x] 10.7 运行 `openspec validate complete-postgres-agent-runtime --type change --strict --no-interactive` 与 `openspec validate --all --strict`，确认 superseded change不会污染main specs
- [x] 10.8 以实现前固定点对完整diff执行 `code-review` 的 Standards/Spec 双轴审查，修复全部 P0/P1/P2 findings
- [x] 10.9 派发独立 `constitution-review` 子代理逐条检查完整diff，修复全部 red flag、violation 与高风险 constitution gap
- [x] 10.10 修复审查finding后重新运行 OpenSpec、Rust、real PG/NATS、Web、deletion proof 与 kernel-restraint 全部门禁
- [x] 10.11 逐项核对checkbox与具体evidence，以最终implementation convention更新受影响 `AGENTS.md`，并在merge前提醒用户确认归档文档
- [ ] 10.12 只有implementation、真实验证、独立审查与文档全部完成后，才准备sync/archive `complete-postgres-agent-runtime`；本次apply不提前归档
