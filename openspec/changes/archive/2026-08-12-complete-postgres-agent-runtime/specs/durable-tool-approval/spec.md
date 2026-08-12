## ADDED Requirements

### Requirement: 审批身份与生命周期以 AgentRuntime durable ledger 为唯一真相
当 `decide_tool_call` 的审批 Handler 判定一个已完成最终复验的 Tool call 需要人工决定时，系统必须（SHALL）由服务端生成 UUIDv7 `ApprovalId`，并把它绑定到 exact AgentRuntimeId、Turn 与 `HookInvocationId`。AgentRuntimeId只由外层 ledger/sink scope 提供，不得（SHALL NOT）加入kernel journal variant；现有Hook journal保持只包含invocation identity、point、iteration、CallId、digest与decision/failure，storage与恢复编排必须（SHALL）通过外层row验证exact AgentRuntime/Session/Turn归属。系统必须（SHALL）在对应 `HookInvocationPending` 已提交后追加 `ToolApprovalRequested`；Requested payload 必须（SHALL）保存 `hook_invocation_id`、最终 `CallId`、Tool name、Echo 的schema-validated user-authored opaque arguments及typed `ToolKind`/`DangerLevel`，使恢复时能够证明用户决定针对的就是Handler所见逻辑调用。当前closed composition不得（SHALL NOT）暴露credential/reference/provider字段或注册credential-aware Tool；未来此类Tool必须先通过独立PATCH定义opaque reference、批准消费后的安全注入与fail-closed result transform。

审批 requester 本身是 durable writer：它必须（SHALL）在开启Requested transaction前持有exact runtime的live dispatcher handle；hosted Handler可以复用bound sink持有的handle，否则只能调用无caller-frontier参数的hub ensure取得handle。missing generation的frontier必须（SHALL）由hub在per-runtime ensure/retirement gate内读取当前committed PG high-water并安装，不得由requester预读或传入。requester随后必须（SHALL）持有同一handle完成transaction并跨过commit，只有commit后才能通过该handle提交receipt。它不得（SHALL NOT）假设此前的Pending仍使某个generation存活，或让receipt在commit后隐式创建generation。

`durable_events` 必须（SHALL）是审批的唯一耐久真相；Requested、Resolved与matching journal事实必须（SHALL）以同一`(agent_runtime_id,event_seq)` ledger identity持久化，审批唯一性与查询范围必须（SHALL）以AgentRuntimeId为fence，不得使用pinned AgentId或template string version tag替代runtime scope。系统不得（SHALL NOT）建立 `tool_approvals`、审批派生状态表或审批 claim。审批状态必须（SHALL）从同一 AgentRuntime ledger 派生：存在 Requested 表示 Requested，存在同 `ApprovalId` 的 `ToolApprovalResolved` 表示 Resolved，存在 matching `HookInvocationCompleted` 表示 Consumed；所属 Turn 已提交 terminal event 且此前没有 matching Completed 时表示 Invalidated。系统不得（SHALL NOT）以 TTL 自动使审批过期。公开Requested/Resolved realtime frame必须（SHALL）由per-AgentRuntime dispatcher发布，并同时携带exact AgentRuntimeId与runtime固定AgentId。

#### Scenario: 首次请求人工审批
- **WHEN** 已提交 Pending 的 decide invocation 首次需要人工决定
- **THEN** requester在开启Requested事务前复用bound handle或通过hub ensure取得live handle，再生成UUIDv7 ApprovalId并追加唯一ToolApprovalRequested；commit后只通过同一handle提交receipt，且Requested引用exact HookInvocationId

#### Scenario: 请求保存当前 Echo 的最终 Tool 调用
- **WHEN** transform_tool_call 修改 arguments 或授权元数据且最终复验通过后进入审批
- **THEN** ToolApprovalRequested保存审批Handler所见的最终CallId、Tool name、user-authored opaque arguments与typed ToolKind/DangerLevel，而不是provider原始输入或不存在的credential channel

#### Scenario: Credential-aware Tool 未完成安全 PATCH 前不可注册
- **WHEN**composition试图注册需要credential reference、provider注入或结果脱敏的Tool
- **THEN**该Tool不得进入当前runtime registry；本change不把Echo的opaque JSON误报为typed credential安全边界

#### Scenario: 恢复同一逻辑请求
- **WHEN** 相同 AgentRuntime、Turn 与 HookInvocationId 的 Pending invocation 在恢复后再次进入审批 Handler
- **THEN** Handler 从 ledger 复用既有 ApprovalId 与 Requested，不生成新身份、不追加第二个 Requested，也不再次创建独立审批提示

#### Scenario: 同一 Turn 有多个审批
- **WHEN** 一个 Turn 先后产生多个需要审批的 Hook invocation
- **THEN** 每个 invocation 拥有各自稳定的 ApprovalId，查询不得以 AgentRuntime 或 Turn 级单例覆盖其他审批

#### Scenario: Resolved 尚未 Consumed
- **WHEN** ToolApprovalResolved 已提交但 matching HookInvocationCompleted 尚未提交且 Turn 仍 running
- **THEN** 派生状态是 Resolved，系统不得声称决定已经被 AgentLoop 应用或 Tool 已执行

#### Scenario: Completed 派生 Consumed
- **WHEN** ledger 中出现引用 Requested 所绑定 HookInvocationId 的 HookInvocationCompleted
- **THEN** 审批派生状态是 Consumed，无需写入第二份审批状态

#### Scenario: Terminal 派生 Invalidated
- **WHEN** 所属 Turn 在 matching HookInvocationCompleted 之前提交 LoopFinished、LoopFailed 或 LoopCancelled
- **THEN** 审批派生状态是 Invalidated，既有 Requested 和 Resolved 历史永久保留但不得再被应用

### Requirement: Resolve 在 AgentRuntime 写入序列中线性化且保持精确幂等
审批 resolve 命令必须（SHALL）只接受路径 `POST /v1/agent-runtimes/{agent_runtime_id}/approvals/{approval_id}` 中的 `agent_runtime_id`、`approval_id` 与请求体中的 exact `turn_id`、`decision`。resolver必须（SHALL）在开启写事务前通过无caller-frontier参数的hub ensure取得live dispatcher handle并持有跨过transaction与commit；missing generation由hub gate读取当前committed PG high-water并安装。命令随后必须（SHALL）在事务中对对应 `agent_states` row执行 `FOR UPDATE`，用该行锁与同 AgentRuntime 的其他 durable writer 串行化并分配连续 `event_seq`；随后必须（SHALL）验证请求 Turn 是 `current_turn_id`，并从该runtime ledger查询 Requested、Resolved、matching Completed 与 terminal 事实。系统不得（SHALL NOT）为 resolve 增加审批行锁、claim 或另一套锁序。

若 exact Turn 已 terminal，命令必须（SHALL）返回 `409 approval_invalidated`。若已有 Resolved，完全相同的决定必须（SHALL）返回 `204 No Content` 且不追加事件；相反决定必须（SHALL）返回 `409 approval_already_resolved`。只有存在未决定 Requested 且 exact Turn 仍 running 时，命令才必须（SHALL）追加唯一 `ToolApprovalResolved`。事务必须（SHALL）先提交，再只通过前述同一 dispatcher handle提交commit receipt，并 best-effort 通知进程内waiter；提交失败不得（SHALL NOT）发送receipt或通知。NATS实时通道只消费该runtime dispatcher发布的frame，不是审批真相。

#### Scenario: 首次 Resolve
- **WHEN** 调用方对 exact running Turn 中尚未决定的 Requested 提交批准或拒绝
- **THEN** resolver先通过hub ensure取得live handle，再在锁定exact AgentRuntime `agent_states` row的事务中追加唯一ToolApprovalResolved；提交后通过该handle发送receipt、返回204，再best-effort通知waiter

#### Scenario: 相同决定重试
- **WHEN** 调用方对已有 Resolved 的 ApprovalId 提交完全相同的决定且 Turn 尚未 terminal
- **THEN** 系统返回 204，保留原决定与原 event_seq，不追加事件且不产生第二次决定

#### Scenario: 相反决定冲突
- **WHEN** 调用方对已有 Resolved 的 ApprovalId 提交相反决定且 Turn 尚未 terminal
- **THEN** 系统返回 409 approval_already_resolved，原决定保持不变且不追加事件

#### Scenario: Terminal 优先判为 Invalidated
- **WHEN** exact Turn 已提交 terminal event，无论审批此前是 Requested 还是 Resolved
- **THEN** resolve 返回 409 approval_invalidated，不追加新的 ToolApprovalResolved

#### Scenario: Resolve 身份 Fence
- **WHEN** AgentRuntime 不存在、Approval 不存在或不属于路径 AgentRuntime、请求 turn_id 不等于审批所属 Turn、或不等于 current_turn_id
- **THEN** 系统分别返回 `404 agent_runtime_not_found`、`404 approval_not_found` 或 `409 stale_turn`，不追加事件也不通知 waiter

#### Scenario: Resolve 与 Terminal 并发
- **WHEN** resolve 与 Turn terminal append 同时竞争同一 AgentRuntime
- **THEN** agent_states 行锁使 ledger 只暴露一个线性化顺序，terminal 之后不会出现可应用的新决定

#### Scenario: Resolve 未托管 Turn
- **WHEN** exact Turn 在 Postgres 中仍 running 但当前进程没有托管它
- **THEN** 系统仍在原 AgentRuntime ledger提交有效决定并返回 204，但不创建 AgentLoop、不接管 Turn、不改变 current_turn_id，继续执行必须由调用方显式 resume

### Requirement: Hosted 审批等待不得丢失已提交决定
Hosted 审批 Handler 必须（SHALL）使用 register-then-read-and-poll 协议：先按 `(AgentRuntimeId,ApprovalId)` 注册进程内 waiter，再从该runtime的Postgres durable ledger读取状态。若尚未Resolved，Handler必须（SHALL）在同一cancellation-safe wait loop中选择Turn cancellation、进程内通知与有上限的退避tick；通知或tick到达后都必须（SHALL）重新读取ledger。退避间隔可以增长但必须（SHALL）有固定内部上限，不得（SHALL NOT）依赖公开配置、NATS活动或下一条命令才能再次读取。Resolver 必须（SHALL）先提交 Postgres 再 best-effort 通知；进程内通知和 NATS 只负责降低等待延迟，不得（SHALL NOT）成为决定真相或 Tool 执行依据。waiter在决定、取消、shutdown或typed fail-closed错误后必须（SHALL）注销。

#### Scenario: 决定早于 Waiter 注册
- **WHEN** ToolApprovalResolved 在 Handler 注册 waiter 之前已经提交
- **THEN** Handler 注册后的 ledger 读取立即观察到决定，不阻塞也不重新询问

#### Scenario: 决定与注册并发
- **WHEN** resolve 提交发生在 waiter 注册与随后 ledger 读取之间
- **THEN** 随后的读取观察到决定，Handler 不得永久丢失唤醒

#### Scenario: 通知发送失败
- **WHEN** ToolApprovalResolved 已提交但进程内通知或 NATS 发布失败
- **THEN** 决定仍然有效，当前 Handler最迟在bounded backoff tick后重读ledger并继续；若进程已退出，恢复后的Handler通过首次ledger读取继续

#### Scenario: 等待期间取消
- **WHEN** Handler 等待决定期间 Turn CancellationToken 被取消
- **THEN** Handler 停止等待且不返回 Execute 或 Block，由随后提交的 Turn terminal event 将尚未 Consumed 的审批派生为 Invalidated

### Requirement: 审批决定通过 Hook journal 被消费
审批 Handler 必须（SHALL）只把 durable Resolved 映射为普通 `decide_tool_call` decision：批准映射为 Execute，拒绝映射为带安全 reason 的 Block。AgentLoop 必须（SHALL）先按既有 Hook journal 合同提交 matching `HookInvocationCompleted`，再应用决定；该 Completed 是审批 Consumed 的唯一证明。未托管 Turn 中提交的决定不得（SHALL NOT）自动 resume；显式 resume 后，Pending invocation 必须（SHALL）复用 ledger 中的既有决定。

#### Scenario: 批准被消费
- **WHEN** hosted Handler 读取到 durable approve 决定
- **THEN** Handler 返回 Execute，AgentLoop 先提交 matching HookInvocationCompleted，再提交 ToolExecutionStarted 并执行 Tool

#### Scenario: 拒绝被消费并继续 Turn
- **WHEN** hosted Handler 读取到 durable reject 决定
- **THEN** Handler 返回 Block，AgentLoop 先提交 matching HookInvocationCompleted，再让阻断结果经过 after_tool_call 并以 role=tool 的 MessageAppended 持久化，Tool 不执行且 AgentLoop 继续后续模型迭代

#### Scenario: Resolved 后崩溃恢复
- **WHEN** 进程在 ToolApprovalResolved 已提交但 HookInvocationCompleted 尚未提交时停止
- **THEN** 显式 resume 后以原 HookInvocationId 重试 Pending，Handler 读取既有决定且不重新问人，AgentLoop 再提交 matching Completed

#### Scenario: Completed 后崩溃恢复
- **WHEN** matching HookInvocationCompleted 已提交但进程在应用 Execute 或 Block 前停止
- **THEN** resume 直接复用 journal 中 digest 匹配的 Hook decision，不调用审批 Handler，也不追加第二个 Resolved

#### Scenario: 未托管决定需要显式 Resume
- **WHEN** Turn 未托管期间 ToolApprovalResolved 已提交
- **THEN** AgentRuntime 保持 running/unhosted，只有显式 resume exact Turn 后才读取并消费决定

### Requirement: Pending approvals 从同一一致性屏障派生
`AgentRuntimeView.pending_approvals` 必须（SHALL）在与 `snapshot_event_seq`、AgentRuntime status、固定 `agent_id` 和 current Turn 相同的 Postgres MVCC snapshot 中，从 exact AgentRuntime current Turn 的 durable ledger 派生。它必须（SHALL）只返回存在 Requested、尚无 Resolved、尚无 matching Completed 且 Turn 未 terminal 的审批，并按该runtime Requested 的 `event_seq` 稳定排序；刷新、重连或 NATS 保留窗口过期不得（SHALL NOT）改变这一真相。

#### Scenario: 刷新重新发现未决定审批
- **WHEN** 浏览器没有接收到实时 Requested 或在等待期间刷新
- **THEN** AgentRuntimeView 从 exact AgentRuntime ledger 返回同一个 ApprovalId 与安全 Tool 详情，用户可以继续提交决定

#### Scenario: 已决定审批不再要求决定
- **WHEN** ToolApprovalResolved 已提交但 Turn 尚未 resume 或尚未提交 matching Completed
- **THEN** 该审批不出现在 pending_approvals 中，AgentRuntimeView 仍可通过 running/unhosted 与 resume_required 提示显式恢复

#### Scenario: 多个 Pending 独立返回
- **WHEN** current Turn 的固定屏障内存在多个未决定 Requested
- **THEN** AgentRuntimeView 按 requested event_seq 返回全部审批，客户端按 `(AgentRuntimeId,ApprovalId)` 独立处理

#### Scenario: NATS 缺失不影响 Pending
- **WHEN** Requested 的 NATS 增量丢失、重复或已经超出短保留窗口
- **THEN** Postgres AgentRuntimeView 仍返回正确 pending_approvals，实时通道不得覆盖 ledger 派生结果
