## 背景

最终产品方向由真实实现和三份权威文档共同确定：`PRODUCT.md` 约束产品范围与语气，`stratum-web/DESIGN.md` 约束视觉系统，`stratum-web/AGENTS.md` 约束前端实现方式。此前未归档的 OpenSpec 仍保留阶段性方案，必须在归档前消除冲突。

## 目标与非目标

**目标：**

- 建立一个与当前实现一致、可以进入主 specs 的最终前端规格。
- 保持对话页单一视觉重心，同时让全局导航、页面导航和历史入口各守边界。
- 用标准语义 Token 和 utility-first 组件实现统一暗色科技材质。
- 保留真实 Agent 执行、审批、恢复、取消、历史与错误处理能力。
- 明确未来画布与当前对话产品的边界。

**非目标：**

- 恢复概览页、隆中对命名、固定产品侧栏或顶部就绪状态。
- 在对话页引入画布、节点、连接线或常驻参数面板。
- 通过伪状态、伪参数、文化主题文案或装饰小字制造产品感。
- 修改后端协议或重新实现受保护的复用组件。

## 决策

### 权威文档分工

- `PRODUCT.md` 回答产品服务谁、解决什么问题以及哪些能力当前不存在。
- `stratum-web/DESIGN.md` 记录 Token、排版、布局、材质、组件和动效规则。
- `stratum-web/AGENTS.md` 记录 utility-first、组件所有权、路由与验证约束。
- OpenSpec 主规格记录可验证的产品和工程要求，不复制大段视觉参数台账。

### 当前信息架构

根路由 `/` 重定向到 `/chat`。当前没有面向用户的概览页，也不保留 `/longzhong`。`/component-gallery` 仅在开发模式注册，用于校准 Token、排版和组件状态，不属于一级产品导航。

根 Layout 渲染唯一的 `CenteredNavigation`。具体页面自行渲染 `VerticalNavigation`，并将其作为视口覆盖层，使聊天列和 Composer 仍以完整视口居中。聊天页的垂直导航只保留唯一的新建对话入口与按需历史入口。

### Token 与 utility-first

`app/app.css` 只包含依赖导入、标准 shadcn 语义 Token、Tailwind theme 映射、字体、基础元素与全局无障碍规则。页面和组件的布局、间距、状态、响应式与视觉材质全部通过 Tailwind utilities 表达。重复的结构与行为通过 `app/components/stratum/` 组件复用，不通过全局业务 CSS 类复用。

业务组件只消费 `background`、`foreground`、`card`、`popover`、`primary`、`secondary`、`muted`、`accent`、`destructive`、`border`、`input`、`ring`、`chart-*` 和 `sidebar-*`。品牌主色为 `#78ED9D`，信号色只承担已定义的状态或辅助职责。

### 材质与层级

产品使用近黑石墨画布、低对比中性表面、白色正文和少量信号色。玻璃材质仅用于覆盖内容的全局导航、Composer 与浮层，通过半透明语义表面、方向性渐变、背景模糊和向下阴影建立纵深，不使用可见描边。普通内容区域保持平面，不构建玻璃卡片墙。

### Composer 与真实状态

新对话状态只显示 Composer，不添加欢迎标题、解释副标题、配置节点或就绪面板。Agent、模型和思考配置只存在于 Composer 工具区；只有一个 Agent 时不重复显示选择器。所有消息、工具调用、审批、错误、取消、重连和历史恢复都继续来自真实运行数据。

### 组件所有权

Stratum 产品组件位于 `app/components/stratum/`。`app/components/ui/*`、`app/components/react-bits/*` 和 `app/components/ai-elements/*` 默认视为外部或可复用源码，适配优先发生在使用方。只有获得明确授权时才直接修改其内部实现。

### 动效与可访问性

CSS 负责短促状态反馈，Motion 负责浮层和垂直导航的空间变化，GSAP 只用于真正需要编排的一次性入场。所有动效提供 reduced-motion 最终态。主要流程支持键盘、屏幕阅读器、中英文和至少 44px 的主要触控目标。

## 风险与权衡

- [玻璃与信号色可能重新滑向装饰性科技感] -> 严格限制玻璃使用范围，并让每个信号色承担稳定职责。
- [页面级导航覆盖内容] -> 导航不参与宽度计算，并在桌面与窄屏分别验证遮挡和触控行为。
- [第三方组件默认样式与 Token 契约冲突] -> 优先使用 Stratum 包装层；任何源码修改都需要单独授权。
- [历史 OpenSpec 与最终规格同时存在] -> 旧 Tavily change 使用 `--skip-specs` 归档，最终 change 正常合并到主 specs。

## 迁移与回滚

1. 将旧 Tavily change 标记为已被替代，并仅作历史归档。
2. 用本变更覆盖阶段性的 Lovable 内容，重新校验 proposal、design、specs 与 tasks。
3. 正常归档本变更，生成 `openspec/specs/frontend-visual-system/spec.md`。
4. 若需回滚，只回滚实现提交和主 spec；历史 archive 不参与运行时。

## 待确认问题

无。当前产品范围、主色、导航架构、Composer 重心和未来画布边界均已由用户确认并在实现中验证。
