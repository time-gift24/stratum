## MODIFIED Requirements

### Requirement: 当前产品路由范围
当前 Stratum Web SHALL 提供面向最终用户的对话与白板页面，以及与其边界分离、面向开发者和管理员的 Studio 管理界面。

#### Scenario: 进入根路由
- **WHEN** 用户访问 `/`
- **THEN** 应用必须直接进入 `/conversation`，不得显示概览页、营销 Hero 或 Studio 中间页

#### Scenario: 查看对话页面
- **WHEN** 用户访问 `/conversation`
- **THEN** 页面不得出现 Provider、Model、Agent definition 管理表单、统计监控占位或其他 Studio 配置概念

#### Scenario: 查看白板页面
- **WHEN** 用户访问 `/excalidraw`
- **THEN** 页面必须保持独立的沉浸编辑交互，不得承担 Studio dashboard 或资源配置职责

#### Scenario: 查看 Studio
- **WHEN** 用户访问 `/studio`
- **THEN** 应用必须呈现面向开发者/管理员的 Agent-first 仪表盘，并通过最右设置图标进入 Provider/Model 设置

#### Scenario: 访问旧路由
- **WHEN** 用户尝试使用已移除的 `/longzhong` 或 `/chat`
- **THEN** 应用不得将其作为当前产品页面或一级导航入口

### Requirement: 全局与页面导航分层
Stratum Web MUST 由根 Layout 渲染唯一的全局导航，并由具体页面按任务需要提供会话动作、沉浸控制或 Studio 设置动作。

#### Scenario: 全局导航
- **WHEN** 用户在对话、白板或 Studio 之间导航
- **THEN** 全局导航必须提供真实可用的产品入口、保持布局稳定，并使用可访问的当前页状态

#### Scenario: 对话页面垂直导航
- **WHEN** `/conversation` 渲染会话动作
- **THEN** 垂直导航必须作为视口覆盖层存在，不参与消息列或 Composer 的宽度计算，并且只提供唯一的新建对话入口与按需历史入口

#### Scenario: Studio 设置动作
- **WHEN** `/studio` 渲染 header
- **THEN** 设置图标必须位于 header 最右侧并进入 Studio Settings，不得把 Provider/Model 提升为全局或仪表盘一级页签

#### Scenario: 窄屏导航
- **WHEN** 视口不足以显示桌面导航形态
- **THEN** 全局导航与页面动作必须保持至少 44px 的主要触控目标，且不得遮挡 Composer、Agent 卡片操作或管理表单保存动作

### Requirement: 标准 Token 与 utility-first 实现
前端 MUST 通过 `app/globals.css` 中的标准 shadcn 语义 Token 和组件内 Tailwind utilities 实现双主题产品视觉。

#### Scenario: 定义全局样式
- **WHEN** 修改 `app/globals.css`
- **THEN** 文件只能包含依赖导入、标准语义 Token、Tailwind theme 映射、字体、基础元素和全局无障碍规则，不得包含具体聊天、导航、设置、卡片或路由实现

#### Scenario: 实现组件样式
- **WHEN** 组件需要布局、间距、排版、颜色、状态或响应式行为
- **THEN** 实现必须优先使用 Tailwind v4 utilities，并通过 React 组件边界复用结构和行为，不得用 `@apply` 或全局业务类回退为传统 CSS

#### Scenario: 消费颜色
- **WHEN** 业务组件需要颜色、透明表面或阴影
- **THEN** 必须消费标准 `background`、`foreground`、`card`、`popover`、`primary`、`secondary`、`muted`、`accent`、`destructive`、`border`、`input`、`ring`、`chart-*` 或 `sidebar-*` Token，并从当前主题 Token 推导透明度和阴影

### Requirement: 石墨科技视觉与限定玻璃材质
Stratum Web SHALL 在深色模式保留近黑石墨、绿色 primary 与限定玻璃材质，并在浅色模式使用来自 `rbp-portfolio` 的暖色 soft-minimalism；两个主题必须共享语义而不是共享物理色值。

#### Scenario: 浅色画布与表面
- **WHEN** 当前主题为 light
- **THEN** `background/card/foreground/primary/accent/ring` 必须分别建立暖纸画布、暖白表面、炭黑文字、炭黑主行动、稀缺鼠尾草绿选择与靛蓝焦点语义，不得使用纯白卡片或纯黑正文

#### Scenario: 浅色层级
- **WHEN** light 主题渲染导航、Composer、卡片、popover 或表单
- **THEN** 层级必须主要来自暖色 tonal steps、hairline border 与 5–12% 暖墨阴影，不得使用 backdrop blur、玻璃透明、霓虹 glow、WebGL 氛围、装饰渐变或编排式页面入场

#### Scenario: 浅色品牌与状态色
- **WHEN** light 主题表达当前选择、成功、主行动或焦点
- **THEN** 鼠尾草绿只能稀缺用于选择/成功，主行动使用炭黑，焦点使用靛蓝，并且任何状态不得只靠颜色传达

#### Scenario: 深色产品主色
- **WHEN** 当前主题为 dark 且渲染品牌强调、主要行动、选择或焦点反馈
- **THEN** 必须继续使用映射到 dark primary 的 Stratum 绿色，黄色、蓝色、洋红和红色只承担文档规定的辅助或语义职责

#### Scenario: 深色玻璃覆盖层
- **WHEN** dark 主题渲染全局导航、Composer 或浮层
- **THEN** 可以使用半透明语义表面、背景模糊和具有垂直位移的柔和阴影，不得构建玻璃卡片墙或零偏移霓虹光晕

#### Scenario: 普通内容
- **WHEN** 任一主题渲染消息、表格、Agent 卡片或普通内容分组
- **THEN** 内容必须保持平面、清晰且信息优先，不得加入无信息目的的装饰表面或 hover 演出

## ADDED Requirements

### Requirement: 浅色模式的参考取舍
Stratum Web MUST 只借鉴 `rbp-portfolio` 中适合 Operate surface 的视觉和交互规则，不得复制其作品集展示机制。

#### Scenario: 借鉴参考项目
- **WHEN** 设计浅色导航、设置切换、卡片和表单反馈
- **THEN** 可以借鉴暖色阶、低对比暖阴影、滑动选中底片与稳定 inline 状态反馈，并使用现有 CSS/GSAP 实现

#### Scenario: 排除展示机制
- **WHEN** 实现任一 Stratum 产品页面
- **THEN** 不得引入参考项目的 WebGL shader、site frame、smooth scroll、physics chips、展示型卡片 lift、衬线 UI 标签或第二套 motion library

### Requirement: 主题化组件所有权
浅色重设计 MUST 在 Stratum 自有组件和使用方完成，不得未经授权修改受保护的外部/复用组件源码。

#### Scenario: 全局导航需要双主题形态
- **WHEN** 既有复用导航无法满足实色浅色与玻璃深色的主题差异
- **THEN** 实现必须在 `components/stratum/chrome/` 或使用方建立产品导航，不得直接修改 `components/react-bits/*`

#### Scenario: Composer glow
- **WHEN** PromptInput 在 light 主题获得焦点
- **THEN** 自有组件必须使用清晰 ring/border 状态而不渲染 BorderGlow；dark 主题可以保留已有语义 glow
