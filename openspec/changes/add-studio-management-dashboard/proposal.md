## Why

Stratum 已有 Studio 管理入口，但 Provider 运行时仍保留 boot `[llm]` seed、transport 配置和禁用 Studio 时的配置回退，Agent definition 也仍可从 `[agent].templates_root` 导入，形成多个可能漂移的真相源。现在需要把 Studio PostgreSQL 收敛为 Provider、Model、credential 与 Agent definition 的唯一 authoring/runtime 真相，并让每次新的 LLM work 从数据库组装可信快照，同时保留 Agent-first 仪表盘的信息架构。

## What Changes

- 新增独立 `/studio` 仪表盘：首期以真实 Agent 定义卡片为核心，不显示解释性 “Agents” 区块、伪造指标或空的监控占位；布局为后续 Agent 统计与监控面板保留可扩展区域。
- 在全局 product navigation 最右侧提供设置图标；Provider 与 Model 管理通过设置页进入，不作为仪表盘一级页签或独立“资源配置”导航。
- 新增 Agent 定义的列表、创建、读取、更新和删除能力；wire contract 显式包含 author-supplied `agent_version`，编辑以结构化表单为主、脱敏原始配置为辅，并通过只读 `GET /v1/tools` 使用 host 真实可注册的工具目录。
- 新增 Provider 的列表、创建、读取、credential 更新和删除，以及一次性连接测试；Model 是 Provider-scoped immutable identity，只提供列表、创建、读取和删除，所谓“更新”等价于受引用检查保护的删除后重建。Model 参数继续以服务端 schema 驱动，避免前端硬编码能力。
- **BREAKING**：移除 Provider/Model/credential 从 boot `[llm]` 配置 seed 或回退装配、以及 Agent definition 从 `[agent].templates_root` 导入/热读的行为；Studio database URL 成为 API host 的必需配置，空 catalog 由管理 API 显式建立。
- Provider adapter 的固定 endpoint 与 timeout 归属代码内受信任 adapter policy，不再从部署配置读取；所有可管理 Provider 状态只存于 Studio PostgreSQL。每次新的 LLM work / Turn 从数据库重新组装 Provider snapshot，只有正在执行的 Turn pin 住捕获的 Provider `Arc`，不维护需要热替换的进程级 catalog cache。
- 删除 Provider 只在其 Model 仍被 Agent definition 引用时返回 blocker；无引用时在一个 Studio transaction 中删除其 owned Models、credential 与 Provider，绝不 cascade 删除或改写 Agent definition。
- 为管理 API 定义分页、类型化 DTO、错误响应、引用冲突与 secret 安全边界；所有管理结果使用真实持久化数据。
- 重设计 Stratum 浅色视觉系统：采用 `rbp-portfolio` 的暖纸画布、暖白表面、炭黑主行动、稀缺鼠尾草绿选中态、靛蓝焦点环与低对比暖色阴影；浅色模式移除玻璃、发光、WebGL 和装饰性入场，深色模式维持现有方向。
- 更新 PRODUCT.md、`stratum-web/PRODUCT.md` 与 `stratum-web/DESIGN.md`，把 Studio 明确为服务开发者/管理员的第二界面，并归档浅色模式规则。

**非目标**：本 change 不实现 Agent 统计、监控、运行日志或告警面板；不管理 Tools、MCP、Workflow（`GET /v1/tools` 仅为 host 真实可注册目录的只读投影）；不引入资源关系画布；不在对话页暴露管理表单；不伪造 Provider、Model 或 Agent 的在线/就绪状态。

本 change 不取代当前进行中的 `add-ontology-list-canvas-frontend`；两者路由、领域与实现范围独立。

## Capabilities

### New Capabilities

- `studio-dashboard-ui`: Agent-first Studio 仪表盘、设置入口、管理表单、状态反馈、响应式与可访问性交互。
- `agent-definition-management`: Agent 定义的持久化 CRUD、校验、引用冲突与管理 API 契约。
- `llm-resource-management`: Provider 的持久化管理、immutable Model identity、secret 安全、连接测试、per-work DB snapshot、schema 驱动参数与管理 API 契约。

### Modified Capabilities

- `frontend-visual-system`: 扩展当前路由与导航范围，并为浅色模式建立来自 `rbp-portfolio` 的简约暖色视觉契约，同时保留既有深色模式。
- `agent-runtime-api`: `/v1/agent-templates` 与 runtime create 从 filesystem template 热读切换为 Studio definition 的数据库投影。
- `postgres-agent-runtime-storage`: immutable execution definition 改为从 Studio authoring definition 显式物化，并移除 `[agent]` catalog 配置。
- `session-runtime-identity`: AgentId 的作者 tag 来源改为 Studio definition，同时保持 exact name/tag 与 pinned immutable definition 语义。

## Impact

- **前端**：`stratum-web/` 新增 `/studio`、`/studio/agents/new`、`/studio/agents/[agent_name]` 与 `/studio/settings` 路由；扩展 typed API client、管理 feature/hooks 与 `components/stratum/studio/`；全局 product navigation 新增 Studio 入口和最右侧设置动作。
- **后端**：`stratum-api` 的 host 装配始终连接并验证 `stratum-studio`，每次新 LLM work / Turn 从数据库构建 Provider snapshot；`management_enabled` 只控制 loopback 管理路由及其 OpenAPI fragment，不再选择数据源，Studio 仍是启动与 readiness 的必需依赖。
- **配置与兼容性**：严格移除 `[agent]` 与 `[llm]`，环境 API key 不再影响 Provider runtime；保留 `/v1/models`、`/v1/agent-templates` 与既有错误码，但其结果只投影数据库 catalog。
- **领域与存储**：`stratum-studio` 的独立 PostgreSQL database 是 Agent definition、Provider、Model 与 credential 的唯一 authoring store；Provider secret 必须以 `secrecy::Secret` 处理且永不通过读取 API 回显。
- **视觉文档**：更新产品与设计权威文档、语义 token 和主题行为；仅对 `components/react-bits/SiteNav` 增加已确认的窄 `actions` 组合槽以承载最右设置动作，其他产品定制仍不直接修改受保护组件。
- **依赖**：不新增前端状态管理或表单依赖；复用现有 React、Tailwind、shadcn、GSAP，以及 Rust workspace 的 `secrecy 0.10` 与 `reqwest 0.12`。
