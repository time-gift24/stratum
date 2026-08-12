## ADDED Requirements

### Requirement: TranscriptCompacted 与 Companion Summary 原子提交
当kernel append `TranscriptCompacted`时，Postgres execution storage必须（SHALL）在单一transaction中按 AgentRuntimeId 锁定对应`agent_states` row、分配AgentRuntime-wide event_seq、插入`durable_events` discriminator、插入同一`(agent_runtime_id,event_seq)`的`transcript_compactions` companion并更新state high-water。companion的`(agent_runtime_id,event_seq)`必须（SHALL）以外键引用同一runtime的durable discriminator。不得（SHALL NOT）写`agent_messages` marker或完整transcript projection。`DurableEventSink::append`只能（SHALL）在整个transaction commit后确认。

companion必须（SHALL）永久保存`turn_id`、`compacted_iteration`、`upto`、non-null `retained_from_event_seq`、kernel提交的单一typed system-marker `summary`与`created_at`。该event的持久化durable payload必须（SHALL）为空对象，不得（SHALL NOT）复制任何companion字段；store读取typed `TranscriptCompacted`时必须（SHALL）将discriminator与companion组合成kernel event。

#### Scenario: Compaction 原子成功
- **WHEN** TranscriptCompacted append transaction提交
- **THEN**discriminator、companion summary与state high-water同时可见并共享event_seq

#### Scenario: Companion Insert 失败
- **WHEN**durable discriminator已在transaction中插入但companion insert或constraint失败
- **THEN**整个transaction回滚，kernel不收到durability acknowledgement，high-water不前进

#### Scenario: Summary 没有第二份副本
- **WHEN**检查同一compaction的durable row与companion
- **THEN**summary只存在于`transcript_compactions.summary`，durable row仅承担有序discriminator身份

#### Scenario: 不创建 Message Marker Projection
- **WHEN**compaction提交
- **THEN**只有durable row、companion与state变化；公开history后续直接从ledger join映射marker

### Requirement: Companion 只保存 Summary 与 Retained Frontier
`transcript_compactions`不得（SHALL NOT）保存完整messages、保留suffix副本、`window_start_line`或`summary_digest`。`summary`必须（SHALL）直接保存kernel提交的压缩system marker。`stratum-api`外层runtime编排在重放committed context时必须（SHALL）维护每条message的来源AgentRuntime-wide event_seq，并以kernel `upto`坐标确定压缩后第一条保留`MessageAppended`的`retained_from_event_seq`。

kernel现有compaction cut invariant必须（SHALL）保证至少存在一条保留suffix message，因此`retained_from_event_seq`必须（SHALL）non-null并指向同AgentRuntime中早于compaction event的真实`MessageAppended`。该pointer只是读取加速起点，不是compaction event自身sequence，也不得（SHALL NOT）由物理行号、NATS cursor或message_seq表示。无法在append时从provenance解析合法pointer必须（SHALL）使整个transaction fail closed。

#### Scenario: 部分前缀压缩
- **WHEN**committed context的`[0,upto)`被summary替换，第一条保留message来自event_seq 5而TranscriptCompacted分配event_seq 9
- **THEN**companion保存`event_seq=9`和`retained_from_event_seq=5`，不复制sequence 5之后的messages

#### Scenario: Summary 无 Digest
- **WHEN**recovery读取companion
- **THEN**它使用单一typed summary，不读取或计算`summary_digest`

#### Scenario: 不使用 Filesystem 坐标
- **WHEN**companion写入或读取
- **THEN**定位只使用AgentRuntimeId和event_seq，不出现AgentId/string version tag、file path、line number、byte offset或SSE cursor

#### Scenario: 无法解析 Retained Pointer 时 Append 回滚
- **WHEN**合法Compaction decision不能对应到一条真实保留MessageAppended来源
- **THEN**storage拒绝companion写入并整体回滚，不保存nullable或猜测pointer

### Requirement: Recovery 从 Latest Companion 构造 Historical Base
对current Turn的`LoopStarted.event_seq - 1`所得fixed base，`stratum-api`外层runtime编排必须（SHALL）在 exact AgentRuntimeId 内查找event_seq不大于base的最新`transcript_compactions` companion。候选只有在AgentRuntime/Turn identity、event sequence、对应durable row的`event_version`及其summary shape、compacted iteration、upto和retained pointer均可与durable ledger相互校验时才能（SHALL）使用快速路径；系统不得（SHALL NOT）为summary再发明独立version字段。结构完整但高于当前支持范围的`TranscriptCompacted.event_version`必须（SHALL）返回`runtime_incompatible`；只有当前支持版本的companion缺失、summary shape/内容无法严格解码或identity不一致才必须（SHALL）返回`durable_state_corrupt`。

有效时，historical base必须（SHALL）从companion summary开始，并读取`retained_from_event_seq..=base`范围内的后续`MessageAppended`与历史terminal control boundaries，构造与full replay等价的committed context。compaction event_seq只是summary locator；message suffix必须（SHALL）从retained pointer开始，即使它小于compaction sequence。current Turn在base之后的TranscriptCompacted不得（SHALL NOT）提前并入historical base，而必须（SHALL）在`(base,through]` exact-Turn replay中按序应用。

#### Scenario: Latest Companion 快速恢复
- **WHEN**base以内存在多次compaction且最新companion与pointer有效
- **THEN**recovery使用最新summary与retained suffix，得到与event_seq 1起full replay相同的context

#### Scenario: Locator 与 Read Start 不同
- **WHEN**latest companion event_seq为9且retained pointer为5
- **THEN**recovery定位summary@9后从message@5读取保留suffix，不错误地只从9之后读消息

#### Scenario: Current Turn 后续 Compaction
- **WHEN**current Turn在base之后、through barrier之前提交TranscriptCompacted
- **THEN**historical base不吸收它，kernel replay在current-Turn sequence中应用一次

#### Scenario: Barrier 之后的 Companion 不可见
- **WHEN**compaction event_seq大于fixed base或current through
- **THEN**本次recovery不把它用于相应historical base或replay

#### Scenario: 未知 Compaction Event Version
- **WHEN** `TranscriptCompacted` row结构完整但`event_version`高于当前支持范围
- **THEN** recovery返回`runtime_incompatible`，不把未知版本误报为summary损坏或尝试full replay绕过

### Requirement: 加速 Pointer 失效时只做 In-memory Full Replay
当base前没有compaction时，`stratum-api`外层runtime编排必须（SHALL）从durable ledger起点重放。当companion identity与单一summary完整有效，但其locator选择、`retained_from_event_seq`或provenance校验不足以使用快速路径时，外层编排必须（SHALL）忽略加速信息，并在相同fixed base内从event_seq 1按序物化`MessageAppended`、每个可join的`TranscriptCompacted`与历史terminal boundaries。系统不得（SHALL NOT）在线修复或重写companion，不得（SHALL NOT）提供projection rebuild API/CLI/启动扫描，也不得（SHALL NOT）回退filesystem。

对于当前支持的`TranscriptCompacted.event_version`，若ledger存在discriminator却缺少其必需companion、单一summary无法严格解码，或companion与discriminator的AgentRuntime/Turn/event identity冲突，则durable truth不完整，必须（SHALL）返回`durable_state_corrupt`。未知但结构完整的更高event version仍必须（SHALL）返回`runtime_incompatible`。两种情况都不得（SHALL NOT）被描述成可忽略的checkpoint miss或通过full replay绕过。

#### Scenario: 从未发生 Compaction
- **WHEN**base前不存在TranscriptCompacted discriminator或companion
- **THEN**recovery从ledger起点full replay并正常继续

#### Scenario: Retained Pointer 无效
- **WHEN**companion summary有效但pointer不指向预期AgentRuntime的MessageAppended或与upto provenance不一致
- **THEN**recovery丢弃该加速pointer并内存full replay，不写数据库

#### Scenario: Required Companion 缺失
- **WHEN**durable discriminator存在但companion或summary缺失
- **THEN**full replay fail closed为`durable_state_corrupt`，不回退旧filesystem index

#### Scenario: 重启不主动 Rebuild
- **WHEN**API启动或readiness检查运行
- **THEN**系统不扫描、修复或重建compaction历史；具体recovery只按需选择快速路径或内存replay

### Requirement: Compaction 作为 Typed History Marker 公开且不删除原文
每个durable TranscriptCompacted必须（SHALL）通过API-owned `AgentRuntimeProductEventV1`映射为可折叠的“上下文已压缩”marker，至少公开完整`summary`与`compacted_iteration`，不得（SHALL NOT）伪装成user/assistant/system chat message，也不得（SHALL NOT）公开kernel内部`upto`或retained pointer。原始`MessageAppended`与所有companion summary必须（SHALL）永久保留；compaction不得（SHALL NOT）删除、重写或阻止向上分页访问原始消息。

#### Scenario: History 显示 Compaction Marker
- **WHEN**history page覆盖TranscriptCompacted event_seq
- **THEN**客户端得到typed marker并可按需展开完整summary，而不是伪造system message或全局banner

#### Scenario: 原始消息仍可向上分页
- **WHEN**用户在compaction marker之后向上滚动加载更旧history
- **THEN**API仍从durable ledger返回被压缩的原始MessageAppended

#### Scenario: Realtime 收到 Marker
- **WHEN**TranscriptCompacted commit后NATS product frame成功发布
- **THEN**frame使用该AgentRuntime的durable event_seq并携带exact AgentRuntimeId与固定AgentId，前端插入marker但不删除已有消息

#### Scenario: NATS 丢失不丢 Summary
- **WHEN**compaction已在PG commit但NATS publish失败
- **THEN**后续AgentRuntimeView/history cold read仍通过exact AgentRuntime companion返回同一marker与summary
