# excalidraw-whiteboard Specification

## Purpose

Excalidraw 以前端插件形态置入系统：对话界面把 `excalidraw_render` 工具结果内嵌渲染为只读白板，另提供独立可编辑画板页 `/excalidraw`（SiteNav 入口 + 方向性转场 + 沉浸模式）。视觉全部改接产品语义 token，画布透明透出容器底色，亮暗零色差；库与样式表动态加载，不进首屏。后端工具实现与内容持久化不在本能力范围。

## Requirements

### Requirement: 按工具名分发 Excalidraw 渲染

对话工具调用块 SHALL 在 `call.name === "excalidraw_render"` 且 `call.result` 通过最小 scene 形状校验时，将结果区渲染为 Excalidraw 白板组件，而不是原始 JSON 文本。其他工具名的渲染路径 MUST NOT 因此改变。

最小 scene 形状校验：结果文本可解析为 JSON 对象，且 `elements` 字段为数组。校验失败 MUST 回退到现有原始 JSON `<pre>` 展示，不得抛出渲染错误。

#### Scenario: Excalidraw 工具结果渲染为白板

- **WHEN** 工具调用 `excalidraw_render` 完成，且 `result` 是含 `elements` 数组的 JSON
- **THEN** 工具调用块展开后，结果区显示只读白板而非原始 JSON

#### Scenario: 非 Excalidraw 工具不受影响

- **WHEN** 任意其他工具名的调用完成
- **THEN** 结果区仍按现有方式渲染参数/结果/错误文本

#### Scenario: 结果形状不合法时回退

- **WHEN** `excalidraw_render` 的 `result` 不是合法 JSON 或缺少 `elements` 数组
- **THEN** 结果区回退为原始 JSON 文本展示，页面不报错

### Requirement: 白板为只读内嵌预览

白板 MUST 以 Excalidraw view mode 渲染（禁止编辑），内嵌于工具调用块内部，不得脱离对话消息列布局（遵守消息列 `max-w-[44rem]` 约束）。可编辑画布由独立白板页 `/excalidraw` 承载（见「独立白板页与导航入口」），不进入对话页面。

#### Scenario: 只读不可编辑

- **WHEN** 用户在白板区域点击或拖放元素
- **THEN** 场景内容不发生变化（view mode，zen mode 关闭、网格关闭）

#### Scenario: 内嵌于工具调用块

- **WHEN** 工具调用块处于折叠状态
- **THEN** 白板不渲染不占位；展开后白板在结果区以固定高度展示

### Requirement: 独立白板页与导航入口

系统 SHALL 提供独立白板页 `/excalidraw`，承载完整可编辑的 Excalidraw 编辑器，并在顶部 SiteNav 提供「Excalidraw」入口。该路由 MUST 登记进 `PAGE_ORDER`，与 `/conversation` 之间跳转带方向性左右转场。编辑器 MUST 动态加载（SSR 关闭、样式表随 chunk 懒加载）并跟随 next-themes 主题。

#### Scenario: 导航进入白板页

- **WHEN** 用户点击 SiteNav 的「Excalidraw」入口
- **THEN** 当前页面向左滑出、白板页从右滑入（PAGE_ORDER 中 /excalidraw 位于 /conversation 右侧），编辑器加载完成后可直接编辑

#### Scenario: 白板页主题跟随

- **WHEN** 用户在白板页切换亮色/暗色主题
- **THEN** Excalidraw 画布主题随之切换

#### Scenario: 首屏不携带编辑器

- **WHEN** 用户访问 /conversation 且未打开白板页
- **THEN** 初始加载不含 Excalidraw 的 JS/CSS chunk

#### Scenario: 白板页沉浸模式

- **WHEN** 用户位于 /excalidraw
- **THEN** 顶部导航在进入时短暂展示后自动收起，画布满铺；顶边热点悬停/点击或键盘聚焦可唤出导航，离开或按 Esc 收回；其他页面导航保持常开

### Requirement: 主题与视觉 token 合规

白板卡片 MUST 只消费 `app/globals.css` 的语义 token（如 `bg-card`、`border-border`），MUST NOT 写死色值；Excalidraw 画布主题 MUST 跟随 next-themes 的 light/dark 切换。

#### Scenario: 跟随主题切换

- **WHEN** 用户切换亮色/暗色主题
- **THEN** 白板画布与卡片容器随主题切换，无色值残留

### Requirement: 动态加载与降级状态

Excalidraw MUST 通过动态导入加载（SSR 关闭），加载中 SHALL 显示与白板卡片形状一致的骨架占位；库加载失败时 MUST 回退为原始 JSON 文本展示。

#### Scenario: 加载中骨架

- **WHEN** 用户展开含 Excalidraw 结果的工具调用块且库尚未加载完成
- **THEN** 结果区显示固定高度的骨架占位，加载完成后无布局跳动地替换为白板

#### Scenario: 加载失败回退

- **WHEN** 动态导入失败（如 chunk 加载错误）
- **THEN** 结果区显示原始 JSON 文本，页面其余功能不受影响
