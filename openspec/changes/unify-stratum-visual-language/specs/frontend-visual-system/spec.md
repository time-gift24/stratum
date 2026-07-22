## ADDED Requirements

### Requirement: Lovable 产品配色基础
Stratum Web 前端 SHALL（必须）在两种受支持的主题中，使用 Lovable 的奶油色、炭黑色、米白色、柔和灰色、边框色和焦点色作为产品表面与控件的语义基础。

#### Scenario: 浅色主题产品角色
- **WHEN** 用户在浅色主题下查看概览页或隆中对路由
- **THEN** 页面必须使用 `#f7f4ed` 作为主画布，使用 `#1c1c1c` 作为主文本与主要操作，使用 `#fcfbf8` 作为操作文本或高对比强调，使用 `#5f5f5d` 作为弱化文本，并使用 `#eceae4` 作为静态边框

#### Scenario: 暗色主题产品角色
- **WHEN** 用户在暗色主题下查看任一路由
- **THEN** 页面必须反转 Lovable 奶油色与炭黑色角色，并从其透明度阶梯派生表面、弱化文本和边框，不得重新引入原有蓝灰色或紫灰色产品表面

#### Scenario: 键盘焦点
- **WHEN** 键盘用户聚焦可交互控件
- **THEN** 控件必须显示基于 Lovable `rgba(59, 130, 246, 0.5)` 焦点色的清晰焦点状态，并与所在表面保持足够对比度

### Requirement: 配色层保持分离
前端 MUST（必须）区分 Lovable 产品色、语义状态色和保留的 Stratum 特效色，使每种颜色只承担一种明确职责。

#### Scenario: 普通产品界面
- **WHEN** 前端渲染导航、输入区、历史记录、普通卡片、页面背景或正文
- **THEN** 这些元素必须使用 Lovable 产品 token，而不是 Stratum 特效色

#### Scenario: 语义状态
- **WHEN** 前端渲染成功、警告、危险、审批或错误状态
- **THEN** 必须使用与 Lovable 产品配色可区分且满足可访问性要求的语义色

#### Scenario: 保留特效配色
- **WHEN** 本次变更实施完成
- **THEN** 现有 Stratum 多色配色必须继续保留给品牌标识和未来按需启用的特效，但本次变更不得引入新的装饰性特效

### Requirement: Lovable 排版参数
前端 SHALL（必须）采用 Lovable 字号、400 与 600 字重层级、行高、字距和响应式展示字阶，同时保留现有 Noto Sans 与 Nunito Sans 字体资源。

#### Scenario: 桌面端展示层级
- **WHEN** 概览页 Hero 在桌面视口渲染
- **THEN** 展示标题必须使用 60px 字号、600 字重、1.00-1.10 行高和约 -1.5px 字距

#### Scenario: 响应式展示层级
- **WHEN** 概览页 Hero 跨越平板端与移动端断点
- **THEN** 展示标题必须从 60px 缩放到 48px，再缩放到 36px，同时保持 Lovable 的比例字距和可读行高

#### Scenario: 产品文本角色
- **WHEN** 前端渲染章节标题、次级标题、卡片标题、大号正文、正文、操作、说明文字或紧凑控件
- **THEN** 必须分别使用文档规定的 Lovable 48px、36px、20px、18px、16px 与 14px 角色参数，并使用 400 或 600 字重及对应行高

#### Scenario: 多语言排版
- **WHEN** 界面在中文与英文之间切换
- **THEN** 两种语言必须保持相同的语义层级，不得出现字形裁切、桌面端操作文案意外换行或低于 16px 的正文字号

### Requirement: 统一的表面与形状语言
概览页与隆中对路由 SHALL（必须）为同类界面元素使用一致的边框主导纵深模型和明确圆角职责。

#### Scenario: 共享导航语言
- **WHEN** 用户在概览页与隆中对路由之间切换
- **THEN** 即使导航响应式宽度发生变化，也必须保持相同的 Lovable 表面、边框、排版和形状处理

#### Scenario: 容器纵深
- **WHEN** 前端渲染普通内容组、输入区、历史抽屉或浮层
- **THEN** 普通内容组必须保持平面，标准容器必须使用静态边框且不使用厚重投影，只有浮层可以使用克制的纵深

#### Scenario: 圆角职责
- **WHEN** 组件应用样式
- **THEN** 控件必须使用约 6px 圆角，标准容器必须使用 12px 圆角，大型浮层可以使用 16px 圆角，全圆角仅用于圆形操作、开关或行为上确有必要的控件

### Requirement: 有目的且可访问的动效
Stratum 自有前端动效 SHALL（必须）用于表达交互或状态变化，保持简短，并提供减少动态效果的结果。

#### Scenario: 普通过渡
- **WHEN** 用户对 Stratum 自有界面元素进行悬停、聚焦、选择、打开、关闭或导航操作
- **THEN** 视觉反馈通常必须在 150-250ms 内完成，并尽可能只动画 transform 或 opacity

#### Scenario: 减少动态效果
- **WHEN** 用户启用 `prefers-reduced-motion: reduce`
- **THEN** 路由、抽屉、选择和工作区过渡必须立即或近乎立即完成，且不得隐藏内容或弱化状态变化

#### Scenario: 装饰性特效
- **WHEN** 统一视觉系统完成应用
- **THEN** 本次变更不得新增环境式、永久循环、滚动驱动或装饰性动画

### Requirement: 保留现有产品行为
视觉迁移 MUST（必须）保留当前信息架构、运行时数据行为、可访问性交互和隆中对硬性布局约束。

#### Scenario: 隆中对布局
- **WHEN** 隆中对路由在任一受支持视口渲染
- **THEN** 必须保留居中单列对话、文档滚动、固定输入区，并将历史记录保留为可切换浮层而不是永久侧栏

#### Scenario: 运行时内容
- **WHEN** 渲染对话消息、推理、工具、审批或历史记录
- **THEN** 视觉迁移后仍必须只显示后端或本地持久化提供的事实，不得伪造工具状态、审批说明、对话结果或产品预览数据

#### Scenario: 受保护组件适配
- **WHEN** `app/components/ui`、`app/components/react-bits` 或 `app/components/ai-elements` 下的可复用组件需要视觉适配
- **THEN** 实现必须使用语义 token、props、Stratum 自有包装层或使用方样式，不得修改受保护组件的内部实现

### Requirement: 权威设计文档
仓库 SHALL（必须）将生成的 Lovable DESIGN.md 保留为参考，并将 Stratum 自有 DESIGN.md 作为产品特有视觉与行为约束的权威来源。

#### Scenario: 安装 Lovable 参考文档
- **WHEN** 在已存在 `stratum-web/DESIGN.md` 的情况下，从 `stratum-web` 运行 `npx getdesign@latest add lovable`
- **THEN** 生成的 Lovable 文档必须独立保留，且不得覆盖 Stratum 权威设计文档

#### Scenario: 后续前端工作
- **WHEN** Agent 或开发者在本次变更后修改 Stratum 前端界面
- **THEN** 必须遵循 `stratum-web/DESIGN.md` 中已采纳的 Lovable 规则、Stratum 特有约束，以及产品配色层与特效配色层之间的分离规则
