# postgres-execution-storage Specification（add-postgres-execution-storage）

## ADDED Requirements

### Requirement: Postgres 耐久事件后端
系统必须（SHALL）提供 Postgres `DurableEventSink` 实现：每个事件作为一行写入 `durable_events` 表，携带物化寻址列（`session_id`、`agent_id`、`turn_id`、per-run 单调 `seq`、`event_type`、`created_at`）与完整 canonical JSON `payload`（jsonb）。`payload` 的序列化字节必须与 filesystem 后端的 JSONL 行一致。事件确认语义与 filesystem 后端等价：`append` 返回前事务已提交。系统必须（SHALL）提供按 `turn_id` 有序读取完整事件序列的读取器，供 resume 重放使用。

#### Scenario: 事件落库即可重放
- **WHEN** AgentLoop 通过 Postgres sink 提交耐久事件
- **THEN** 事件以物化寻址列 + canonical JSON payload 落表，per-run `seq` 单调递增，读取器按 `turn_id` 取回的事件序列与提交顺序一致

#### Scenario: payload 与 JSONL 字节一致
- **WHEN** 同一 `DurableAgentEvent` 分别写入 filesystem 与 Postgres 后端
- **THEN** Postgres `payload` 列的 JSON 与 filesystem JSONL 行反序列化后逐字段相等

#### Scenario: 唯一性约束兜底乱序
- **WHEN** 同一 `turn_id` 出现重复 `seq` 的写入（实现缺陷或重试错序）
- **THEN** 数据库唯一约束拒绝写入，sink 返回类型化错误，不产生乱序事件流

### Requirement: 消息提交是单事务原子写入
`PostgresAgentStore::append_message` 必须（SHALL）在单个事务内完成：原子递增该 agent 的 `next_message_seq` 计数器并取回序号、向 `agent_messages` 写入完整 `StreamEnvelope` 行。两者必须（SHALL）同生共死：任一失败整体回滚，不得（SHALL NOT）产生序号已推进但消息缺失、或消息已落行但序号未推进的部分提交。journal 与消息历史的统一投影（kernel run 的 `MessageAppended` 落 `agent_messages`）不在本期范围，待新 kernel 组合进 API 时由投影器实现。

#### Scenario: 一次提交两处可见
- **WHEN** append_message 成功返回
- **THEN** 序号推进与消息行对后续读者同时可见，返回的 envelope 携带分配的 message_seq

#### Scenario: 并发提交序号无竞态
- **WHEN** 同一 agent 的两个 append_message 并发执行
- **THEN** 两者分配到不同的连续 message_seq，agent_messages 主键不冲突，无丢失更新

### Requirement: 状态前置条件以条件更新实现
`start_turn` 与 `complete_iteration` 的前置校验必须（SHALL）以条件 UPDATE 表达：仅当当前行满足期望状态（status、active_turn_id、迭代前沿）时更新生效；影响行数为零必须（SHALL）映射为与 filesystem 后端相同的类型化前置条件错误，不得（SHALL NOT）吞掉或改写为通用存储错误。

#### Scenario: 前置满足则提交
- **WHEN** start_turn 时 agent 处于可开始状态
- **THEN** 状态行原子更新并钉死 runtime_snapshot，返回新状态

#### Scenario: 前置不满足 fail closed
- **WHEN** complete_iteration 的期望迭代与持久化前沿不一致
- **THEN** 更新影响零行，返回与 filesystem 后端一致的前置条件错误，状态行不变

### Requirement: 消息历史读路径是主表范围读
`history_page` 必须（SHALL）从 `agent_messages` 表按 `(agent_id, message_seq)` 主键范围读取，不得（SHALL NOT）在读路径过滤或展开 journal 事件流。`agent_messages` 必须（SHALL）是 append-only：压缩（`TranscriptCompacted`）不得（SHALL NOT）改写、删除或隐藏消息行，用户可见历史永远完整。

#### Scenario: 分页读取是主键范围扫描
- **WHEN** 客户端按 after_seq/through_seq/limit 请求历史页
- **THEN** 结果为 agent_messages 主键范围扫描，页边界与 has_more 语义与 filesystem 后端一致

#### Scenario: 压缩不改写用户可见历史
- **WHEN** 某 turn 提交了 TranscriptCompacted
- **THEN** agent_messages 中被压缩的原始消息行保持完整可读，history_page 结果不变

### Requirement: 后端显式选择且不允许静默回退
`stratum-api` 组合根必须（SHALL）通过配置显式选择存储后端（`postgres` 或 `filesystem`）：配置缺失、取值非法或 Postgres 无法连接时必须（SHALL）启动失败并给出类型化错误，不得（SHALL NOT）静默回退到另一后端。生产部署配置（docker-compose、示例配置）必须（SHALL）默认使用 postgres。

#### Scenario: 无法连接即启动失败
- **WHEN** backend 为 postgres 但数据库不可达
- **THEN** API 进程启动失败，错误信息指向存储配置，不降级为 filesystem

#### Scenario: filesystem 后端可用于本地与测试
- **WHEN** backend 为 filesystem
- **THEN** API 以 filesystem 后端正常启动，行为与现状一致，不依赖容器

### Requirement: 双后端重放行为一致
同一事件序列经 filesystem 与 Postgres 后端分别持久化后，resume 重放的重建结果必须（SHALL）逐事件一致：committed context、迭代前沿、Hook journal 查表结果与终态判断完全相同。

#### Scenario: 双后端 replay 对齐
- **WHEN** 一批覆盖五个 Hook 点、压缩与终态的事件序列分别写入两种后端并各自 resume
- **THEN** 两次重建的 committed context 与迭代前沿逐项相等，Hook decision 复用/重试/重现判断一致

### Requirement: 不提供存量数据迁移路径
系统不得（SHALL NOT）提供 filesystem 到 Postgres 的存量数据迁移工具：Postgres 后端上线即空库起跑，既有 filesystem 数据原地保留且 filesystem 后端仍可读取。库内格式演进必须（SHALL）由 `state_version` 列与事件载荷的 serde 版本纪律承担。

#### Scenario: 空库起跑
- **WHEN** 对已有 filesystem 数据的部署切换到 postgres 后端
- **THEN** API 以空库正常启动，新运行全部落 PG，无迁移步骤、无数据损坏
