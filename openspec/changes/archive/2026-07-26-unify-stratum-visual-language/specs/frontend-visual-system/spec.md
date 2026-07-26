## ADDED Requirements

### Requirement: 权威产品与设计文档
Stratum Web MUST（必须）以 `PRODUCT.md`、`stratum-web/DESIGN.md` 和 `stratum-web/AGENTS.md` 分别作为产品范围、视觉系统和前端实现约束的权威来源。

#### Scenario: 后续前端变更
- **WHEN** 开发者或 Agent 修改 Stratum 前端
- **THEN** 实现必须同时满足产品范围、视觉规则和 utility-first 组件约束，不得从已归档的阶段性探索恢复失效设计

### Requirement: 当前产品路由范围
当前面向用户的 Stratum Web SHALL（必须）只提供 Agent OS 对话页面，并将未来画布保持为独立能力。

#### Scenario: 进入根路由
- **WHEN** 用户访问 `/`
- **THEN** 应用必须直接进入 `/chat`，不得显示概览页、营销 Hero 或中间工作台

#### Scenario: 查看对话页面
- **WHEN** 用户访问 `/chat`
- **THEN** 页面不得出现画布、节点、连接线、参数检查器、常驻运行面板或未来能力的占位入口

#### Scenario: 访问旧路由
- **WHEN** 用户尝试使用已移除的 `/longzhong`
- **THEN** 应用不得将其作为当前产品页面或一级导航入口

### Requirement: 开发态组件校准页
Stratum Web SHALL（必须）将 `/component-gallery` 作为开发环境中的视觉校准工具，而不是面向用户的产品页面。

#### Scenario: 开发模式
- **WHEN** 应用以开发模式运行
- **THEN** `/component-gallery` 可以展示 Token、排版、导航、控件和交互状态，但不得改变 `/` 到 `/chat` 的默认产品路径

#### Scenario: 生产模式
- **WHEN** 应用构建生产路由
- **THEN** `/component-gallery` 不得注册为可公开访问的产品路由

### Requirement: 全局与页面导航分层
Stratum Web MUST（必须）由根 Layout 渲染唯一的居中全局导航，并由具体页面按需渲染覆盖式垂直导航。

#### Scenario: 全局导航展开
- **WHEN** 用户通过 hover、focus 或 click 展开全局导航
- **THEN** 导航必须保持固定宽度，只改变高度，不得产生横向跳动、上下硬分割线或按钮位移

#### Scenario: 页面垂直导航
- **WHEN** `/chat` 渲染页面动作
- **THEN** 垂直导航必须作为视口覆盖层存在，不参与消息列或 Composer 的宽度计算，并且只提供唯一的新建对话入口与按需历史入口

#### Scenario: 窄屏导航
- **WHEN** 视口不足以显示桌面导航形态
- **THEN** 全局导航与页面导航必须保持至少 44px 的主要触控目标，且不得遮挡 Composer 的关键操作

### Requirement: 单一 Composer 视觉重心
新对话状态 SHALL（必须）将 Composer 作为唯一的主要视觉入口，同时保留真实配置和运行操作。

#### Scenario: 新建对话
- **WHEN** 当前没有活动会话
- **THEN** 页面必须以居中的 Composer 作为主要内容，不得增加欢迎标题、解释副标题、配置节点、伪参数或就绪状态

#### Scenario: Composer 配置
- **WHEN** 用户配置对话
- **THEN** Agent、模型与思考设置必须只显示在 Composer 工具区；只有一个 Agent 时不得重复显示 Agent 选择器

#### Scenario: 活动对话
- **WHEN** 会话已经创建
- **THEN** Composer 必须停靠在安全的底部位置，消息使用文档滚动，并为最后一条消息、审批和滚动恢复保留完整空间

### Requirement: 真实运行事实
对话界面 MUST（必须）只呈现 API、本地持久化和当前执行状态能够证明的事实。

#### Scenario: Agent 执行
- **WHEN** Agent 发送消息、推理、调用工具、等待审批、取消、失败、重连或恢复
- **THEN** 界面必须保持对应真实行为，不得用静态演示内容、虚构状态或装饰性遥测替代

#### Scenario: 资源状态
- **WHEN** 模型或模板正在加载、为空或请求失败
- **THEN** 界面只应在相关操作位置表达真实反馈，不得向用户展示全局“就绪/未就绪”状态

#### Scenario: 历史会话
- **WHEN** 用户打开历史浮层
- **THEN** 只能显示本地持久化中的真实会话，并支持关闭、焦点返回、失效引用处理和恢复

### Requirement: 标准 Token 与 utility-first 实现
前端 MUST（必须）通过 `app.css` 中的标准 shadcn 语义 Token 和组件内 Tailwind utilities 实现产品视觉。

#### Scenario: 定义全局样式
- **WHEN** 修改 `app/app.css`
- **THEN** 文件只能包含依赖导入、标准语义 Token、Tailwind theme 映射、字体、基础元素和全局无障碍规则，不得包含具体聊天、导航、弹窗、卡片或路由实现

#### Scenario: 实现组件样式
- **WHEN** 组件需要布局、间距、排版、颜色、状态或响应式行为
- **THEN** 实现必须优先使用 Tailwind v4 utilities，并通过 React 组件边界复用结构和行为，不得用 `@apply` 或全局业务类回退为传统 CSS

#### Scenario: 消费颜色
- **WHEN** 业务组件需要颜色、透明表面或阴影
- **THEN** 必须消费标准 `background`、`foreground`、`card`、`popover`、`primary`、`secondary`、`muted`、`accent`、`destructive`、`border`、`input`、`ring`、`chart-*` 或 `sidebar-*` Token，并从这些 Token 推导透明度和阴影

### Requirement: 石墨科技视觉与限定玻璃材质
Stratum Web SHALL（必须）使用近黑石墨画布、清晰浅色文字、`#78ED9D` 品牌主色和职责稳定的信号色，并限制玻璃材质的使用范围。

#### Scenario: 产品主色
- **WHEN** 渲染品牌强调、主要行动、选择或焦点反馈
- **THEN** 必须使用映射到 `primary` 的 `#78ED9D`，黄色、蓝色、洋红和红色只承担文档规定的辅助或语义职责

#### Scenario: 玻璃覆盖层
- **WHEN** 渲染全局导航、Composer 或浮层
- **THEN** 可以使用半透明语义表面、方向性渐变、背景模糊和具有垂直位移的柔和阴影，不得依赖可见描边或零偏移霓虹光晕

#### Scenario: 普通内容
- **WHEN** 渲染消息、表格或普通内容分组
- **THEN** 内容必须保持平面和清晰层级，不得构建玻璃卡片墙、随机渐变或无信息目的的装饰表面

### Requirement: 组件所有权边界
Stratum Web MUST（必须）保持产品组件与外部或复用组件之间的所有权边界。

#### Scenario: 适配复用组件
- **WHEN** `app/components/ui/*`、`app/components/react-bits/*` 或 `app/components/ai-elements/*` 需要主题、尺寸或行为适配
- **THEN** 必须优先在 `app/components/stratum/*` 或具体使用方通过 props、CSS 变量、Token、utilities 或包装层实现

#### Scenario: 修改复用源码
- **WHEN** 必须直接修改受保护组件内部实现
- **THEN** 开发者必须先说明原因并获得用户明确授权，同时保持组件 API 与标准 Token 契约稳定

### Requirement: 有目的且可访问的交互
Stratum Web SHALL（必须）保证核心流程支持键盘、屏幕阅读器、中英文、触控和减少动态效果。

#### Scenario: 键盘与屏幕阅读器
- **WHEN** 用户仅使用键盘或辅助技术操作导航、Composer、历史、审批和配置菜单
- **THEN** 控件必须具有可本地化名称、清晰焦点、合理顺序和正确的焦点返回

#### Scenario: 减少动态效果
- **WHEN** 用户启用 `prefers-reduced-motion: reduce`
- **THEN** 路由、导航、浮层和反馈动效必须立即或近乎立即到达完整最终状态，不得隐藏内容或状态

#### Scenario: 动效实现
- **WHEN** 实现悬停、焦点、开合、Dock 缩放或一次性编排
- **THEN** CSS、Motion 与 GSAP 必须分别用于短反馈、空间变化和必要编排，不得添加永久循环或纯装饰动画
