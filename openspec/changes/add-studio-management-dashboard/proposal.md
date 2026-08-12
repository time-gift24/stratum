## Why

Stratum 已能从配置加载 Provider、Model 与 Agent template，但缺少面向开发者和管理员的管理入口，也没有可写的资源管理 API；调整这些定义仍需直接修改文件并重启服务。现在需要新增一个 Agent-first Studio 仪表盘，在不侵入最终用户对话界面的前提下完成日常配置，并为后续 Agent 统计与监控能力留下稳定的信息架构。

## What Changes

- 新增独立 `/studio` 仪表盘：首期以真实 Agent 定义卡片为核心，不显示解释性 “Agents” 区块、伪造指标或空的监控占位；布局为后续 Agent 统计与监控面板保留可扩展区域。
- 在 Studio 顶部最右侧提供设置图标；Provider 与 Model 管理通过设置页进入，不作为仪表盘一级页签或独立“资源配置”导航。
- 新增 Agent 定义的列表、创建、读取、更新和删除能力；编辑以结构化表单为主、脱敏原始配置为辅，并支持校验、未保存提醒和删除冲突反馈。
- 新增 Provider 与 Model 的列表、创建、读取、更新和删除能力；Provider 支持凭据替换和连接测试，Model 参数继续以服务端 schema 驱动，避免前端硬编码能力。
- 为管理 API 定义分页、类型化 DTO、错误响应、引用冲突与 secret 安全边界；所有管理结果使用真实持久化数据。
- 重设计 Stratum 浅色视觉系统：采用 `rbp-portfolio` 的暖纸画布、暖白表面、炭黑主行动、稀缺鼠尾草绿选中态、靛蓝焦点环与低对比暖色阴影；浅色模式移除玻璃、发光、WebGL 和装饰性入场，深色模式维持现有方向。
- 更新 PRODUCT.md、`stratum-web/PRODUCT.md` 与 `stratum-web/DESIGN.md`，把 Studio 明确为服务开发者/管理员的第二界面，并归档浅色模式规则。

**非目标**：本 change 不实现 Agent 统计、监控、运行日志或告警面板；不管理 Tools、MCP、Workflow；不引入资源关系画布；不在对话页暴露管理概念；不伪造 Provider、Model 或 Agent 的在线/就绪状态。

本 change 不取代当前进行中的 `add-ontology-list-canvas-frontend`；两者路由、领域与实现范围独立。

## Capabilities

### New Capabilities

- `studio-dashboard-ui`: Agent-first Studio 仪表盘、设置入口、管理表单、状态反馈、响应式与可访问性交互。
- `agent-definition-management`: Agent 定义的持久化 CRUD、校验、引用冲突与管理 API 契约。
- `llm-resource-management`: Provider 与 Model 的持久化 CRUD、secret 安全、连接测试、schema 驱动参数与管理 API 契约。

### Modified Capabilities

- `frontend-visual-system`: 扩展当前路由与导航范围，并为浅色模式建立来自 `rbp-portfolio` 的简约暖色视觉契约，同时保留既有深色模式。

## Impact

- **前端**：`stratum-web/` 新增 `/studio`、`/studio/agents/new`、`/studio/agents/[agent_name]` 与 `/studio/settings` 路由；扩展 typed API client、管理 feature/hooks 与 `components/stratum/studio/`；站点导航新增 Studio 入口和右侧设置动作。
- **后端**：`stratum-api` 新增 `/v1/agent-definitions`、`/v1/providers` 与其 model/test 子资源的 REST API、OpenAPI DTO 与错误映射；应用装配支持持久化变更生效。
- **领域与存储**：Agent 定义仍遵循 `stratum-store`/既有文件系统边界；Provider secret 必须以 `secrecy::Secret` 处理且永不通过读取 API 回显。具体持久化边界由 design.md 决定。
- **视觉文档**：更新产品与设计权威文档、语义 token 和主题行为；不直接修改受保护的 `components/ui/*`、`components/react-bits/*` 或 `components/ai-elements/*`。
- **依赖**：预期不新增前端状态管理或表单依赖；优先复用现有 React、Tailwind、shadcn、GSAP 与 Rust workspace 依赖。
