# Alpha 手工验证清单

## 目的

本文档记录 `complete-postgres-agent-runtime` 在 Alpha 阶段仍需人工控制真实进程、网络、Postgres、NATS 或浏览器时序才能可信验证的场景。它不是 HTTP 或事件协议的第二份定义；行为冲突时以 OpenSpec、生成的 OpenAPI 和 crate `AGENTS.md` 为准。

关联但不会因本文档存在而自动完成的 OpenSpec gate：

- `9.10`：真实 NATS/API/Web 集成；
- `11.3`：每个 transaction、crash window、race、version boundary 与 typed error；
- `11.5`：真实 Postgres + NATS 的完整端到端路径；
- `11.6`：preamble、commit uncertainty、NATS loss、cursor、overflow、cleanup 与 compaction fallback；
- `12.6`：完成全部验证后才能同步和归档 change。

已经能由普通单元测试或 crate-local ignored integration test 稳定覆盖的情况不应只保留在这里。人工场景一旦可以确定性自动化，应迁入对应 crate/Web 测试，并把本文件保留为发布验收入口。

## 安全边界

1. 只使用可丢弃的 Alpha Postgres database、NATS stream 和测试 Agent。不得对开发共享库、生产库或用户历史做 SQL corruption、drop、retention eviction 或进程注入。
2. 当前是单进程、单一可信操作者的 Alpha：入站 API 没有 auth/authz 或 tenant isolation，CORS 不是认证。API 只能绑定 loopback/受控私网，或置于带 TLS 与认证的反向代理后；Postgres 与 NATS 端口不得暴露公网。
3. 只使用合成测试数据。prompt、Tool arguments/result、approval、summary、resolved template 与 compaction 前原始消息都会持久化，当前核心资产没有 delete API；真实 LLM 还会外发上下文并产生费用，只能使用低权限、限额测试 key。
4. 本次 cutover 会 drop/rebuild 执行数据库与 NATS tail，不提供旧 beta schema 的原地 migration。操作前必须备份；回滚必须把 binary、schema、config 与 NATS state 作为一组恢复。
5. `config.toml`、真实 provider key 与连接凭据不得加入 Git；本地 secret 文件至少限制为 owner-only 权限。示例凭据必须在 Alpha 环境替换，证据中也不得出现其值。
6. 破坏性步骤前记录 database、stream、API/Web commit 和容器身份；结束后清理测试资源，不复用已被手工篡改的数据库。集成测试会 TRUNCATE 核心表或删除目标 stream，其连接地址必须再次人工确认不是共享/真实环境。
7. 不向生产代码增加公开 failpoint、debug endpoint、绕过鉴权的管理入口或第二套状态。精确窗口优先使用 debugger、测试 binary、TCP proxy、容器 pause/stop 或一次性测试配置。
8. 证据不得包含 API key、token、credential value、原始 provider body、prompt、Tool arguments/result、SQL connection string 或用户对话正文。数据库证据只记录安全 identity、event type、version、sequence 和状态。
9. `kill -9`、网络切断和直接 SQL mutation 都必须作用于已确认的精确 PID、容器、database 与 AgentId；禁止使用宽泛进程匹配或未解析变量。
10. Alpha 验证不得扩大当前版本保证：不测试或承诺 scheduler lease/fencing、多实例自动接管、rolling deploy、自动 resume、durable cancel、并发 Tool、通用 Tool 幂等、template management、Workflow 协调或 NATS durable backlog。
11. crash 后的 running Turn 需要显式 resume；cancel `202` 只是进程内 signal，shutdown 也不等于业务 cancel。Tool 在 `ToolExecutionStarted` 后崩溃会以同一 CallId 至少一次重做，副作用去重由 Tool/service 负责；approval resolve 与 resume 保持分离。
12. Tool 与 agent 生成的命令只能在容器或等价沙箱中运行，禁止直接在宿主机执行。副作用目标必须是 mock/隔离资源，并使用非生产凭据。

## 每轮记录

执行前填写：

| 字段 | 值 |
|---|---|
| Git commit |  |
| 日期 / 执行人 |  |
| API / Web 地址 |  |
| Postgres / NATS 版本与测试资源 |  |
| 浏览器 / OS |  |
| Evidence 目录或 CI URL |  |

每个场景至少保存：

- 带时间戳的操作顺序和故障注入点；
- HTTP status 与稳定 `error.code`，不保存敏感正文；
- AgentId、SessionId、TurnId、ApprovalId/CallId（适用时）；
- 故障前后 `agent_state.status/current_turn_id/last_event_seq`；
- 按 event_seq 排序的安全 event type 清单及是否连续；
- SSE control/durable/telemetry 的 identity、sequence 与 cursor 行为；
- 浏览器最终可见状态、是否需要 refresh/resume，以及是否出现重复消息、ghost draft 或伪造 terminal；
- 进程和依赖恢复后的最终结果。

可使用以下只读查询记录安全证据；不要在证据中选择 payload、summary 或 definition：

```sql
SELECT status, session_id, current_turn_id, last_event_seq
FROM agent_state
WHERE agent_id = '<agent_uuid>';

SELECT event_seq, turn_id, event_type, event_version
FROM durable_events
WHERE agent_id = '<agent_uuid>'
ORDER BY event_seq;

SELECT event_seq, turn_id, compacted_iteration, upto, retained_from_event_seq
FROM transcript_compactions
WHERE agent_id = '<agent_uuid>'
ORDER BY event_seq;

SELECT expected.event_seq AS missing_event_seq
FROM generate_series(
  1,
  (SELECT last_event_seq FROM agent_state WHERE agent_id = '<agent_uuid>')
) AS expected(event_seq)
LEFT JOIN durable_events AS actual
  ON actual.agent_id = '<agent_uuid>'
 AND actual.event_seq = expected.event_seq
WHERE actual.event_seq IS NULL;
```

通用通过条件：Postgres 是唯一 durable truth；已提交事实不因 NATS 或 HTTP 响应丢失而回滚；未提交事务不消耗 event_seq；不确定结果不靠猜测；恢复不创建替代 Agent/Session/Turn；错误保持 typed、fail-closed 且不泄露敏感信息。

## 场景索引

| ID | 场景 | 主要 OpenSpec gate | 结果 |
|---|---|---|---|
| A01 | 完整真实端到端路径 | 9.10, 11.5 | [ ] |
| A02 | 两个 message preamble 边界 | 11.3, 11.6 | [ ] |
| A03 | Started-only terminal COMMIT 结果不确定 | 11.3, 11.6 | [ ] |
| A04 | 运行中进程崩溃、exact resume 与 stale cleanup | 11.3, 11.5, 11.6 | [ ] |
| A05 | Tool 至少一次的两个崩溃窗口 | 11.3, 11.5 | [ ] |
| A06 | Approval refresh、崩溃恢复与 terminal race | 9.10, 11.3, 11.5 | [ ] |
| A07 | Cancel 的内存级语义与崩溃限制 | 11.3, 11.5 | [ ] |
| A08 | NATS down、post-commit publish loss 与恢复 | 9.10, 11.5, 11.6 | [ ] |
| A09 | Cursor retention expiry 与 stream generation 变化 | 9.10, 11.6 | [ ] |
| A10 | SSE bounded-buffer overflow 后冷恢复 | 9.10, 11.6 | [ ] |
| A11 | Subscribe-before-snapshot、reconcile 与迟到 telemetry | 9.10, 11.5 | [ ] |
| A12 | Postgres outage、readiness 与破坏性 baseline | 11.3, 11.5 | [ ] |
| A13 | Compaction pointer fallback 与核心事实损坏 | 11.3, 11.6 | [ ] |
| A14 | 严格持久版本、shape 与 identity fail-closed | 11.3 | [ ] |
| A15 | Graceful shutdown 的统一 deadline | 11.3, 11.5 | [ ] |

## A01 — 完整真实端到端路径

### 前置条件

- 全新的 Alpha database 与 Agent-scoped NATS stream；
- 真实 API、Web 和一个可控的 mock LLM/Tool composition；
- Tool 能产生需要人工 approve/reject 的调用，并能记录不含参数的调用次数与 CallId；
- 预置足够多的安全消息，以便最终出现两页以上 history。

### 操作

1. 从 Web 读取 template/model catalog，创建 Agent，确认创建本身仍为 idle 且没有 Turn event。
2. 发送首条消息，观察 `stream_ready`、`LoopStarted`、user message、LLM telemetry、完整 assistant message与 terminal。
3. 开始下一 Turn，触发 Tool approval；分别完成一次 approve 路径和一次 reject 后继续运行路径。
4. 在一个 running Turn 中停止 API 进程，使用同一 Postgres/NATS 启动新 API 进程，刷新页面并显式 resume exact Turn。
5. 对另一个 running Turn 发 cancel；只在 durable terminal 到达后确认最终状态。
6. 刷新页面，向上滚动直到跨越至少两个 history page，并展开 compaction marker（若该 fixture 会触发 compaction）。

### 预期

- create → message → stream → approval/tool → cancel/resume → restart → refresh → pagination 全链路不依赖 filesystem execution 或 projection table；
- 所有 durable item 以 `(AgentId,event_seq)` 去重，event truth 连续；公开过滤视图允许跳号；
- 刷新后 pending approval、status、usage、telemetry floor 与 history 从同一 PG barrier 收敛；
- resume 沿用原 AgentId、SessionId 和 TurnId，不追加第二个 `LoopStarted`；
- Tool result 只由 `MessageAppended(role=tool, tool_call_id=CallId)` 表示；
- 旧消息只在向上滚动时加载，固定 through barrier 不混入更新数据。

## A02 — 两个 message preamble 边界

### A02.1：`LoopStarted` COMMIT 前停止

1. 在 exact starting claim 已安装、`LoopStarted` transaction COMMIT 前暂停 API。
2. 终止 API 进程并启动一个连接同一 PG 的新进程。
3. 读取 AgentView 与 durable ledger。

预期：事务完全回滚；原 status/current Turn/default model/high-water 不变；不存在新 `LoopStarted`；旧进程 claim 不会成为 durable hosting truth；合法 CAS 仍可重新发送消息。

### A02.2：`LoopStarted` 已提交、首条 user message 未提交

1. 暂停在 `LoopStarted` COMMIT 成功之后、首条 `MessageAppended(role=user)` COMMIT 之前。
2. 终止 API 并以空 registry 重启。
3. 验证 Agent 为 `running + unhosted`、`resume_required=true`，该 Turn 只有一个 `LoopStarted`，default model仍是此前值。
4. 调用 exact resume。

预期：原 message 请求没有返回 202；resume 不调用 LLM/Tool/Hook，原子追加唯一安全 `LoopFailed`并把 state 置 failed，返回 `409 turn_preamble_incomplete`；Session/current Turn 保留，event_seq 无洞。

## A03 — Started-only terminal COMMIT 结果不确定

### 操作

1. 先按 A02.2 构造 started-only Turn。
2. 在 resume 写入安全 `LoopFailed` 时，通过数据库 TCP proxy 让 COMMIT 请求可能到达 PG，但在 client 收到最终确认前切断连接。不要用“handler 返回后丢 HTTP response”替代这个数据库 commit-ack 窗口。
3. 让 API 的 exact-Turn reread 分支运行；若读取也暂时失败，恢复 PG 后再次读取 AgentView/ledger。

### 预期

只允许两类 durable 结果：

- COMMIT 已落地：存在唯一 `LoopFailed`，state=failed，API 依据 reread 映射为 preamble incomplete；
- COMMIT 未落地：Turn 仍 running/unhosted，high-water 未推进，API 返回 typed store unavailable。

不得出现第二个 terminal、已推进但缺 row 的 high-water、猜测性 failed 状态或重复 event_seq。

## A04 — 运行中进程崩溃、exact resume 与 stale cleanup

### 操作

1. 让一个正常 Turn 已提交首条 user message，并让 mock provider保持运行中。
2. 记录 Agent/Session/Turn identity 后 `kill -9` API。
3. 使用同一 PG/NATS 启动新 API；刷新 Web，确认 view仍是 durable running 但 registry为空。
4. 并发发送两个 exact resume 请求。
5. 在新 claim 安装后，允许旧 task cleanup（或等价的延迟 cleanup harness）继续执行。

### 预期

- 新进程报告 `resume_required=true`；不会自动接管，也不会产生 terminal；
- 两个 resume 至多一个返回 202 并启动 task，另一个返回 204；
- resume 保持原 AgentId/SessionId/TurnId且不追加第二个 `LoopStarted`；
- 旧 cleanup 只能 compare-remove 自己的 claim，不能删除新 handle；
- publisher 从启动时 PG high-water开始，不向新 subscription重灌旧 NATS history。

## A05 — Tool 至少一次的两个崩溃窗口

使用只记录 `CallId` 与调用次数、且允许相同 CallId 重复的测试 Tool；不要把 runtime 测试误写成通用 Tool 幂等保证。

### A05.1：Started 已提交、result 未提交

1. 暂停在 `ToolExecutionStarted` durable COMMIT 后、role=tool result COMMIT 前。
2. 可分别在“外部副作用前”和“外部副作用后”终止 API。
3. 重启并显式 resume。

预期：仅缺失的有序 Tool 后缀使用原 CallId 重试；外部调用可能发生两次，这是当前明确的 at-least-once 语义；不得发明 AttemptId 或 `ToolExecutionCompleted`。

### A05.2：result 已提交、后续 iteration 前停止

1. 暂停在同 CallId 的 `MessageAppended(role=tool)` COMMIT 后、下一模型调用前并终止 API。
2. 重启并 resume。

预期：已提交 result 不再次调用 Tool；恢复从该 result继续。若 result 无法安全脱敏，ledger/NATS/log中只能存在安全结构化 tool error，不能出现原始 credential-bearing output。

## A06 — Approval refresh、崩溃恢复与 terminal race

### 操作

1. 触发 approval Requested，但在 Web 接收 realtime frame 前断开页面；刷新并确认同一 ApprovalId 从 AgentView重新出现。
2. 在 Requested 后、Resolved 前终止 API；重启后对 unhosted Turn resolve。确认 resolve 只持久化决定，不自动 resume。
3. 显式 resume，暂停在 Resolved COMMIT 后、matching `HookInvocationCompleted` 前，再次终止并恢复。
4. 使用两个独立 client同时提交 approval resolve 与 exact Turn terminal/cancel completion，重复运行直到观察到两种合法线性化顺序。
5. 在一次 resolve 已提交后丢弃 HTTP response，以相同 decision 与相反 decision分别重试。

### 预期

- Requested、Resolved、Consumed、Invalidated 都只由 ledger facts派生；NATS 与 waiter notification丢失不改变决定；
- unhosted resolve 返回 204但保持 running/unhosted，Web明确要求 Resume；
- Resolved 后恢复不重新询问；Completed 后恢复不再次调用 approval Handler；
- resolve 先线性化时可提交唯一 Resolved；terminal 先线性化时 resolver返回 `approval_invalidated`；event_seq始终连续；
- 相同 decision重试为204且不追加，反向 decision为409；
- terminal 后绝不执行 Tool。

## A07 — Cancel 的内存级语义与崩溃限制

### 操作

1. 对 hosted running exact Turn 发 cancel，记录202后立即检查PG与Web。
2. 一次让 AgentLoop先正常完成；另一次让 cancellation最终形成 `LoopCancelled`。
3. 再构造一次 cancel已返回202但terminal尚未提交的窗口，立即终止API并重启。
4. 分别对 starting、unhosted running、stale Turn和已cancelled exact Turn调用cancel。

### 预期

- 202只表示token已signal；PG在terminal COMMIT前仍为running，Web不得提前显示cancelled；
- 正常完成可赢得竞态，且不会补写第二个terminal；
- 进程崩溃会丢失cancel intent，这是已知限制；重启后Turn为running/unhosted，需要显式resume，不得伪造cancelled；
- starting/unhosted/stale/already-cancelled分别得到稳定typed结果，任何错误Turn都不能影响新Turn token。

## A08 — NATS down、post-commit publish loss 与恢复

### 操作

1. 在PG和API健康时停止或隔离NATS，保持Postgres可用。
2. 从Web/API提交message、approval与可产生terminal的操作；记录HTTP结果和PG ledger。
3. 让至少一个durable COMMIT成功后故意使对应NATS publish失败。
4. 恢复NATS连接，让Web保持当前页面并等待PG reconcile；随后重新建立SSE。
5. 另一次通过TCP proxy把NATS publish持续放慢，让模型产生超过dispatcher queue容量的小delta；观察queue saturation后恢复NATS。

### 预期

- PG-backed command仍按durable合同成功，readiness中realtime显示degraded；SSE建立失败为`503 realtime_unavailable`而不是store failure；
- publish失败不回滚PG、不让sink重试append、不改变HTTP/kernel结果；
- Web通过AgentView/history补齐缺失durable frame并清理terminal UI；
- 已接受telemetry可以丢失并形成incomplete draft，但完整assistant message最终收敛；
- 慢NATS和满队列不得阻塞PG acknowledgement；durable wake必须合并并最终追平，telemetry drop只能造成可检测gap；
- NATS恢复后只提供新tail，不补发全部PG历史。

## A09 — Cursor retention expiry 与 stream generation 变化

### 操作

1. 在浏览器当前页面保存一个真实SSE cursor，然后断开连接。
2. 在独立Alpha stream上通过缩短retention或发布足量安全frames淘汰该position。
3. 使用原cursor短重连，确认410后观察Web恢复。
4. 另一次保存cursor后重建Alpha NATS stream/generation，再尝试重连。
5. 刷新页面，观察请求不得携带刷新前cursor。
6. 在PG已有大量历史时重启API；新建无cursor subscription，并在产生新Turn前观察NATS tail。

### 预期

- expired、future或旧generation cursor在建流前明确失败，不能从“未来位置”等待并静默漏tail；
- Web清cursor、draft与transient buffer，以无cursor subscription执行PG cold bootstrap；
- 页面刷新从不复用旧cursor；cursor不与event_seq/telemetry_seq比较；
- API重启后的dispatcher从PG high-water初始化，新subscription不会收到重启前全部durable历史；
- 最终durable timeline与PG barrier一致且没有重复。

## A10 — SSE bounded-buffer overflow 后冷恢复

### 操作

1. 建立Agent SSE并收到`stream_ready`，随后通过代理/debugger阻塞AgentView或history，使cold bootstrap不消费实时buffer。
2. 对同一Agent快速产生超过服务端bounded buffer容量的安全frames；不得通过修改生产常量来伪造结果。
3. 观察最后一个control frame、SSE id和连接关闭；继续观察浏览器下一次subscription。

### 预期

- 旧连接发送一次无SSE id的`stream_reset { reason: "buffer_overflow" }`后关闭；reset不进入PG/NATS；
- Web主动关闭旧EventSource，丢弃该连接buffer、draft和page cursor，阻止浏览器用`Last-Event-ID`自动短重连；
- 新连接无cursor并重新执行cold bootstrap；最终PG durable内容完整且无ghost draft；
- 任何本地Web hard-cap overflow都执行相同恢复语义。

## A11 — Subscribe-before-snapshot、reconcile 与迟到 telemetry

### 操作

1. 让SSE先ready，再延迟AgentView/history响应；延迟期间提交一个durable product event。
2. 在cold buffer中只注入某个LLM call的中段telemetry，不提供完整prefix。
3. 进入live后使NATS丢失LoopStarted或terminal frame，保留PG事实。
4. 制造超过一页的 `(B,T]` durable gap，并把每页读取延迟到超过一次poll interval；同时触发focus和command reconcile。
5. 让PG reconcile先应用assistant final F，再释放`durable_before_event_seq < F`的旧telemetry；随后开始下一Turn并释放上一Turn的迟到telemetry。
6. 在snapshot进行中让SSE于ready之后断开或报错。
7. 在LLM draft进行中把真实浏览器切为offline，让后端完成Turn；恢复网络并触发focus/reconnect。
8. 在页面保存过SSE id且Turn仍running时执行浏览器hard refresh。
9. 预置超过一页history，记录初始through barrier；向上分页期间从第二client提交新Turn并让最新barrier前进。

### 预期

- snapshot期间event要么在barrier内去重，要么以`> barrier`应用一次，不丢不重；
- cold-buffer telemetry全部丢弃，不以残缺prefix构造draft；
- accepted Turn维持PG polling，直到AgentView或同Turn durable frame证明；terminal丢失仍由PG清理draft/Tool UI；
- reconcile保持single-flight，周期/focus/command只coalesce一次补跑；每个fetch deadline能释放永久挂起请求；
- final前旧tail和跨Turn旧telemetry均被拒绝，final后的新call不被误杀；
- ready后连接死亡会作废本轮snapshot和cursor，而不是提交有恢复空洞的view。
- offline恢复后PG final替换残缺draft，terminal/usage/Tool UI最终一致；不会永久停在running；
- hard refresh后的首个events请求不复用旧page cursor，进行中的残缺telemetry不被当作完整prefix；
- 向上分页始终使用原through barrier和单调exclusive-before，新事件只由live/reconcile合并，不污染旧窗口。

## A12 — Postgres outage、readiness 与破坏性 baseline

### 操作

1. 用含已删除beta migration history的可丢弃database启动新API。
2. 记录拒绝启动后，按运维文档重建空database与migration history，再启动API。
3. API健康后停止或隔离Postgres，分别请求health、AgentView、history与一个command；保持NATS可用。
4. 恢复Postgres并等待连接池恢复，再次请求health和只读/command endpoint。
5. 在另一个一次性database上安装一次性trigger，让普通append在durable row insert后、state update时失败；移除trigger后再次append。

### 预期

- 新binary拒绝旧migration history，不做原地升级、不创建filesystem fallback；
- clean baseline只包含四张执行核心表；
- PG不可用时core readiness失败，核心endpoint返回typed `store_unavailable`，不能从NATS猜状态；
- PG恢复后readiness和核心操作恢复，之前未提交的transaction不留下event_seq空洞；
- 中途失败的append必须整体回滚event、state与companion；移除trigger后的下一次成功append复用同一个next event_seq；
- 错误响应和日志不泄露SQL、connection string或credential。

## A13 — Compaction pointer fallback 与核心事实损坏

仅在一次性database中直接SQL mutation；每个子场景从干净fixture重新开始。

### A13.1：Companion insert 原子失败

1. 在可触发compaction的fixture上，为`transcript_compactions`安装只失败一次的`BEFORE INSERT`测试trigger。
2. 触发真实compaction，记录kernel错误和三张相关表；移除trigger后继续运行。

预期：`TranscriptCompacted` discriminator、companion和state high-water一起回滚；kernel没有收到durability acknowledgement；后续append复用同一个next event_seq。

### A13.2：Compaction COMMIT 后的进程边界

1. 在`TranscriptCompacted` transaction COMMIT后、后续iteration durable boundary前暂停并终止API。
2. 重启并resume exact Turn，与无crash基准比较provider context和summary Hook调用次数。

预期：discriminator与companion各只有一条；恢复复用已提交compaction，不重复执行summary Hook或追加第二个compaction；context与健康基准等价。

### A13.3：加速pointer失效

1. 创建包含多次compaction与后续running Turn的fixture，记录未篡改时的provider context摘要和ledger high-water。
2. 仅把最新companion的`retained_from_event_seq`改成schema允许、但provenance校验失败的MessageAppended位置；保持discriminator、identity和summary完整。
3. 重启并resume。

预期：忽略pointer，从event_seq 1做纯内存full replay；恢复context与未篡改fixture字节级等价；数据库没有repair/rebuild写入，原始message与companion仍永久存在。

### A13.4：必需companion/summary损坏

1. 从新fixture删除必需companion，或写入可通过DB CHECK但无法按v1严格解码的summary。
2. 调用AgentView/history/resume中会读取该事实的路径。

预期：返回`durable_state_corrupt`并且任何LLM/Tool/Hook动作都未开始；不得把核心事实缺失降级为pointer miss、filesystem fallback或在线repair。

## A14 — 严格持久版本、shape 与 identity fail-closed

在不同的一次性fixture中分别注入：

- 高于当前支持范围的definition、event或runtime snapshot version；
- v1 event/snapshot/approval payload的未知字段、非canonical默认值或非法嵌套variant；
- `agent_state.last_event_seq`与truth rows不一致、current-Turn slice缺行或row属于错误Session/Turn；
- 非`TranscriptCompacted` event挂companion、companion与discriminator identity不一致；
- approval/hook派生查询中的foreign-Turn或非法version matching fact；
- Tool result未知、重复、稀疏、乱序或脱离assistant group。

预期：结构完整但未来版本映射`runtime_incompatible`；受支持版本的损坏映射`durable_state_corrupt`；所有读入口一致fail-closed，dispatcher不得跳过未知persisted variant并推进frontier，resume不得开始外部动作，error body/log不得泄露被注入payload。

## A15 — Graceful shutdown 的统一 deadline

### 操作

1. 分别让API存在：正在admit的HTTP请求、running Turn、等待approval的Handler、安静的SSE pump、慢provider stream。
2. 发送SIGTERM/SIGINT并记录shutdown开始时间；在窗口内再发一个新command。
3. 一轮让任务在deadline内自然结束；另一轮让协作方持续挂起直到超过`shutdown_drain_timeout_seconds`。

### 预期

- shutdown开始后停止新admission并返回稳定503；server、admission和managed runtime tasks共享同一个剩余deadline，不串联多个完整timeout；
- shutdown不会把进程生命周期转换成业务`LoopCancelled`，不会写cancel intent或猜测terminal；
- deadline内完成的PG commit正常保留；未提交transaction回滚且event_seq不消耗；
- deadline到期后进程退出，不遗失已启动但从registry移除后变成detached的Turn task；
- SSE pump、dispatcher和telemetry provider被有界结束，错误退出仍flush telemetry provider的安全日志链。

## 结果汇总

完成一轮后更新本表；失败必须关联issue，不得用“符合预期”覆盖恢复空洞、重复外部动作或已知限制之外的行为。

| ID | Commit | 结果（PASS/FAIL/BLOCKED） | Evidence | Issue / 备注 |
|---|---|---|---|---|
| A01 |  |  |  |  |
| A02 |  |  |  |  |
| A03 |  |  |  |  |
| A04 |  |  |  |  |
| A05 |  |  |  |  |
| A06 |  |  |  |  |
| A07 |  |  |  |  |
| A08 |  |  |  |  |
| A09 |  |  |  |  |
| A10 |  |  |  |  |
| A11 |  |  |  |  |
| A12 |  |  |  |  |
| A13 |  |  |  |  |
| A14 |  |  |  |  |
| A15 |  |  |  |  |

## Alpha 结束条件

只有在以下条件同时成立后，才能把相关 OpenSpec checkbox 改为完成并进入sync/archive：

1. A01完整链路通过；
2. A02、A03、A04证明两个preamble与进程/commit crash window；
3. A05、A06、A07证明Tool、approval、cancel的恢复边界与明确限制；
4. A08—A11证明真实NATS/SSE/Web在丢失、过期、overflow和慢reconcile下最终由PG收敛；
5. A12—A15证明核心依赖、compaction、严格解码与shutdown fail-closed；
6. 所有FAIL都有已修复并重新验证的issue；
7. 自动化门禁仍全绿，独立constitution review没有red flag或violation；
8. 受影响crate `AGENTS.md` 已由用户确认，OpenSpec evidence与checkbox逐项对应。
