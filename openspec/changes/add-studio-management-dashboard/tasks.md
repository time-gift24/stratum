## 1. 实现准备与产品文档

- [x] 1.1 按 `using-git-worktrees` 在新的 `codex/` 分支与独立 worktree 开始实现；重新读取 CONSTITUTION.md、rust-skills、PRODUCT.md、`stratum-web/PRODUCT.md` 与 `stratum-web/DESIGN.md`
- [x] 1.2 按 Impeccable 记录 `/studio` Operate surface brief：Agent-first 仪表盘、右上设置入口、无 Agents 页签/解释区/Prompt 摘要/监控占位，以及浅色 `rbp-portfolio` 取舍
- [x] 1.3 更新根 PRODUCT.md 与 `stratum-web/PRODUCT.md`：加入面向开发者/管理员的 Studio 第二界面，明确首期和非目标，不改变最终用户对话定位
- [x] 1.4 更新 `stratum-web/DESIGN.md`：归档暖色 light tokens、实色表面、scarce sage、indigo focus、warm shadows、light 禁用 glow/glass/WebGL，以及 dark 主题保留规则

## 2. 配置、secret 与持久化基础

- [x] 2.1 在 `stratum-config` 增加受 serde 严格校验的 `management_enabled`（默认 false），并拒绝非 loopback bind 启用管理面；补齐类型化错误与单测
- [x] 2.2 将 Provider credential 的内存表示迁移到 `secrecy` secret 类型，确保 Debug/Display、错误链和 tracing 不泄露；增加序列化边界测试
- [x] 2.3 在 `stratum-config` 定义最小 managed LLM catalog（default model、`openai|deepseek`、models、credentials）的 strict TOML 编解码、引用校验和 canonical encode；不引入通用 provider/plugin 抽象
- [x] 2.4 增强 `LocalFilesystem` 的 crash-consistent 原子写与目录落盘，并为 storage root 内文件/目录设置运行用户私有权限；覆盖失败保留旧文件和权限测试
- [x] 2.5 实现 `/providers/catalog.toml` 首次从 boot `[llm]` seed、后续优先读取 managed catalog 的恢复逻辑；覆盖不存在、有效、损坏、写失败与 boot config 不覆盖场景
- [x] 2.6 为 Agent definition 与 managed catalog 实现 canonical representation digest/强 ETag helper，并用固定向量测试稳定性

## 3. Provider manager 热替换

- [x] 3.1 将 `HostState` 的当前 Provider catalog/manager 改为可原子替换的具体状态，读取先 clone 快照且不在 `.await` 期间持有 std guard
- [x] 3.2 实现 candidate catalog 的完整验证与 manager 构造，再按“持久化成功后替换内存”顺序提交；失败时保留旧 catalog/manager
- [x] 3.3 保持已构造 Agent/进行中 Turn 的 `Arc<dyn LlmProvider>` 快照语义，并测试 credential/model 更新只作用于之后新建的 Agent
- [x] 3.4 为 OpenAI 与 DeepSeek 实现固定 endpoint、固定超时、低副作用的连接探测接口；使用 mock transport 测试成功、认证失败、超时与脱敏错误，禁止真实外部请求

## 4. Agent definition 管理后端

- [x] 4.1 在独立 DTO 与 error 模块中定义 Agent definition create/view/update、分页 envelope、violations 与 blocker 响应；复用 `AgentName`、`ModelId`、`ToolName` 等强类型
- [x] 4.2 在 `HostState` 实现 `/templates/{agent_name}.toml` 的分页列表、读取、创建、完整替换和删除，写入经 `stratum-filesystem` 原子完成
- [x] 4.3 实现 name/model/model parameters/tools/prompt/unknown fields 校验，并返回可映射到表单字段的 400/422 错误
- [x] 4.4 实现 POST 名称冲突 409、PUT/DELETE `If-Match` 过期 412；删除 definition 不删除 runtime Agent、Session、history 或 event
- [x] 4.5 增加 `/v1/agent-definitions` 与 `/{agent_name}` REST handlers、utoipa path/schema、Location/ETag headers 和统一安全错误映射
- [x] 4.6 保持 `/v1/agent/templates` 向后兼容并从最新 definitions/catalog 投影；测试现有对话端 Agent template 选择不回归

## 5. Provider 与 Model 管理后端

- [x] 5.1 定义 Provider/Model create/view/update DTO：Provider enum 仅 `openai|deepseek`，读取只暴露 `credential_configured`，Model 返回 canonical ModelId 与 parameter schema
- [x] 5.2 实现 Provider 分页列表、读取、创建、credential 可选替换和删除；拒绝自定义 base URL、未知 provider 与重复资源
- [x] 5.3 实现 Provider 下 Model 的分页列表、读取、创建和删除；OpenAI 接受合法 model name，DeepSeek 只接受现有 adapter 支持的枚举
- [x] 5.4 实现 default Model、Agent definitions 对 Model/Provider 的引用检查与结构化 blocker 列表；409 时禁止 cascade 或猜测迁移
- [x] 5.5 实现 Provider/Model GET 强 ETag 与 PUT/DELETE `If-Match`，保证 catalog 内 default/provider/models 在一次原子替换中提交
- [x] 5.6 增加 `/v1/providers`、Provider model 子资源与 `POST /test` handlers、utoipa path/schema、分页/状态码/错误映射
- [x] 5.7 仅在 `management_enabled` 且 loopback 时注册全部 management routes，并扩展 CORS methods 为实际使用的 GET/POST/PUT/DELETE；默认配置下验证旧 routes 不变
- [x] 5.8 增加 API 集成测试：CRUD、分页/排序、ETag 412、引用 409、secret 永不返回、test 脱敏、catalog 写失败回滚、重启恢复和 OpenAPI 路径完整性

## 6. 前端 API 与管理状态

- [x] 6.1 扩展 `lib/stratum/api.ts` 的 typed client：Agent definitions、Providers、Models、Provider test、pagination、ETag/If-Match、violations/blockers，并保持既有 conversation client 行为
- [x] 6.2 建立 `features/studio-management/` 类型与纯转换函数：API DTO ↔ Agent TOML draft、Provider 非 secret raw view、Model schema view
- [x] 6.3 实现管理表单 reducer/state machine：loaded、dirty、saving、invalid、conflict、testing；保存成功只确认对应 response revision
- [x] 6.4 实现 JSON Schema 到结构化 Model parameter controls 的最小解析层，复用现有 Thinking schema 逻辑且不硬编码等级；未知 schema 形状安全退回 raw parameters
- [x] 6.5 实现 dirty 路由离开/刷新提醒、412 保留 draft、409 blockers、字段 violations 与 safe retry；无 dirty 状态不得阻断

## 7. 全局浅色视觉重设计

- [x] 7.1 更新 `app/globals.css` light semantic tokens：暖纸 `#F7F4EE`、暖白 `#FAF8F3`、炭黑 `#2E2D2A/#383735`、muted well `#F0ECE3`、border `#E7E2DA`、sage `#9EB6A6`、ring `#7378D8`、destructive `#B3462F` 与可访问 muted foreground；dark tokens 保持既有语义
- [x] 7.2 在 Stratum 自有 chrome/使用方实现双主题全局导航：light 实色暖表面和轻暖阴影、dark 维持限定玻璃；新增 Studio 入口且不修改受保护的 `components/react-bits/*`
- [x] 7.3 调整自有 PromptInput、conversation rail、popover/卡片 wrapper 的 light 形态，禁用 BorderGlow、backdrop blur、黑色阴影和装饰性入场；dark 行为不回归
- [x] 7.4 为 light/dark 的 focus、selected、hover、disabled、error、success 和 skeleton 状态做 WCAG 对比度与不只靠颜色的校准

## 8. Studio 仪表盘

- [x] 8.1 实现 `/studio` 薄路由与 Studio header：标题、搜索、新建 Agent、最右 44px 设置按钮；不得出现 Agents 页签、解释性 Agents 区块或资源配置一级入口
- [x] 8.2 实现真实 Agent definition card grid：名称、Provider/Model、tool 数量、更新时间和编辑入口；不得显示 Prompt 摘要、健康灯或假监控数据
- [x] 8.3 实现 Agent 列表分页、搜索、同形 skeleton、请求失败重试、真实空态与搜索无结果；保持可恢复的查询参数
- [x] 8.4 实现桌面/平板/移动布局与键盘焦点顺序；为未来真实统计模块保留自然的版面顺序，但不创建 slot abstraction 或占位组件

## 9. Agent 编辑器

- [x] 9.1 实现 `/studio/agents/new` 与 `/studio/agents/[agent_name]` 全页路由、加载/404/错误状态和返回仪表盘行为
- [x] 9.2 实现结构化 Agent 表单：创建后不可改的 name、Model、schema 参数、tools 与 system prompt，字段错误就近展示
- [x] 9.3 实现 Agent raw TOML 次级视图：canonical encode、解析成功才回填 draft、解析失败保留文本和错误位置
- [x] 9.4 实现保存/冲突/删除确认：ETag 更新、dirty 清除、412 保留本地 draft、删除只说明 definition 影响而不暗示历史被删除

## 10. Settings、Provider 与 Model 编辑

- [x] 10.1 实现设置图标到 `/studio/settings/providers` 的路由，以及 Provider/Model 二级切换的滑动 selected indicator；使用 CSS/GSAP 且支持 reduced motion
- [x] 10.2 实现 `/studio/settings/providers` 列表与 `/new`、`/[provider]` 全页表单：受支持 kind、credential configured、secret 单向替换、models count 和 ETag
- [x] 10.3 实现 Provider test 的按钮内联 pending/success/sanitized failure，保持按钮布局稳定且刷新后不伪装为持续健康状态
- [x] 10.4 实现 Provider raw config 次级视图，确保 DOM、序列化文本、错误和客户端状态中都没有已存 secret
- [x] 10.5 实现 `/studio/settings/models` 列表与 `/new`、`/[model_id]` 全页表单，展示 Provider、model name 与只读 parameter schema
- [x] 10.6 实现 Provider/Model 删除确认、default/Agent definition blocker 列表、412 conflict 和移动端列表→详情下钻

## 11. 验证与设计 QA

- [x] 11.1 运行 Rust 单元/集成测试、`cargo fmt --all -- --check`、`cargo clippy --workspace --all-targets -- -D warnings` 与 `cargo test --workspace --all-targets`
- [x] 11.2 运行 `pnpm lint`、`pnpm typecheck` 与 `pnpm build`，修复所有本 change 引入的问题
- [x] 11.3 在真实本地 API 上走查 Agent/Provider/Model CRUD、secret 替换、test、ETag 412、引用 409、未保存提醒、错误恢复与重启持久化；不得使用产品 mock 数据
- [x] 11.4 用浏览器分别检查 light/dark 的 desktop 与 mobile：Studio、Settings、Agent 编辑、对话与白板；验证 light 无 glass/glow/WebGL、dark 无视觉回归、焦点/触控/中英文溢出正常
- [x] 11.5 按 Impeccable 只在 UI 完成后运行一次 mechanical detector，并由独立 finish reviewer 对照原请求、surface brief、DESIGN.md 与参考项目取舍审查；修复全部 material findings
- [x] 11.6 constitution-review：对照根 CONSTITUTION.md 务必派发子代理分条款审查本 change 完整 diff，修复全部 red-flag 与 violation
- [x] 11.7 更新相关 crate 与 `stratum-web/AGENTS.md` 归档最终实现约定，并提醒用户在 PR 合入前确认归档内容

## 12. 归档准备

- [x] 12.1 运行 `openspec validate --all --strict`，确认本 change 与进行中的 `add-ontology-list-canvas-frontend` 不产生失效或冲突 delta
- [ ] 12.2 确认所有任务真实完成且验证证据齐全后，执行 `/opsx:archive`
