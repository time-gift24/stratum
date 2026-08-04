# stratum-api 约定

- 只有在 Agent/Session/Turn 的必要状态、runtime snapshot 和输入已持久化后，创建或消息接口才能返回已接受；失败不得留下可被接受为成功的半成品。
- hosted-agent registry 的锁只保护内存映射访问。文件系统、Store、NATS、provider 和 agent 的异步工作必须在锁外完成。
- Store 是 agent 状态、消息历史和启动恢复的持久化真相源；NATS/JetStream 只负责事件分发与重放，不能代替 Store。
- 执行事实存储后端由 `[storage]` 配置段显式选择（`backend = "postgres" | "filesystem"`），组合根 `host.rs` 的 `StoreBackend` 只构造配置选定的那一个，无静默回退。配置段缺失（`require_storage()`）、backend 拼错、`postgres.url` 缺失或为空、Postgres 无法连接或迁移失败都直接启动失败（fail closed），拒绝"连不上就悄悄降级 filesystem"的耐久性隐形降级。
- postgres 是生产唯一支持路径（docker-compose 与 config.example 默认 postgres），启动即跑完 schema migrations；filesystem 后端只服务单测、嵌入式与无容器本地体验，同时是双后端 replay 对齐测试的行为参照。
- SSE 使用传输序号 cursor：响应写入 `id`，恢复时 `Last-Event-ID` 优先于 `after_cursor`，过期 cursor 必须显式报错。
- Session 是长期、图无关的核心身份；API 可以创建 Session 或将新 Agent 加入既有 Session，但一期每个 Session 同时只允许一个活跃操作。
- 启动时必须从持久化 definition 和 Store 完整重建 registry；恢复失败不得返回部分 registry。
- 新 Turn 必须同时通过 Session 单活检查、Agent persisted-running 检查和 Store start-turn CAS；持久化 `running` 只能显式 resume，任何新 Turn 都不得覆盖原 Session/Turn。
- Resume 复用既有 Session 占用，不得被单活集合误判为第二个操作；恢复 terminalize 后同步释放占用。
- failed/cancelled turn 中已经持久化的完整 user、assistant、tool 消息属于后续上下文；同进程后续请求必须从 Store 刷新到与重启恢复相同的 history，流式 partial delta 不进入 history。
- `HostState` 持有共享 shutdown token。shutdown 关闭 admission 后结束 SSE，在独立固定时限内 drain 已准入请求，再 stop 所有 active Agent 并有界等待终态持久化；超时保留 durable `running`，由下次启动显式 resume。
- create、message 和 resume 在任何持久化或 provider I/O 前必须取得 atomic admission RAII，并在 pending Store/EventStreamBus 工作中观察 shutdown token。admission drain 超时后的 late acceptance 必须自我 stop；create 还必须在 registry write lock 内重查 closed，禁止 snapshot 后注册。关闭后的新 durable work 返回安全稳定的 503，且不得触碰 Store/history。
- create 的持久化 mutation 必须拆成有界阶段，并在每个 `await` 前进入“可能已写入”状态；shutdown、timeout 或检查失败都必须 fail-safe 保留 definition/Store/history，只有 mutation 已确定结束且能确定零消息时才能 cleanup。Store commit 后的 NATS best-effort forward 必须内部有界，不能决定 durable acceptance。
- persisted `running` 只有在 Session ID 和 Turn ID 都存在、且 current Turn 在固定 Store barrier 内完全没有 durable message 时，`/resume` 才可把 Started-only 状态 terminalize 为 `failed`，保留 Session/Turn/usage/history/frontier；任一身份缺失或 current Turn 存在消息都必须走正常 resume 校验。
- SSE 直接 Session 路由与 Agent 解析路由都必须订阅同一个 Session stream；cursor 只控制传输重放。
- HTTP 最终错误边界只记录一次安全的结构化 operational error；span 可记录 Agent/Session/Turn/cursor 等 ID，不得记录 message、prompt、tool args、secret 或 host path。
- Recovery derives an agent's provider configuration solely from its persisted `ModelConfig`; the API
  exposes schemas only for configured models and never a second default-parameter representation.
- `POST /v1/agents` accepts an optional `model_config`. When present, creation preflights,
  persists, and composes with that configuration; when absent, it uses the resolved template default.
- `POST /v1/agents/{agent_id}/messages` also accepts an optional `model_config`. A valid override is
  committed only with an accepted new Turn and becomes that Agent's persisted default; omission
  reuses the persisted value, and any rejected start leaves it unchanged.
- API 文档以 utoipa 生成的 OpenAPI 为唯一权威：每个 handler 必须有 `#[utoipa::path]`，DTO 与 wire 类型必须有 `ToSchema`；错误响应只声明该 handler 经 `error_response()` 实际可达的状态码；SSE 端点以 `text/event-stream` + `StreamEnvelope` body 描述，帧语义（id=cursor、event=内层事件名、data=envelope JSON）写在 path description。`docs/PROTOCOL.md` 已废弃。
