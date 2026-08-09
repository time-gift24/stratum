# stratum-infra 约定

## 职责范围

- `stratum-infra` 包含外部基础设施适配器。接口定义保留在各能力的 `definition.rs` 文件中，
  错误保留在 `error.rs` 中，具体后端和适配器保留在按名称划分的模块中。
- 保留的职责面很窄：内核接收端契约（`DurableEventSink`、`TelemetryEventSink`）以及具体的
  AgentRuntime 范围 NATS 短尾传输。旧的 `FilesystemAgentStore`、文件系统持久化接收端、
  `compact.jsonl` 检查点、`StoreEventStreamBus` 装饰器以及通用的内存/NATS `EventStreamBus`
  均已删除，不得重新引入。

## 持久化与遥测接收端

- `DurableEventSink::append` 是正确性边界：只有已配置的消费方确认事件后才能返回，并且必须
  保留原始错误链。不得把持久化失败降级为只记录日志。
- `TelemetryEventSink::emit` 是同步的尽力而为操作，不得向循环返回失败，也不得因传输 I/O 阻塞循环。
- 本 crate 只负责两个内核契约 trait 和 `DurableEventSinkError`；具体的持久化实现在
  `stratum-postgres` 中，将已提交行转换为短尾帧的每 AgentRuntime 实时调度器位于
  `stratum-api` 中。旧的限定范围文件系统/总线接收端流水线已删除，不得重新引入。

## AgentRuntime 范围的 NATS 短尾流

- NATS 传输是职责窄而具体的 `agent_runtime_tail::NatsAgentRuntimeTail`：它在一条 JetStream
  流上发布和订阅按 AgentRuntime 划分的短期保留帧流，并使用可配置的有限时长、字节数和消息数
  上限以及丢弃旧数据的保留策略。它不是持久化历史，也从不保证跨重启重放；Postgres 持久化
  账本才是恢复时的事实来源。
- 短尾流载荷是不透明的 `Bytes`；`AgentRuntimeStreamFrameV1` 的序列化，以及每个运行时内产品帧
  与遥测帧的顺序，都由 `stratum-api` 负责。短尾流只保证每个主题内的 JetStream 顺序。
- NATS 主题命名集中在 `agent_runtime_tail/subject.rs` 中；业务代码绝不能直接使用
  `async-nats`。
- 无游标订阅只传递从订阅时刻起产生的新帧（`DeliverPolicy::New`）；只要游标位置仍被保留，就从
  该位置之后恢复；如果该位置已被丢弃，则必须在传递任何内容前返回类型化错误
  `AgentRuntimeTailError::CursorExpired`——绝不能静默回退到完整重放。超前于当前短尾流、来自另一
  AgentRuntime、来自旧的流重建代次，或应用于空流的游标，都必须在发送响应头前判定过期，并强制
  执行冷启动引导。
- `AgentRuntimeTailCursor` 是不透明且带版本的传输位置，绑定 `AgentRuntimeId`、JetStream 流创建
  代次与流序号（编码为字符串以用作 SSE `id`）；绝不能将其
  与 `event_seq`/遥测序号比较，也不得将其持久化为业务状态。
- `NatsAgentRuntimeTail::is_available` 反映运行时操作状况，而不只是启动时的构造结果：发布、订阅
  或传递失败会降低就绪状态，后续成功的消息代理操作会恢复就绪状态。游标过期校验是一次成功的
  消息代理查询，不会将 NATS 标记为不可用。

## 安全与可观测性

- 基础设施错误不得包含载荷、提示词、推理内容、工具参数/结果、凭据、NATS 身份验证材料或
  机密信息。结构化日志只能使用 ID、事件类型、游标、超时和后端错误。
- 发布和订阅操作必须保持取消安全。不得跨 `.await` 持有互斥锁/`RwLock` 的锁守卫；凡是传输延迟可能
  阻塞运行时推进的地方，都必须采用有界行为。
- 真实 NATS 测试继续作为 `tests/` 下默认忽略的集成测试，并通过本 crate 的
  `Makefile`/`docker-compose.test.yml` 运行。单元测试不得依赖实时消息代理。
