## Context

Stratum 当前从根配置的 `[llm]` 段装配 OpenAI 与 DeepSeek Provider，并从 agent storage root 下的 `/templates/*.toml` 读取 Agent template。`GET /v1/models` 与 `GET /v1/agent/templates` 只能读取已解析结果；没有写 API，`LlmProviderManager` 也在启动后固定。Web 端现有 `/conversation` 与 `/excalidraw`，顶部是面向最终用户的悬浮导航，尚无服务开发者/管理员的工作区。

本 change 跨越 `stratum-config`、`stratum-llm`、`stratum-api`、持久化文件边界和 `stratum-web`。管理界面属于 Impeccable 的 Operate 模式：熟悉、清晰和状态可信优先于表达性。用户确认 `/studio` 仍是仪表盘，但首期只以 Agent 卡片承载真实内容；后续统计和监控将围绕 Agent 加入，当前不得放置空面板或示例指标。Provider / Model 不成为一级页签，而从 Studio 顶部最右侧的设置图标进入。

浅色视觉参考为本机 `rbp-portfolio` 的 “Sunlit Reading Room”：暖纸画布、暖白表面、炭黑动作、稀缺鼠尾草绿和低对比暖色阴影。可借鉴的是暖色阶、滑动选中底片、稳定的 inline 状态反馈和响应式表单；WebGL shader、site frame、Lenis、物理 chips、展示型 hover lift 与大标题衬线不适合管理任务，明确不移植。

## Goals / Non-Goals

**Goals:**

- 提供 Agent-first `/studio` 仪表盘与可写的 Agent definition、Provider、Model 管理能力。
- 让所有写入可校验、可并发保护、可原子持久化，并在不重启的情况下作用于后续新建 Agent。
- 保证运行中的 Agent 持有创建时的 provider/definition 快照，不因管理写入中断或漂移。
- 结构化表单为主、raw config 为辅；secret 不回显、不进入 raw config、日志或错误。
- 全站浅色主题切换到 `rbp-portfolio` 的暖色 soft-minimalism，同时保留现有深色视觉世界和对话行为。
- 保持受保护组件源码不变，产品定制放在 `components/stratum/` 与使用方。

**Non-Goals:**

- Agent 统计、监控、运行日志、告警、成本分析或其占位 UI。
- Tools、MCP、Workflow、Session 或运行实例管理。
- 任意第三方 Provider plugin、自定义 base URL、Provider SDK marketplace；首期只管理当前真实支持的 OpenAI 与 DeepSeek。
- 远程多租户管理、账号、RBAC 或审计后台；首期管理面仅在显式启用且 API 绑定 loopback 时开放。
- 修改既有对话事件、审批、恢复、取消或历史协议。

## Decisions

### D1: `/studio` 是 Agent-first 仪表盘，不是 “Agents” 资源页

首屏由稳定的 Studio header、搜索、新建 Agent 动作、最右设置图标和 Agent 卡片网格组成。卡片显示真实的 `agent_name`、Provider / Model、tool 数量与更新时间，不显示 Prompt 摘要、解释性 “Agents” 标题、说明文案、健康灯、在线状态或伪统计。空态直接说明尚无 Agent 并提供创建动作。

后续统计/监控可以在 header 与卡片网格之间增加真实模块，但本 change 不为它建立 plugin slot、抽象 dashboard schema 或隐藏占位。这样保留视觉空间而不违反项目的克制设计原则。被否决方案是 Provider → Model → Agent 的依赖谱系首屏，以及 “Agents / 资源配置” 一级页签；前者让低频配置压过主要任务，后者违背用户指定的设置入口。

### D2: 设置图标进入独立 Settings surface

Studio header 最右侧齿轮按钮使用明确的 `aria-label="设置"` 并进入 `/studio/settings/providers`。Settings 内只使用轻量的 Provider / Model 二级切换，不进入全局 SiteNav，也不把“资源配置”提升为产品一级导航。桌面采用列表 + 独立详情路由，移动端按列表 → 全页表单下钻：

- `/studio/settings/providers` 与 `/studio/settings/providers/[provider]`
- `/studio/settings/models` 与 `/studio/settings/models/[model_id]`

创建路由使用 `/new`。Agent 编辑同样使用 `/studio/agents/new` 与 `/studio/agents/[agent_name]`，不用 modal 或容易裁切的 drawer 承载长表单。浏览器后退、深链接和未保存拦截因此保持自然。

### D3: REST 资源与运行实例分离

新增管理资源端点：

- `/v1/agent-definitions` 与 `/v1/agent-definitions/{agent_name}`
- `/v1/providers` 与 `/v1/providers/{provider}`
- `/v1/providers/{provider}/models` 与 `/v1/providers/{provider}/models/{model_name}`
- `POST /v1/providers/{provider}/test`

`/v1/agents` 继续表示运行实例，不复用为 definition CRUD；既有 `/v1/agent/templates` 与 `/v1/models` 保持向后兼容并从同一最新 catalog 投影。列表统一 `page/per_page/sort`，写操作使用 POST/PUT/DELETE；每个 Handler 与 DTO 纳入 utoipa OpenAPI。错误继续使用统一 envelope，并增加稳定的 `resource_conflict`、`revision_conflict`、`provider_test_failed` 等安全错误码。

### D4: 复用现有文件边界，不新增 repository/service trait

Agent definition 继续使用 `/templates/{agent_name}.toml`；运行期读写统一经过 `stratum-store` 的具体 `FilesystemConfigurationStore`，再由它使用 `stratum-filesystem` 的 CAS 与原子写能力。LLM 管理 catalog 使用 storage root 内单个 `/providers/catalog.toml`，包含默认 model、OpenAI/DeepSeek 是否配置、各自 models 与凭据。单文件保证 Provider、Model 和 default model 的引用不变量能在一次原子替换中提交；文件权限设为仅运行用户可读写。该 Store 是当前唯一后端，因此不新增 repository/service trait。

`HostState` 增加一个具体的、进程内 management catalog 状态与有界写临界区，不引入只有单实现的 repository/service trait。写流程为：在锁内基于当前 revision 构造 candidate → 完整验证并构建新的 provider manager → 原子写文件 → 替换内存 catalog/manager。读取先 clone 快照再释放锁，不在 await 期间持有 std guard。

被否决方案：直接改原始 `config.toml`（host 已丢失可靠的来源路径且容易覆盖运维配置）、为三个资源新增数据库（超出当前存储模型）、每个 model 单独文件（需要跨文件事务维护 default/reference）。

### D5: boot config 只作为首次 seed，managed catalog 成为后续真相

启动时若 `/providers/catalog.toml` 不存在，则用当前 `[llm]` 配置生成并原子写入；存在时优先读取 managed catalog。随后扫描独立的 `/templates` 目录，对每个 canonical Agent name、strict TOML、Model、parameters 与 tool 引用做完整恢复校验；任一异常都 fail closed，禁止返回只恢复部分 definition 的 Host。旧配置继续作为首次部署与回滚输入，不在每次启动覆盖管理变更。回滚到不支持 Studio 的版本时，运维方仍可从原 config 启动；managed catalog 不会破坏旧二进制。

Agent definitions 已位于 `/templates`，无需数据迁移。更新 definition 只影响之后创建的 runtime Agent；现存 Agent 已持久化 resolved definition 与 runtime snapshot，继续使用原语义。删除 definition 不删除历史或运行实例，只阻止以后以该名称创建 Agent。

### D6: catalog 热替换不改变进行中的 Turn

可配置 provider 保持 `Arc` 快照语义。Provider credential 或 model catalog 更新成功后，新的 `LlmProviderManager` 原子替换为 HostState 的当前 manager；已经构造的 Agent/Turn 继续持有旧 `Arc<dyn LlmProvider>`，不会在执行中换 key 或模型。之后的新 Agent 使用新 manager。删除 model/provider 前检查 default model 与所有 Agent definitions；有引用时返回 409 和结构化 blocker 列表，不做 cascade 猜测。

Provider 首期是闭集 enum `openai | deepseek`，base URL 使用当前代码中的固定可信地址。这样避免自定义 URL 与连接测试带来的 SSRF，同时忠实反映当前 adapter 能力。未来增加真正的 provider kind 时再扩展 enum，不先造 plugin 层。

### D7: secret 通过单向写入边界管理

Provider 创建要求 API key；更新请求的 `api_key` 可省略以保留已有值。读取响应只返回 `credential_configured: bool`，raw config 也不包含占位 key、长度、前后缀或 hash。API key 进入 Rust 后立即包装为 `secrecy::SecretString` / 现有 `ApiKey`，HTTP DTO 不实现会泄露 secret 的 Debug；日志、tracing 与错误只记录 provider enum 和安全错误码。

本 change 新增直接依赖 `secrecy 0.8.0`（`Apache-2.0 OR MIT`）。选择它是为了使用默认脱敏 Debug、显式 `ExposeSecret` 边界与 drop 时由 `zeroize` 清理，而不是维护容易误实现 Serialize/Debug 的自定义包装。启用 `serde` 仅用于 secret 输入反序列化；`SecretString` 默认不可序列化，持久化必须通过 catalog 的单一显式编码边界。依赖继续受现有 CI `cargo audit` 与 `cargo deny check` 门禁约束。

Provider test 是瞬时命令，不保存“在线/就绪”状态。它使用 adapter 定义的低副作用连接探测、固定超时和脱敏错误；UI 只显示本次请求的 pending/success/failure，刷新后消失。管理路由仅在 `api.management_enabled = true` 且 bind address 为 loopback 时注册；非 loopback 配置在启动校验阶段失败。远程管理需要后续带认证的独立设计。

### D8: ETag + If-Match 保护并发编辑

单资源 GET 返回由 canonical persisted representation 计算的强 ETag。PUT/DELETE 必须携带 `If-Match`；revision 不一致返回 412，前端保留本地表单并提供重新加载，不静默覆盖。POST 对重复 `agent_name`、已存在 Provider 或重复 Model 返回 409。HostState 的写临界区保证同进程 check-and-write 原子，文件写采用临时文件 + rename，并满足文件/目录落盘要求。

### D9: 前端采用 route-local hooks 与显式表单状态

typed client 在 `lib/stratum/api.ts` 增加 management 方法；`features/studio-management/` 保存 DTO 映射、表单 reducer、schema 表单解析与 revision 状态；`hooks/` 编排请求副作用。不引入 zustand、React Query 或表单依赖。表单 reducer 明确区分 `loaded / dirty / saving / conflict / invalid / testing`，路由离开和 `beforeunload` 只在 dirty 时阻止。

Agent 主表单包含 name（创建后不可改）、model、schema 驱动的 model parameters、tools 与 system prompt。结构化视图是默认；raw config 是次级高级视图，Agent TOML 可编辑并须通过同一 parser/validator 回填结构化状态。Provider raw 视图永不显示或编辑 secret；Model raw 视图展示服务端 parameter schema，schema 本身只读。模型能力与参数控件必须从服务端 schema 解析，不硬编码 Thinking 等级。

### D10: 浅色主题采用暖色 soft-minimalism，深色主题不重做

浅色 token 映射：`background #F7F4EE`、`card/popover #FAF8F3`、`foreground #2E2D2A`、`primary #383735`、`primary-foreground #F7F4EE`、`secondary/muted #F0ECE3`、`border/input #E7E2DA`、`accent #9EB6A6`、`accent-foreground #2E2D2A`、`ring #7378D8`、`destructive #B3462F`。`muted-foreground` 采用满足正文 4.5:1 的较深暖灰；参考中的 `#9AA3B2` 只用于非关键 placeholder/disabled 信息，不承载正文。

浅色表面靠暖色阶和 5–12% 暖墨阴影分层，不使用 `backdrop-blur`、玻璃透明度、霓虹/BorderGlow、WebGL、渐变氛围或编排式页面入场。鼠尾草绿覆盖不超过约 10%，只用于当前选择与成功强调；主操作使用炭黑。设置二级切换可以借鉴 rbp nav 的滑动选中底片，但通过现有 GSAP/CSS 实现，不引入 Motion。深色继续使用当前石墨背景、绿色 primary、必要玻璃和具有语义的输入 glow。

全局导航的新产品实现落在 `components/stratum/chrome/` 或现有使用方，通过主题分支提供实色浅色形态；不直接修改 `components/ui/*`、`components/react-bits/*`、`components/ai-elements/*`。`PromptInput` 等自有组件在浅色禁用 glow，在深色保留。

### D11: Agent raw config 使用 `smol-toml` 1.7.0

Agent raw config 必须接受后端 `toml` crate 生成的 canonical TOML（包括 table、inline table、转义字符串和空 `model_parameters` 省略），并拒绝未知顶层字段；正则或逐行 JSON 近似解析无法忠实满足该边界。前端因此使用锁定在 `pnpm-lock.yaml` 的 `smol-toml` 1.7.0 做 parse/stringify，解析后仍由 Stratum 自有代码限制为 `model/model_parameters/tools/prompt` 和 JSON-compatible 参数，最终保存继续交给后端同一 validator，不把第三方 parser 当成信任边界。

该包许可证经已安装包 metadata 与 LICENSE 核对为 BSD-3-Clause，运行时无传递依赖，仓库为 `squirrelchat/smol-toml`。安全与维护取舍：增加一个浏览器依赖和上游维护面，但避免自制 TOML parser 的歧义与静默丢字段；已知的超 53-bit integer 与日期语义限制不扩大产品能力——Model parameters 本身是 JSON 值，日期对象被拒绝，超安全整数仍由字段/schema 与服务端校验拒绝。版本升级必须经过 lockfile review、前端验证和 raw config 回归，不自动信任新的 minor 行为。

## Risks / Trade-offs

- [managed catalog 与原 config 出现两个配置来源] → 明确“一次 seed、以后 managed catalog 优先”，启动日志只记录来源类型，不记录内容；文档提供导出/回退步骤。
- [热替换时运行 Agent 与新定义短暂不同] → 这是有意的 snapshot 语义；UI 保存成功文案说明变更影响后续新建 Agent，不声称重配现存运行实例。
- [Provider 测试误导为持续健康状态] → 结果不持久化、不出现在卡片状态点，只在用户主动测试后展示本次结果与时间。
- [暖色浅色系统降低 Stratum 的科技辨识度] → 品牌保留在标志、稀缺选择色与严谨的信息层级；Operate surface 不使用作品集 shader 或展示动效。
- [raw config 与结构化表单双向同步复杂] → 只允许 canonical TOML/JSON，解析成功才提交到共享 draft；错误保留 raw 文本并定位，不污染有效结构化状态。
- [管理 API 无远程认证] → 首期 fail closed 为 loopback + 显式 enable；远程管理保持非目标，不以 CORS 代替认证。

## Migration Plan

1. 在 workspace/config 中加入受校验的 `management_enabled`，默认关闭；现有部署行为不变。
2. 启动时检测 managed catalog；不存在则从 `[llm]` 一次性 seed 并设置安全文件权限，存在则读取、验证并装配。
3. 新增 API 与 Web 路由；保留 `/v1/models`、`/v1/agent/templates` 和既有对话消费者。
4. 更新 PRODUCT.md、`stratum-web/PRODUCT.md`、DESIGN.md 与相关 AGENTS.md；运行前端 detector、lint/typecheck/build 与 Rust fmt/clippy/test。
5. 回滚时关闭 `management_enabled` 或回退二进制；旧 config 仍可启动，managed catalog 可保留以便再次升级。任何删除都不触及 agent history。

## Open Questions

- 远程、多用户 Studio 需要哪种认证与授权边界；该问题不阻塞仅 loopback 的首期实现。
- 后续 Agent 统计/监控的数据模型与卡片上方布局由独立 change 设计，本 change 不预设指标或 dashboard abstraction。
