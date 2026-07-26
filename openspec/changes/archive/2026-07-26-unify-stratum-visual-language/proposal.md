## Why

Stratum Web 在多轮视觉探索中先后形成 Lovable、Tavily 工作台和画布式科技界面等互相冲突的方案。最终产品已经收敛为面向使用者的单一 Agent OS 对话界面，但尚未归档的 OpenSpec 仍描述概览页、隆中对路由、固定侧栏、就绪状态和旧配色。若直接归档，这些失效约束会进入主规格。

本变更将最终实现、`PRODUCT.md`、`stratum-web/DESIGN.md` 和 `stratum-web/AGENTS.md` 统一为一个权威规格，并将被推翻的探索方案仅作为历史保留。

## What Changes

- 将当前面向用户的产品范围限定为 `/chat`；根路由 `/` 直接进入该页面，不保留概览页或 `/longzhong`。
- 明确画布是未来独立能力，不得以节点、连接线、参数检查器或点阵工作面提前进入对话页。
- 采用根 Layout 中的居中全局导航，以及由具体页面以覆盖层方式渲染的垂直导航；页面导航不参与核心内容宽度计算。
- 将 Composer 作为新对话的唯一视觉重心，并保留真实 Agent、模型、思考、发送、取消、重连、审批、历史和恢复行为。
- 以标准 shadcn 语义 Token 作为唯一颜色来源；`app.css` 只负责系统级 Token 与基础规则，组件样式遵循 Tailwind utility-first。
- 采用石墨暗色表面、`#78ED9D` 品牌主色、克制信号色和限定范围的玻璃材质，并将最终视觉规则归档到 `stratum-web/DESIGN.md`。
- 保持可复用组件所有权边界，优先在 Stratum 使用方通过 props、Token、utilities 和包装层适配。

## Capabilities

### 新增能力

- `frontend-visual-system`：定义 Stratum 当前 Agent OS 对话产品的路由范围、导航架构、Token 契约、组件边界、视觉材质、真实运行行为和可访问性要求。

### 修改能力

无。当前 `openspec/specs/` 中尚无对应的已归档能力。

## Impact

- 影响 `PRODUCT.md`、`stratum-web/DESIGN.md`、`stratum-web/AGENTS.md`、全局前端 Token、根 Layout、聊天工作区、历史浮层、导航组件、组件校准页和相关本地化文案。
- 不修改后端 API、事件协议、Agent Runtime、数据模型或既有表单字段语义。
- `adopt-tavily-product-workbench` 已被本变更取代，只进行历史归档，不把其 delta specs 合并到主规格。
