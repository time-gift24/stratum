## ADDED Requirements

### Requirement: Agent-first Studio 仪表盘
Stratum Web SHALL 在 `/studio` 提供面向开发者和管理员的 Agent-first 仪表盘，并只呈现管理 API 能证明的 Agent definition 数据。

#### Scenario: 查看 Studio 首屏
- **WHEN** 用户访问 `/studio` 且存在 Agent definitions
- **THEN** 页面必须直接显示可搜索的 Agent 卡片网格以及新建动作，不得增加 “Agents” 一级页签、解释性区块、Prompt 摘要、伪造指标、健康灯或空监控面板

#### Scenario: 查看 Agent 卡片
- **WHEN** 一个 Agent definition 被渲染为卡片
- **THEN** 卡片必须显示真实的名称、Provider、Model、tool 数量和更新时间，并提供进入编辑器的明确操作

#### Scenario: 没有 Agent definition
- **WHEN** Agent definition 列表为空
- **THEN** 页面必须显示说明真实空状态的简洁空态和唯一的新建 Agent 主行动，不得注入示例 Agent

#### Scenario: 后续监控尚未实现
- **WHEN** 当前版本没有 Agent 统计或监控 API
- **THEN** Studio 不得渲染示例图表、零值指标或“即将推出”占位模块

### Requirement: 设置入口与信息架构
Studio SHALL 将 Provider 和 Model 管理放在 header 最右侧设置图标之后，而不是仪表盘一级页签或全局资源配置入口。

#### Scenario: 打开设置
- **WHEN** 用户激活 Studio header 最右侧、具有可访问名称的设置图标
- **THEN** 应用必须进入 `/studio/settings/providers` 并在 Settings surface 内提供 Provider / Model 二级导航

#### Scenario: 返回仪表盘
- **WHEN** 用户从 Settings 返回 Studio
- **THEN** 应用必须恢复仪表盘路由和此前可恢复的搜索/分页上下文

#### Scenario: 移动端设置
- **WHEN** 用户在窄屏打开 Provider 或 Model
- **THEN** 应用必须使用列表到全页详情的下钻，不得把主编辑流程塞入被裁切的 popover 或窄 drawer

### Requirement: 结构化管理表单
Studio SHALL 以结构化表单作为 Agent、Provider、Model 的默认编辑体验，并提供受约束的 raw config 辅助视图。

#### Scenario: 编辑 Agent definition
- **WHEN** 用户创建或编辑 Agent definition
- **THEN** 表单必须提供名称、Model、schema 驱动 model parameters、tools 和 system prompt，并在名称、引用或参数无效时显示字段级错误

#### Scenario: 使用 raw Agent config
- **WHEN** 用户切换到 Agent raw config
- **THEN** UI 必须编辑 canonical TOML，只有解析与校验成功后才同步结构化 draft，失败时保留 raw 文本并定位错误

#### Scenario: 查看 Provider raw config
- **WHEN** 用户查看 Provider raw config
- **THEN** UI 不得显示、推断或可编辑已有 secret；API key 替换必须留在独立 secret 字段中

#### Scenario: 查看 Model schema
- **WHEN** 用户查看 Model 的高级信息
- **THEN** UI 必须显示服务端返回的 parameter schema 且不得允许客户端改变 adapter 声明的 schema

### Requirement: 编辑状态与安全反馈
Studio MUST 对加载、dirty、保存、校验、并发冲突、连接测试和删除结果提供真实且就近的反馈。

#### Scenario: 离开未保存表单
- **WHEN** 用户在表单 dirty 时导航离开、关闭或刷新页面
- **THEN** UI 必须提示存在未保存更改；无更改时不得制造阻断

#### Scenario: 保存成功
- **WHEN** 管理 API 接受更新并返回新 representation 与 ETag
- **THEN** UI 必须更新本地 acknowledged 状态、清除 dirty 标记，并说明 definition/provider/model 变更只影响后续使用该资源的 Agent

#### Scenario: revision 冲突
- **WHEN** API 返回 412 revision conflict
- **THEN** UI 必须保留用户 draft，提示资源已在别处变更，并让用户显式重新加载，不得静默覆盖或自动重试写入

#### Scenario: 删除被引用资源
- **WHEN** API 返回 409 resource conflict 与 blocker 列表
- **THEN** UI 必须展示阻止删除的 Agent definitions 或默认 Model 引用，并保持资源不变

#### Scenario: 测试 Provider 连接
- **WHEN** 用户主动测试 Provider
- **THEN** 按钮必须就地呈现 pending、success 或 sanitized failure；结果只代表本次测试，刷新后不得显示为持续健康状态

### Requirement: 响应式、可访问与本地化
Studio SHALL 支持键盘、屏幕阅读器、中文/英文和窄屏操作，并在减少动态效果时保持完整状态。

#### Scenario: 键盘操作仪表盘
- **WHEN** 用户只使用键盘浏览卡片、搜索、新建和设置
- **THEN** 所有动作必须具有可见焦点、合理 tab 顺序、可本地化名称和正确的路由焦点落点

#### Scenario: 减少动态效果
- **WHEN** 用户启用 `prefers-reduced-motion: reduce`
- **THEN** 卡片反馈、设置选中底片和保存状态切换必须立即到达最终状态且不得隐藏内容

#### Scenario: 长名称和多工具
- **WHEN** Agent/Model 名称接近允许上限或 tools 数量很多
- **THEN** 卡片与表单必须保持关键操作可见，并通过换行、截断加完整名称辅助文本或可滚动选择器处理溢出

### Requirement: 真实列表状态
Studio SHALL 为 Agent、Provider 和 Model 列表提供真实的加载、错误、空态、搜索无结果与分页行为。

#### Scenario: 列表加载
- **WHEN** 列表请求尚未完成
- **THEN** UI 必须使用与最终布局同形的 skeleton，不得以全屏 spinner 或示例内容替代

#### Scenario: 列表请求失败
- **WHEN** 管理 API 请求失败
- **THEN** UI 必须显示安全错误和重试操作，同时保留仍然有效的已加载内容

#### Scenario: 搜索无结果
- **WHEN** 搜索条件没有匹配资源
- **THEN** UI 必须区分“无匹配结果”和“尚未创建资源”，并提供清除筛选操作
