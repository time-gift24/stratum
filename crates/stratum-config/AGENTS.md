# stratum-config 约定

- 配置使用严格模式；未知字段必须拒绝，不能静默忽略拼写错误。已移除的 `[agent]`、`[llm]` 与 Provider 资源字段属于未知字段，必须 fail closed，不能兼容性忽略。
- `[api]` 存在但省略 `bind` 时固定默认 `127.0.0.1:8080`；`allowed_origins` 在配置解析边界校验为合法 `http::HeaderValue` 并拒绝 `*`，路由器不得静默过滤。关闭排空、SSE 保活与调度器空闲超时均为正秒数配置，零值必须从严失败；审批回退重读周期是内部固定上限，不得配置化。
- 执行存储配置收敛为 `[postgres].url`：没有后端选择器或静默回退。`postgres.url` 缺失（`require_postgres`）或不是指向具体 database 的 PostgreSQL URL 即从严失败；`Debug` 中 URL 永久脱敏。
- Execution URL query 只允许不改变 database identity 的显式 TLS、statement cache、application name 与 `options` 键；`host`/`hostaddr`/`port`/`dbname`、credential override 和未知键一律拒绝，避免绕过三库隔离。三个 PostgreSQL URL 都必须显式写 port，禁止让 SQLx 从 `PGPORT` 注入隐式 identity。database identity 必须直接使用与 SQLx 相同的 `url::Url` 解析语义，再从规范化 authority/port/path 比较；path 还需与 SQLx 一致地移除全部前导 `/` 后再 percent-decode，禁止另写保留 dot-segment 的 URL parser。
- NATS 连接超时与 AgentRuntime 短尾流的保留时长/字节数/消息数上限继续放在 `[nats]` 能力配置中，解析边界完成字段校验（非空字符串、正数超时/上限、`replicas` 为 `1..=5`）；不编码固定历史保留保证。配置类型到基础设施运行时类型的映射由 `stratum-api` 装配时完成，本 crate 不依赖 `stratum-infra`。
- Provider、Model、credential 与 Agent definition 的运行时真相只来自 Studio PostgreSQL；本 crate 不承载 Provider API key、model allow-list、endpoint、timeout、环境变量覆盖或 template resolver。
- `[tools].workspace_root` 是 `shell` 与 `apply_patch` 共享的默认工作目录；本 crate 拒绝空值，`stratum-api` 启动时验证它是已有目录。它不承载 Agent definition、Provider 或执行持久化状态。
- `[api].readiness_timeout_ms` 是正整数；执行、Ontology 与 Studio readiness 共用这一完整探针时限，不在 handler 中硬编码环境超时。
- `[ontology].database_url` 在解析边界验证为无 query 参数的 `postgres`/`postgresql` URL，`Debug` 永久脱敏。Execution、Ontology 与 Studio 可以使用同一服务器，但同一 normalized host/effective port 上任意两个 section 都必须使用不同的 percent-decoded database path；不同 authority 上的同名 database 是独立 identity。比较忽略 credential/query，冲突错误不得包含 URL、database 名或 credential。
- `[studio].database_url` 是 section 内必填的 `postgres`/`postgresql` URL，使用与 Ontology 相同的严格无 query 校验并永久脱敏。API host 必须调用 `require_studio`，无论 `management_enabled` 为何都连接该 database；该 flag 只控制 management routes，并仅可在 loopback API bind 上启用。
