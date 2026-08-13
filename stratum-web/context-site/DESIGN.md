# Stratum Context Site — 视觉与交互约定

## 方向合同

- **模式**：Read。它是工程现场手册，不是营销页、控制台或产品聊天界面。
- **核心命题**：先用 kernel 运行结构定位每个名词，再沿一条可追踪的运行路线解释 Stratum；恢复必须从 durable facts 推导，不靠叙事猜测。
- **视觉世界**：暗色搪瓷线路图与事件账本。绿色表示当前事实/强制不变量，蓝色表示恢复路径，琥珀色表示风险，红色表示失败，紫色表示内部事实。
- **组合语法**：核心结构图与“沿图认词”是阅读入口；纵向 runtime route 是其后的主骨架。event ledger、state machine、responsibility boundary 与 crash timeline 嵌入对应章节，不另建互相竞争的页面。
- **生成种子**：Impeccable concept seed `ca08474b`；它只决定构图偏向，不是运行时随机输入。

## 内容密度与排版

- 中文是主叙事语言；领域类型、事件、ID、symbol 与路径保留精确英文，并使用等宽字体。
- 标题和正文使用离线可用的系统中文 serif/sans 栈，代码使用共享 mono token；静态制品不内联产品完整字体。
- 先用同一张核心结构图回答术语位于哪一层、是否已耐久、是否参与恢复；再展开定义与不变量、正常路径、失败、恢复、风险和证据。不要用大段散文掩盖状态差异。
- 桌面宽度优先用于横向关系和账本；移动端把关系降为单列，但不隐藏定义、风险、恢复或证据。

## 图示

- 图示必须来自 `content/*.ts` 的 typed data 或生成器中与 typed content 同域的封闭模板。
- 默认使用语义 HTML、CSS grid/border 与少量 GSAP 状态反馈。完整 ReAct 这类多泳道、分支和恢复关系可由 Mermaid 在构建期编译为内联 SVG；不得引入远程图表脚本或浏览器运行时 DSL。
- Mermaid 源只能由 `content/*.ts` 的 typed data 或生成器中同域封闭模板推导，不能维护手写 `.mmd` 文件。构建器使用系统 Chrome/Chromium 渲染 SVG，不下载浏览器；生成制品只保留 SVG，不含 Mermaid runtime。
- 这样仍能保持单文件离线打开、继承站点 token、允许构建期校验 identity/重复 ID，并避免领域文字和 diagram source 演化成两套事实。新增关系图时优先压缩为可读的单一主图；不能把 Mermaid 当成不经编辑的信息堆场。

## 交互与动效

- 只允许导航、页签、证据展开、原生 details 与定位反馈，不提供会改变系统状态的控件。
- GSAP 只服务于首屏建立层级、surface 切换与 dock 邻近反馈；禁止自动循环、滚动劫持和装饰性视差。
- 所有动效必须在 `prefers-reduced-motion` 下立即呈现最终状态。
- 链接与 hash 必须在 `file://` 直接打开时可用；不得依赖 router 或服务器 fallback。

## 构建边界

- 站点只维护暗色阅读模式；亮色设计尚未确认，不在当前范围。
- `generate.mts` 单向读取 `app/globals.css` 的暗色语义 token 与本地 GSAP 包，然后生成根 `CONTEXT.html`；字体只使用操作系统本地栈。
- `CONTEXT.html` 必须自包含、无网络请求、无产品 API 请求，且由 `check.mts` 做 byte-for-byte 校验。
- context-site 不成为 Next.js route，不进入 `next build` 或 Docker image；产品代码不得导入它。
- 静态制品使用系统中文字体栈，不内联 2 MB 产品字体，也不新增字体裁剪依赖；产品前端字体保持不变。不同操作系统的字形会略有差异，人工窄屏/层级验收负责确认可读性。

## 禁止模式

- 普通文档卡片墙、彩色 dashboard、聊天 UI、营销式 hero、伪 terminal、无意义 badge 密集堆叠。
- 为了“技术感”展示随机数字、假 telemetry、假成功率或未经证据支持的运行状态。
- 把失败写成红色正文段落却不给恢复与证据；把潜在风险写成当前事实；把延期伪装成已实现。
- 手工修改生成的 `CONTEXT.html`，或在 Markdown/AGENTS 中复制完整领域解释与待办清单。
