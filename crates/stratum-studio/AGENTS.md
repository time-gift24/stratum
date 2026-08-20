# stratum-studio 约定

- `stratum-studio` 是 Provider、Model、credential 与可变 Agent definition 的唯一 authoring/runtime store；它拥有独立 PostgreSQL database 与 migration history，不读 boot config、环境 API key、template 文件或 execution ledger。
- 新 database 由 migration 创建 singleton catalog metadata，但保持资源表为空；禁止隐式 seed、fallback 或自动导入。已有 catalog migration 不得覆盖 credential、revision、资源或时间戳。
- credential 只以 `SecretString` 穿过可信 Store→provider assembly 边界；读取 API、Debug、错误、tracing、OpenAPI example、execution event 与 NATS 都不得包含 secret 或可推断片段。Provider 缺少或持有空白 credential 属于 `CatalogCorrupt`，不得被查询 join 静默省略。
- 每个 public Store connect/read/write/runtime assembly 边界都使用 `tracing::instrument(skip_all, ...)` 建立 operation span，只显式记录 Provider kind、Model name 或 Agent name 等非敏感资源身份。不得记录 database URL、credential/`SecretString`、整个 Agent definition、prompt 或 model parameters，也不在 Store instrument 上启用错误事件；错误只由 startup/HTTP 处理边界记录一次。
- 所有 mutation 先在 transaction 中独占锁定 `studio_catalog` singleton，维护引用不变量并 bump catalog revision；create/update 的 `Versioned` representation 必须在同一 transaction 内读取并完成类型解码，随后才 commit，禁止 commit 后再查 Provider、Model 或 Agent definition。完整 runtime Provider 快照在同一 transaction 持有共享 catalog 锁读取，禁止跨 revision 拼接 Provider、credential 与 Models。
- Agent definition 必须显式引用已存在 Model；单独删除 Model 时，有 Agent definition 引用则返回结构化 blocker。删除 Provider 时，仅 Agent definition 引用它的 Model 才是 blocker；无引用时必须在同一 Studio transaction 内显式删除该 Provider 的 Models、credential 和 Provider。已创建 AgentRuntime 的 immutable definition 只在 execution ledger，Studio 更新或删除不得改写历史 runtime。
- 当前只有一个 PostgreSQL 实现，不新增 repository/service trait。Provider kind 是闭集 `openai | deepseek`；endpoint、timeout 与 adapter model 支持属于 `stratum-api` 可信策略，不进入本 schema。
