## 1. 实现准备与产品文档

- [x] 1.1 按 `using-git-worktrees` 在新的 `codex/` 分支与独立 worktree 开始实现；重新读取 CONSTITUTION.md、rust-skills、PRODUCT.md、`stratum-web/PRODUCT.md` 与 `stratum-web/DESIGN.md`
- [x] 1.2 按 Impeccable 记录 `/studio` Operate surface brief：Agent-first 仪表盘、全局 product navigation 最右设置入口、无 Agents 页签/解释区/Prompt 摘要/监控占位，以及浅色 `rbp-portfolio` 取舍
- [x] 1.3 更新根 PRODUCT.md 与 `stratum-web/PRODUCT.md`：加入面向开发者/管理员的 Studio 第二界面，明确首期和非目标，不改变最终用户对话定位
- [x] 1.4 更新 `stratum-web/DESIGN.md`：归档暖色 light tokens、实色表面、scarce sage、indigo focus、warm shadows、light 禁用 glow/glass/WebGL，以及 dark 主题保留规则

## 2. 配置、secret 与持久化基础

- [x] 2.1 在 `stratum-config` 增加受 serde 严格校验的 `management_enabled`（默认 false），并拒绝非 loopback bind 启用管理面；补齐类型化错误与单测
- [x] 2.2 将 Provider credential 的内存表示迁移到 `secrecy` secret 类型，确保 Debug/Display、错误链和 tracing 不泄露；增加序列化边界测试
- [x] 2.3 在独立 `stratum-studio` PostgreSQL schema 定义 Provider、Model、credential、Agent definition 与 catalog revision；不引入通用 repository/provider plugin 抽象
- [x] 2.4 删除 management catalog/template 对 `LocalFilesystem` authoring 的依赖；`LocalFilesystem` 只保留 sandbox 业务文件能力，不再承担 Provider、Model、credential 或 Agent definition 的持久化
- [x] 2.5 实现独立 Studio PostgreSQL catalog 与启动恢复边界；后续 DB-only 收敛在 12.1 删除过渡 seed，并保持已有 catalog 不被覆盖
- [x] 2.6 为 Agent definition、Provider 与 Model 实现持久化 UUID revision 的强 ETag 格式化/解析，并覆盖 roundtrip、`If-Match` 与过期 revision 语义

## 3. Provider per-work DB snapshot

- [x] 3.1 生产路径删除长期驻留、需要热替换的 Provider catalog/manager cache；每次新的 LLM work / Turn 从 Studio DB 读取一致的 Provider/Model/credential snapshot 并组装 manager，测试注入路径保持明确隔离
- [x] 3.2 Studio create/update 在 transaction 内物化并解码返回 representation，Model adapter/schema 在写前取得；commit 成功即成为唯一完成状态，之后不再查询资源、重建 Provider manager 或同步内存 catalog，读取/组装失败返回类型化错误且不在 `.await` 期间持有 std guard
- [x] 3.3 只让 in-flight Turn 持有捕获的 `Arc<dyn LlmProvider>`；当前 Turn 不重新查询或替换该 Arc，同一 AgentRuntime 的下一次 LLM work 重新从 DB 读取 Provider snapshot
- [x] 3.4 为 OpenAI 与 DeepSeek 实现固定 `/models` endpoint、10s connect/overall timeout、禁止 redirect 且不读取 response body 的低副作用连接探测；复用 workspace `reqwest 0.12`，使用 loopback transport 测试成功、status failure、超时与脱敏错误，禁止真实外部请求

## 4. Agent definition 管理后端

- [x] 4.1 在独立 DTO 与 error 模块中定义包含 author-supplied `agent_version` 的 Agent definition create/view/update、分页 envelope、violations 与 blocker 响应；复用 `AgentName`、`AgentVersionTag`、`ModelId`、`ToolName` 等强类型
- [x] 4.2 在 `StudioStore` 实现 Agent definition 的分页列表、读取、创建、完整替换和删除，写入经 PostgreSQL transaction 原子完成
- [x] 4.3 实现 name/agent_version/model/model parameters/tools/prompt/unknown fields 校验，并返回可映射到表单字段的 400/409/422 错误；更新必须提供不同于当前值的新 author tag
- [x] 4.4 实现 POST 名称冲突 409、PUT/DELETE `If-Match` 过期 412；删除 definition 不删除 runtime Agent、Session、history 或 event
- [x] 4.5 增加 `/v1/agent-definitions`、`/{agent_name}` 与只读 `/v1/tools` REST handlers、utoipa path/schema、Location/ETag headers 和统一安全错误映射；tools 只投影 host 真实可注册目录，不提供管理写入
- [x] 4.6 保持 `/v1/agent-templates` 向后兼容并从最新 definitions/catalog 投影；测试现有对话端 Agent template 选择不回归

## 5. Provider 与 Model 管理后端

- [x] 5.1 定义 Provider/Model create/view/update DTO：Provider enum 仅 `openai|deepseek`，读取只暴露 `credential_configured`，Model 返回 canonical ModelId 与 parameter schema
- [x] 5.2 实现 Provider 分页列表、读取、创建、credential 可选替换和删除；拒绝自定义 base URL、未知 provider 与重复资源
- [x] 5.3 实现 Provider 下 immutable Model identity 的分页列表、读取、创建和删除，不提供 Model PUT；OpenAI 接受合法 model name，DeepSeek 只接受现有 adapter 支持的枚举，改变 identity 必须删除后重建
- [x] 5.4 实现 Agent definitions 对 Model/Provider 的引用检查与结构化 blocker 列表；不维护独立全局 default Model。Provider 有 Agent 引用时禁止 cascade/migrate Agent，无引用时在同一 transaction 删除 owned Models、credential 与 Provider
- [x] 5.5 实现 Provider/Model GET 强 ETag、Provider PUT 与 Provider/Model DELETE `If-Match`，保证 Provider/Model/credential 与引用检查在 PostgreSQL transaction 中提交
- [x] 5.6 增加 `/v1/providers`、Provider model 子资源与 `POST /test` handlers、utoipa path/schema、分页/状态码/错误映射
- [x] 5.7 仅在 `management_enabled` 且 loopback 时注册全部 management routes（含 Provider test 与 `GET /v1/tools`）及对应 OpenAPI fragment，并扩展 CORS methods 为实际使用的 GET/POST/PUT/DELETE；flag 不影响 Studio DB、readiness 或 runtime 数据源
- [x] 5.8 增加 API 集成测试：Agent/Provider management 与 Model list/create/read/delete、分页/排序、ETag 412、引用 409、secret 永不返回、test 脱敏、catalog 写失败回滚、重启恢复和 OpenAPI 路径完整性

## 6. 前端 API 与管理状态

- [x] 6.1 扩展 `lib/stratum/api.ts` 的 typed client：带 `agent_version` 的 Agent definitions、Providers、immutable Models、Provider test、tools catalog、pagination、ETag/If-Match、violations/blockers，并保持既有 conversation client 行为
- [x] 6.2 建立 `features/studio-management/` 类型与纯转换函数：API DTO ↔ Agent TOML draft、Provider 非 secret raw view、Model schema view
- [x] 6.3 实现管理表单 reducer/state machine：loaded、dirty、saving、invalid、conflict、testing；保存成功只确认对应 response revision
- [x] 6.4 实现 JSON Schema 到结构化 Model parameter controls 的最小解析层，复用现有 Thinking schema 逻辑且不硬编码等级；未知 schema 形状安全退回 raw parameters
- [x] 6.5 实现 dirty 路由离开/刷新提醒、412 保留 draft、409 blockers、字段 violations 与 safe retry；无 dirty 状态不得阻断

## 7. 全局浅色视觉重设计

- [x] 7.1 更新 `app/globals.css` light semantic tokens：暖纸 `#F7F4EE`、暖白 `#FAF8F3`、炭黑 `#2E2D2A/#383735`、muted well `#F0ECE3`、border `#E7E2DA`、sage `#9EB6A6`、ring `#7378D8`、destructive `#B3462F` 与可访问 muted foreground；dark tokens 保持既有语义
- [x] 7.2 在 Stratum 自有 chrome/使用方实现双主题全局导航：light 实色暖表面和轻暖阴影、dark 维持限定玻璃；新增 Studio 入口，并仅按已确认边界为受保护的 `components/react-bits/SiteNav` 增加窄 `actions` slot
- [x] 7.3 调整自有 PromptInput、conversation rail、popover/卡片 wrapper 的 light 形态，禁用 BorderGlow、backdrop blur、黑色阴影和装饰性入场；dark 行为不回归
- [x] 7.4 为 light/dark 的 focus、selected、hover、disabled、error、success 和 skeleton 状态做 WCAG 对比度与不只靠颜色的校准

## 8. Studio 仪表盘

- [x] 8.1 实现 `/studio` 薄路由与页面标题、搜索、新建 Agent；最右设置按钮只存在于全局 product navigation，不在 Studio header 重复；不得出现 Agents 页签、解释性 Agents 区块或资源配置一级入口
- [x] 8.2 实现真实 Agent definition card grid：名称、Provider/Model、tool 数量、更新时间和编辑入口；不得显示 Prompt 摘要、健康灯或假监控数据
- [x] 8.3 实现 Agent 列表分页、搜索、同形 skeleton、请求失败重试、真实空态与搜索无结果；保持可恢复的查询参数
- [x] 8.4 实现桌面/平板/移动布局与键盘焦点顺序；为未来真实统计模块保留自然的版面顺序，但不创建 slot abstraction 或占位组件

## 9. Agent 编辑器

- [x] 9.1 实现 `/studio/agents/new` 与 `/studio/agents/[agent_name]` 全页路由、加载/404/错误状态和返回仪表盘行为
- [x] 9.2 实现结构化 Agent 表单：创建后不可改的 name、每次 create/update 必传且更新时必须变化的 `agent_version`、Model、schema 参数、来自 `/v1/tools` 的 tools 与 system prompt，字段错误就近展示
- [x] 9.3 实现包含 `agent_version` 的 Agent raw TOML 次级视图：canonical encode、解析成功才回填 draft、解析失败保留文本和错误位置，且 raw 不允许改名
- [x] 9.4 实现保存/冲突/删除确认：ETag 更新、dirty 清除、412 保留本地 draft、删除只说明 definition 影响而不暗示历史被删除

## 10. Settings、Provider 与 Model 编辑

- [x] 10.1 实现全局 product navigation 最右设置图标到 `/studio/settings/providers` 的路由，以及 Provider/Model 二级切换的滑动 selected indicator；Studio page header 不重复设置动作，使用 CSS/GSAP 且支持 reduced motion
- [x] 10.2 实现 `/studio/settings/providers` 列表与 `/new`、`/[provider]` 全页表单：受支持 kind、credential configured、secret 单向替换、models count 和 ETag
- [x] 10.3 实现 Provider test 的按钮内联 pending/success/sanitized failure，保持按钮布局稳定且刷新后不伪装为持续健康状态
- [x] 10.4 实现 Provider raw config 次级视图，确保 DOM、序列化文本、错误和客户端状态中都没有已存 secret
- [x] 10.5 实现 `/studio/settings/models` 列表与 `/new`、`/[model_id]` 全页 surface，展示 immutable Provider/model name 与只读 parameter schema；详情不提供保存/PUT，identity 变化通过删除后重建
- [x] 10.6 实现 Provider/Model 删除确认、仅来自 Agent definition 的 blocker 列表、412 conflict 和移动端列表→详情下钻；Provider 无 Agent 引用时明确删除 owned Models 与 credential

## 11. 验证与设计 QA

- [x] 11.1 运行 Rust 单元/集成测试、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo test --workspace --all-targets`
- [x] 11.2 运行 `pnpm lint`、`pnpm typecheck` 与 `pnpm build`，修复所有本 change 引入的问题
- [x] 11.3 在真实本地 API 上走查 Agent/Provider/Model management 主流程、secret 替换、Provider test、ETag 412、引用 409、未保存提醒、错误恢复与重启持久化；不得使用产品 mock 数据
- [x] 11.4 用浏览器分别检查 light/dark 的 desktop 与 mobile：Studio、Settings、Agent 编辑、对话与白板；验证 light 无 glass/glow/WebGL、dark 无视觉回归、焦点/触控/中英文溢出正常
- [x] 11.5 按 Impeccable 只在 UI 完成后运行一次 mechanical detector，并由独立 finish reviewer 对照原请求、surface brief、DESIGN.md 与参考项目取舍审查；修复全部 material findings
- [x] 11.6 constitution-review：对照根 CONSTITUTION.md 务必派发子代理分条款审查本 change 完整 diff，修复全部 red-flag 与 violation
- [x] 11.7 更新相关 crate 与 `stratum-web/AGENTS.md` 归档最终实现约定，并提醒用户在 PR 合入前确认归档内容

## 12. DB-only 收敛

- [x] 12.1 删除 Provider/Model/credential 与 Agent definition 的 boot config/template seed；空 Studio catalog 必须合法启动且保持为空
- [x] 12.2 将 Studio database URL 改为 API host 必需配置，解耦 `management_enabled`：false 只隐藏 management routes 与 OpenAPI fragment，Studio DB、readiness 与 runtime 始终必需
- [x] 12.3 删除 `[llm].openai`、`[llm].deepseek`、环境 API key 与 transport 配置对 runtime 的影响；endpoint/timeout 固化为受信任 adapter policy
- [x] 12.4 保持 `/v1/models`、`/v1/agent-templates` 兼容投影只来自 Studio DB；删除 production manager 热 cache，每次新 LLM work / Turn 从 DB 组装 snapshot，只有 in-flight Turn pin Provider `Arc`
- [x] 12.5 更新 config example、Docker/local stack、CONTEXT.md、ADR 与相关 crate AGENTS.md，明确 Studio DB 是唯一 truth 与 breaking migration/rollback 方式
- [x] 12.6 增加真实 Studio PostgreSQL 回归：空库、已有 catalog、management disabled 与 OpenAPI gate、Studio readiness、config 字段拒绝、DB 不可用 fail closed、每次 work 的 DB snapshot、Provider owned-row cascade/Agent blocker、`/v1/models` DB 投影、重启持久化与 secret 不泄露
- [x] 12.7 运行 Rust fmt/clippy/workspace tests、前端 lint/typecheck/test/build与真实本地 API/浏览器 smoke；重启后验证前端只命中单一 DB-backed API
- [x] 12.8 再次派发 constitution-review 子代理审查本轮完整 diff，并修复全部 red-flag 与 violation

## 13. 归档准备

- [x] 13.1 运行 `openspec validate --all --strict`，确认本 change 不产生失效或冲突 delta
- [ ] 13.2 确认所有任务真实完成且验证证据齐全后，执行 `/opsx:archive`
