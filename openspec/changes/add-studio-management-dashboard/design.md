## Context

Stratum 已有独立 `stratum-studio` PostgreSQL authoring store 与 Provider/Model/Agent definition 管理 API，但 host 装配仍保留三条配置路径：空库从 boot `[llm]` 与 templates seed、`management_enabled = false` 时从配置直接构建 Provider registry，以及 Provider endpoint/timeout 从配置读取。它们让相同资源在数据库和部署配置之间存在漂移空间，也把管理路由是否暴露错误地耦合到运行时数据源。

本 change 跨越 `stratum-config`、`stratum-llm`、`stratum-api`、持久化文件边界和 `stratum-web`。管理界面属于 Impeccable 的 Operate 模式：熟悉、清晰和状态可信优先于表达性。用户确认 `/studio` 仍是仪表盘，但首期只以 Agent 卡片承载真实内容；后续统计和监控将围绕 Agent 加入，当前不得放置空面板或示例指标。Provider / Model 不成为一级页签，而从全局 product navigation 最右侧的设置图标进入。

浅色视觉参考为本机 `rbp-portfolio` 的 “Sunlit Reading Room”：暖纸画布、暖白表面、炭黑动作、稀缺鼠尾草绿和低对比暖色阴影。可借鉴的是暖色阶、滑动选中底片、稳定的 inline 状态反馈和响应式表单；WebGL shader、site frame、Lenis、物理 chips、展示型 hover lift 与大标题衬线不适合管理任务，明确不移植。

## Goals / Non-Goals

**Goals:**

- 提供 Agent-first `/studio` 仪表盘与可写的 Agent definition、Provider、Model 管理能力。
- 让所有写入可校验、可并发保护、可原子持久化；Agent definition 变更作用于之后新建的 AgentRuntime，Provider / Model 变更作用于之后开始的 LLM work / Turn。
- 让 Studio PostgreSQL 成为 Provider、Model、credential 与 Agent definition 的唯一 authoring/runtime truth；配置只负责数据库连接、HTTP 暴露与非资源型运维参数。
- 保证 AgentRuntime 持有创建时的 immutable definition，而只有正在执行的 Turn pin 住该次 work 从 Studio DB 组装的 Provider `Arc`；管理写入不改变 in-flight Turn，下一次 work 重新读取数据库。
- 结构化表单为主、raw config 为辅；secret 不回显、不进入 raw config、日志或错误。
- 全站浅色主题切换到 `rbp-portfolio` 的暖色 soft-minimalism，同时保留现有深色视觉世界和对话行为。
- 产品定制放在 `components/stratum/` 与使用方；受保护组件只允许已确认的 `SiteNav actions` 窄接口例外，不扩散 Studio 业务逻辑。

**Non-Goals:**

- Agent 统计、监控、运行日志、告警、成本分析或其占位 UI。
- Tools、MCP、Workflow、Session 或运行实例管理；`GET /v1/tools` 只是 host 可注册工具的只读投影，不构成 Tool 管理。
- 任意第三方 Provider plugin、自定义 base URL、Provider SDK marketplace；首期只管理当前真实支持的 OpenAI 与 DeepSeek。
- 远程多租户管理、账号、RBAC 或审计后台；首期管理面仅在显式启用且 API 绑定 loopback 时开放。
- 修改既有对话事件、审批、恢复、取消或历史协议。

## Decisions

### D1: `/studio` 是 Agent-first 仪表盘，不是 “Agents” 资源页

首屏由稳定的页面标题、搜索、新建 Agent 动作和 Agent 卡片网格组成；最右设置图标属于全局 product navigation，不在仪表盘内重复。卡片显示真实的 `agent_name`、Provider / Model、tool 数量与更新时间，不显示 Prompt 摘要、解释性 “Agents” 标题、说明文案、健康灯、在线状态或伪统计。空态直接说明尚无 Agent 并提供创建动作。

后续统计/监控可以在 header 与卡片网格之间增加真实模块，但本 change 不为它建立 plugin slot、抽象 dashboard schema 或隐藏占位。这样保留视觉空间而不违反项目的克制设计原则。被否决方案是 Provider → Model → Agent 的依赖谱系首屏，以及 “Agents / 资源配置” 一级页签；前者让低频配置压过主要任务，后者违背用户指定的设置入口。

### D2: 全局 product navigation 最右设置图标进入独立 Settings surface

全局 product navigation 最右侧齿轮按钮使用明确的 `aria-label="设置"` 并进入 `/studio/settings/providers`。该图标是一个安静的通用 product action，不增加“资源配置”文字入口，也不把 Provider / Model 提升为一级导航。Settings 内只使用轻量的 Provider / Model 二级切换。桌面采用列表 + 独立详情路由，移动端按列表 → 全页表单下钻：

- `/studio/settings/providers` 与 `/studio/settings/providers/[provider]`
- `/studio/settings/models` 与 `/studio/settings/models/[model_id]`

创建路由使用 `/new`。Agent 编辑同样使用 `/studio/agents/new` 与 `/studio/agents/[agent_name]`，不用 modal 或容易裁切的 drawer 承载长表单。浏览器后退、深链接和未保存拦截因此保持自然。

### D3: REST 资源与运行实例分离

新增管理资源端点：

- `/v1/agent-definitions` 与 `/v1/agent-definitions/{agent_name}`
- `/v1/providers` 与 `/v1/providers/{provider}`
- `/v1/providers/{provider}/models` 与 `/v1/providers/{provider}/models/{model_name}`
- `POST /v1/providers/{provider}/test`
- `GET /v1/tools`

`/v1/agent-runtimes` 继续表示运行实例，不复用为 definition CRUD；既有 `/v1/agent-templates` 与 `/v1/models` 保持向后兼容并从同一最新 Studio catalog 投影。Agent definition 使用 POST/PUT/DELETE，Provider 使用 POST/PUT/DELETE；Model 是 Provider-scoped immutable identity，只使用 list/POST/GET/DELETE，不暴露没有独立语义的 PUT，所谓更新必须先通过引用检查删除再显式创建。`GET /v1/tools` 只投影 host 当前真实可注册的工具，不把 Tool 变成可管理资源。每个 Handler 与 DTO 纳入 utoipa OpenAPI。错误继续使用统一 envelope，并增加稳定的 `studio_conflict`、`studio_precondition_failed`、`provider_test_failed` 等安全错误码。

### D4: `stratum-studio` PostgreSQL 是唯一 authoring store

API host 启动时必须连接配置的 Studio database 并应用 `stratum-studio` 自有 migration；缺少 database URL、连接失败或 catalog 损坏均 fail closed。Provider、Model、credential 与 Agent definition 的管理读取和写入只经过具体 `StudioStore`，不新增 repository/service trait，也不访问 `[llm]`、template 文件或 execution ledger。

写入在 Studio transaction 内维护 revision 与引用不变量；create/update 的返回 representation 必须在该 transaction 内读取并完成类型解码，成功提交即以数据库为唯一完成状态，commit 后不得再查询资源或重建 Provider manager。Model create 所需 adapter 校验与 parameter schema 也必须在 mutation 前从当时的 DB credential snapshot 取得，因此响应组装没有第二个可失败 I/O 点。管理读取直接查询 Studio database；每次新的 LLM work / Turn 在开始边界读取一个一致的 Provider / Model / credential snapshot 并组装新的 `LlmProviderManager`。进程不保留可热替换的 production catalog/manager cache，也不在 `.await` 期间持有 std guard。

被否决方案：保留 config fallback（继续制造双真相）、把数据库只用于管理 API 而让 runtime 读配置（保存成功不等于生效）、为单一 PostgreSQL 后端增加 repository trait（没有第二实现需求）。

### D5: 空 catalog 显式管理，不做隐式 seed

启动不再从 `[llm]`、环境 API key 或 `/templates` 导入 Studio 资源。空 database 是合法且可观察的状态：Provider、Model 与 Agent definition 列表为空，`/v1/models` 与 `/v1/agent-templates` 返回空投影；创建依赖未满足的 AgentRuntime 返回既有类型化错误。管理员必须经 loopback management API 按 Provider → Model → Agent definition 的显式顺序建立 catalog。

现有 Studio database 已包含 seed 数据时原样保留，migration 不重写 credential、revision 或资源时间。过渡版本曾把省略的 DeepSeek provider parameters 持久化为 `{}`；adapter 将这个空对象解释为其 schema 已声明的 disabled-thinking 默认值，既不改写数据库，也不放宽任何非空无效对象。更新 definition 只影响之后创建的 runtime Agent；现存 Agent 已持久化 resolved definition 与 runtime snapshot，继续使用原语义。删除 definition 不删除历史或运行实例，只阻止以后以该名称创建 Agent。

### D6: 每次新 LLM work 从 DB 组装，只有 in-flight Turn pin Provider Arc

开始一次新的 LLM work / Turn 时，host 从 Studio PostgreSQL 读取 Provider、credential 与 Models，完整验证后组装本次 work 使用的 provider manager；真正执行中的 Turn 持有捕获的 `Arc<dyn LlmProvider>`，因此中途写入不会换 key、模型或中断调用。Turn 结束后不把该 manager 当作下一次 work 的权威 cache；同一 AgentRuntime 的下一次 Turn 也重新读取 DB，从而自然看见已提交的 Provider / Model 变更。

Model 是 Provider-scoped immutable identity。单个 Model 删除前检查 Agent definitions；被引用时返回 409 blocker。Provider 删除只检查其 owned Models 是否被 Agent definition 引用：有引用时返回 Agent blocker，绝不 cascade 删除或迁移 Agent；没有引用时在同一 Studio transaction 内删除 owned Models、credential 与 Provider。系统不维护独立的全局 default Model；每个 definition 显式选择 Model。

Provider 首期是闭集 enum `openai | deepseek`。endpoint 与 timeout 是 adapter policy，使用代码内固定可信值，不属于部署配置或 Studio 可管理数据：OpenAI runtime base URL 固定为 `https://api.openai.com/v1`，DeepSeek 固定为 `https://api.deepseek.com`；两者共享 connect/request/first-response/stream-idle = 10s/120s/30s/60s 的代码内 timeout policy。这样彻底移除 `[llm]` 对 runtime 的影响，同时避免自定义 URL 与连接测试带来的 SSRF。未来若要让 transport 可配置，必须以独立安全设计加入 Studio schema 与校验，不能退回 boot config。

### D7: secret 通过单向写入边界管理

Provider 创建要求 API key；更新请求的 `api_key` 可省略以保留已有值。读取响应只返回 `credential_configured: bool`，raw config 也不包含占位 key、长度、前后缀或 hash。API key 进入 Rust 后立即包装为 `secrecy::SecretString` / 现有 `ApiKey`，HTTP DTO 不实现会泄露 secret 的 Debug；日志、tracing 与错误只记录 provider enum 和安全错误码。

本 change 复用 workspace `secrecy 0.10`（`Apache-2.0 OR MIT`）。选择它是为了使用默认脱敏 Debug、显式 `ExposeSecret` 边界与 drop 时由 `zeroize` 清理，而不是维护容易误实现 Serialize/Debug 的自定义包装。启用 `serde` 仅用于 secret 输入反序列化；`SecretString` 默认不可序列化，持久化必须通过 catalog 的单一显式编码边界。依赖继续受现有 CI `cargo audit` 与 `cargo deny check` 门禁约束。

Provider test 是瞬时命令，不保存“在线/就绪”状态。它复用 workspace `reqwest 0.12`，只向固定 `https://api.openai.com/v1/models` 或 `https://api.deepseek.com/models` 发送带当前 credential snapshot 的 GET；connect 与 overall timeout 均固定 10s，redirect policy 为 none，判定只读取 HTTP status、绝不读取或记录 response body。所有 transport/status failure 都收敛为脱敏错误。UI 只显示本次请求的 pending/success/failure，刷新后消失。`[studio].management_enabled` 只决定是否注册 management routes 及其 OpenAPI fragment；无论其值为何，host 都必须连接 Studio DB、验证 catalog、把 Studio 纳入 readiness，并在每次新 work 从中装配 runtime。管理路由仅在 bind address 为 loopback 时允许启用；远程管理需要后续带认证的独立设计。

### D8: ETag + If-Match 保护并发编辑

单资源 GET 返回由持久化 revision 计算的强 ETag。Agent definition 与 Provider 的 PUT、以及全部 DELETE 必须携带 `If-Match`；revision 不一致返回 412，前端保留本地表单并提供重新加载，不静默覆盖。Model 不提供 PUT。POST 对重复 `agent_name`、已存在 Provider 或重复 Model 返回 409。PostgreSQL row/catalog lock 与 transaction 保证 check-and-write 原子；提交失败时数据库保持旧状态，因为没有需要提交后热同步的 runtime registry。

### D9: 前端采用 route-local hooks 与显式表单状态

typed client 在 `lib/stratum/api.ts` 增加 management 方法；`features/studio-management/` 保存 DTO 映射、表单 reducer、schema 表单解析与 revision 状态；`hooks/` 编排请求副作用。不引入 zustand、React Query 或表单依赖。表单 reducer 明确区分 `loaded / dirty / saving / conflict / invalid / testing`，路由离开和 `beforeunload` 只在 dirty 时阻止。

Agent 主表单包含 name（创建后不可改）、author-supplied `agent_version`、model、schema 驱动的 model parameters、tools 与 system prompt。创建和每次更新都必须在线路上携带 `agent_version`，更新时必须分配与当前值不同的新 tag；tools 选项来自受 management gate 保护的 `GET /v1/tools` 真实 host catalog。结构化视图是默认；raw config 是次级高级视图，Agent TOML 可编辑并须通过同一 parser/validator 回填结构化状态。Provider raw 视图永不显示或编辑 secret；Model 详情只读展示 identity 与服务端 parameter schema，改变 identity 必须删除后重建。模型能力与参数控件必须从服务端 schema 解析，不硬编码 Thinking 等级。

### D10: 浅色主题采用暖色 soft-minimalism，深色主题不重做

浅色 token 映射：`background #F7F4EE`、`card/popover #FAF8F3`、`foreground #2E2D2A`、`primary #383735`、`primary-foreground #F7F4EE`、`secondary/muted #F0ECE3`、`border/input #E7E2DA`、`accent #9EB6A6`、`accent-foreground #2E2D2A`、`ring #7378D8`、`destructive #B3462F`。`muted-foreground` 采用满足正文 4.5:1 的较深暖灰；参考中的 `#9AA3B2` 只用于非关键 placeholder/disabled 信息，不承载正文。

浅色表面靠暖色阶和 5–12% 暖墨阴影分层，不使用 `backdrop-blur`、玻璃透明度、霓虹/BorderGlow、WebGL、渐变氛围或编排式页面入场。鼠尾草绿覆盖不超过约 10%，只用于当前选择与成功强调；主操作使用炭黑。设置二级切换可以借鉴 rbp nav 的滑动选中底片，但通过现有 GSAP/CSS 实现，不引入 Motion。深色继续使用当前石墨背景、绿色 primary、必要玻璃和具有语义的输入 glow。

全局导航的新产品编排落在 `components/chrome/site-chrome.tsx` 与使用方，通过主题分支提供实色浅色形态。为承载最右侧 product actions，用户已确认可对受保护的 `components/react-bits/SiteNav` 增加一个窄、数据驱动的 `actions` slot；该例外不授权把 Studio 业务、主题特例或其他产品逻辑下沉到底稿。`components/ui/*` 与 `components/ai-elements/*` 仍不直接修改；`PromptInput` 等自有组件在浅色禁用 glow，在深色保留。

### D11: Agent raw config 使用 `smol-toml` 1.7.0

Agent raw config 必须接受后端 `toml` crate 生成的 canonical TOML（包括 table、inline table、转义字符串和空 `model_parameters` 省略），并拒绝未知顶层字段；正则或逐行 JSON 近似解析无法忠实满足该边界。前端因此使用锁定在 `pnpm-lock.yaml` 的 `smol-toml` 1.7.0 做 parse/stringify，解析后仍由 Stratum 自有代码限制为 `agent_version/model/model_parameters/tools/prompt` 和 JSON-compatible 参数，最终保存继续交给后端同一 validator，不把第三方 parser 当成信任边界。`agent_name` 由创建表单/资源路径管理，不在 raw 更新视图中允许改名。

该包许可证经已安装包 metadata 与 LICENSE 核对为 BSD-3-Clause，运行时无传递依赖，仓库为 `squirrelchat/smol-toml`。安全与维护取舍：增加一个浏览器依赖和上游维护面，但避免自制 TOML parser 的歧义与静默丢字段；已知的超 53-bit integer 与日期语义限制不扩大产品能力——Model parameters 本身是 JSON 值，日期对象被拒绝，超安全整数仍由字段/schema 与服务端校验拒绝。版本升级必须经过 lockfile review、前端验证和 raw config 回归，不自动信任新的 minor 行为。

## Risks / Trade-offs

- [移除 boot seed 后新部署没有可用模型] → 允许空 catalog 启动并提供明确空态；部署流程要求先通过 loopback Studio API 创建 Provider 与 Model，再创建 Agent definition。
- [旧部署仍保留 `[llm]` 并误以为会生效] → 配置 schema 拒绝已移除的 Provider resource 字段，示例和运维文档只保留 Studio database URL；升级说明标记 breaking change。
- [每次 work 读取 Studio 增加数据库查询与 adapter 组装成本] → 这是消除进程 cache 双真相的有意取舍；数据库读取使用一致 snapshot，只有 in-flight Turn pin Provider `Arc`。UI 分别说明 definition 变更影响后续新建 AgentRuntime，而 Provider / Model 变更影响下一次 LLM work，不声称重配当前 Turn。
- [Provider 测试误导为持续健康状态] → 结果不持久化、不出现在卡片状态点，只在用户主动测试后展示本次结果与时间。
- [暖色浅色系统降低 Stratum 的科技辨识度] → 品牌保留在标志、稀缺选择色与严谨的信息层级；Operate surface 不使用作品集 shader 或展示动效。
- [raw config 与结构化表单双向同步复杂] → 只允许 canonical TOML/JSON，解析成功才提交到共享 draft；错误保留 raw 文本并定位，不污染有效结构化状态。
- [管理 API 无远程认证] → 首期 fail closed 为 loopback + 显式 enable；远程管理保持非目标，不以 CORS 代替认证。

## Migration Plan

1. 为现有 `stratum_studio` schema 应用向前 migration；保留全部现有 Provider、Model、credential、Agent definition 与 revision。
2. 将 Studio database URL 改为 API host 必需配置；删除 Provider boot seed、环境 API key 注入、config fallback 与 config transport 读取。
3. host 无条件连接并验证 Studio DB，把它纳入 readiness，并在每次新 LLM work / Turn 从 DB 组装 Provider snapshot；`management_enabled` 仅控制 loopback management routes 与 OpenAPI fragment；保留 `/v1/models`、`/v1/agent-templates` 和既有对话协议。
4. 更新配置示例、Docker/local 启动方式与相关 AGENTS.md，运行 DB-only 回归、Rust fmt/clippy/test、前端 lint/typecheck/build 和真实重启验证。
5. 回滚前必须备份 Studio database；回退到依赖 `[llm]` 的旧二进制需要显式重建旧配置，不能假设数据库写入会自动导出为 config。

## Open Questions

- 远程、多用户 Studio 需要哪种认证与授权边界；该问题不阻塞仅 loopback 的首期实现。
- 后续 Agent 统计/监控的数据模型与卡片上方布局由独立 change 设计，本 change 不预设指标或 dashboard abstraction。
