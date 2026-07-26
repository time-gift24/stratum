## 1. 产品与文档收敛

- [x] 1.1 将当前产品范围收敛为 `/chat`，根路由直接进入对话，并移除概览页与 `/longzhong` 的产品约束。
- [x] 1.2 更新 `PRODUCT.md`，归档 Agent OS 定位、真实状态原则、渐进式透明和未来画布边界。
- [x] 1.3 更新 `stratum-web/DESIGN.md`，归档石墨科技方向、`#78ED9D` 主色、标准 Token、导航、Composer、玻璃材质与动效规则。
- [x] 1.4 更新 `stratum-web/AGENTS.md`，将 utility-first、组件所有权、路由范围和验证要求设为前端实现约束。

## 2. 路由与导航架构

- [x] 2.1 在根 Layout 中提供唯一的居中全局导航，并保证展开时宽度稳定、上下无硬边界。
- [x] 2.2 由具体页面渲染覆盖式垂直导航，使其不占据聊天与 Composer 的布局宽度。
- [x] 2.3 将聊天页导航收敛为唯一的新建对话入口与按需历史入口，不展示概览、就绪状态或未来画布能力。
- [x] 2.4 仅在开发模式注册 `/component-gallery`，用于检查视觉系统和复用组件。

## 3. 对话工作区

- [x] 3.1 将新对话首屏收敛为单一 Composer 视觉重心，不增加标题、副标题、伪参数或状态卡片。
- [x] 3.2 保留真实 Agent、模型、思考、发送、取消、重连、审批、历史和恢复行为。
- [x] 3.3 将历史记录实现为按需浮层，保留焦点返回、Escape、失效引用和会话恢复行为。
- [x] 3.4 保持文档滚动、底部安全空间、自动跟随与主动暂停后的滚动恢复。

## 4. 视觉系统与组件实现

- [x] 4.1 将 `app.css` 收敛为标准 shadcn Token、Tailwind theme 映射、字体和基础规则，不承载页面实现。
- [x] 4.2 使用 Tailwind utilities 和 Stratum 自有组件实现布局、玻璃材质、状态与响应式行为。
- [x] 4.3 引入并包装 BorderGlow，用于 Composer 激活反馈，同时保持其现有交互和视觉边界。
- [x] 4.4 保持受保护复用组件的所有权边界，仅在用户明确授权的范围内修改源码。
- [x] 4.5 对消息列表、配置菜单与导航数据进行无视觉变化的 React 重渲染优化，并删除不可达的导航样式分支。

## 5. 验证与归档准备

- [x] 5.1 运行格式化、`pnpm test`、`pnpm typecheck` 和 `pnpm build`。
- [x] 5.2 验证 `/chat` 与 `/component-gallery` 的桌面和窄屏布局、导航展开、控制台错误和关键计算样式。
- [x] 5.3 使用 Tailwind CSS 与 Vercel React Best Practices 完成只读审查，并整改不改变视觉效果的高价值问题。
- [x] 5.4 确认最终实现约定已进入 `PRODUCT.md`、`stratum-web/DESIGN.md` 与 `stratum-web/AGENTS.md`，且 OpenSpec 工件与最终实现一致。
