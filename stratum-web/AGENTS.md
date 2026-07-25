# Stratum Web 开发约定

## 设计上下文

- 修改界面前必须阅读仓库根目录 `PRODUCT.md` 与本目录 `DESIGN.md`。
- Stratum 使用 PicGen 式暗色空间工作面，但所有内容必须来自真实 Agent OS 数据与操作。
- `app/app.css` 的标准 shadcn 语义 Token 是产品颜色唯一来源。业务组件不得写裸 RGB、Hex 或同义颜色变量；透明度、阴影和光晕从标准 Token 推导。
- UI 使用 Geist，标题使用 Outfit，数据使用 Geist Mono，中文回退使用 Noto Sans。
- 禁止用主题化文案、无功能小字或伪技术参数解释设计气质。

## 页面与壳层

- `/` 与 `/chat` 共享 `ProductShell`，URL 与一级导航标签保持稳定。
- 桌面壳层使用 56px 悬浮顶部栏和 56px 左工具轨；不得恢复 248px 管理后台侧栏。
- 概览桌面是输入、Agent、模型、输出四节点关系图，并有 320px 右侧检查器；所有值来自 API 或本地持久化事实。
- 移动端节点必须转为单列文档流，检查器排在节点之后；连接线和缩放控件隐藏，不得整体缩放桌面画布。
- 最近会话从工具轨或移动抽屉进入，聊天消息继续使用文档滚动，不新增内部消息滚动轨道。

## Prompt Input

- 新对话 Composer 使用顶部上下文、中部大输入面、底部配置工具、右侧独立发送按钮的层级。
- Agent、模型与思考配置必须保留现有真实行为；发送、取消、重连、审批和会话恢复不得因视觉改造退化。
- Composer 左侧相邻显示 Agent、Model 和 Thinking。移动端工具可以横向滚动，发送按钮始终可见。
- 视口 bottom 间距由最外层 fixed 容器表达，消息区必须预留完整 Composer 高度与安全区。

## 动效

- CSS 用于短反馈，Motion 用于抽屉与状态迁移，GSAP 用于节点和连接线空间序列。
- 不同库不得控制同一元素的同一属性。
- 所有动效必须提供 `prefers-reduced-motion` 最终态，不做装饰性循环或滚动劫持。

## 组件所有权

- Stratum 自有组件位于 `app/components/stratum/`。
- `app/components/react-bits/` 与 `app/components/ai-elements/` 不得直接修改。
- 用户已批准本轮在必要时调整 `app/components/ui/` 的 shadcn 源码；组件 API 与标准 Token 契约必须保持稳定。

## 验证与测试

- 不得在 `stratum-web` 下新增、恢复或维护前端测试文件。
- 前端变更至少运行格式化、`pnpm typecheck` 与 `pnpm build`。
- 视觉变更必须检查 `/` 与 `/chat` 的桌面/移动、中英文、键盘焦点、对比度、减少动态效果、无横向溢出、加载/空/错误/禁用/选中状态和控制台错误。
