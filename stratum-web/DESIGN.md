# 设计系统：运筹 Stratum

## 1. 视觉方向

Stratum 的候选视觉世界是“精密校准台”：近黑石墨工作面、纯白文字、低对比层次、机械式比例与极少量高纯度信号色。科技感来自材质、对齐、清晰状态和精确动效，不依靠大面积霓虹、伪终端文案或装饰性玻璃卡片。

对话仍然保持单一输入重心。普通产品页面与组件校准页都使用安静的连续表面；点阵、节点和连接线只留给未来正式画布，不得进入当前 `/chat`。

目标读数：布局差异度 7/10，动效强度 5/10，视觉密度 5/10。

## 2. Token 契约

`app/app.css` 中的标准 shadcn Token 是所有组件的颜色真源。业务组件只消费 `background`、`foreground`、`card`、`popover`、`primary`、`secondary`、`muted`、`accent`、`destructive`、`border`、`input`、`ring`、`chart-*` 与 `sidebar-*`。

禁止在业务组件中写 RGB、Hex 或创建同义颜色变量。阴影与半透明表面必须通过标准 Token 的透明度或 `color-mix()` 推导。品牌 SVG 可以保留自身的 mark 变量。

### 全局产品表面

- Background `#090808`
- Foreground `#F9F6F5`
- Card `#171716`
- Popover `#1C1B1B`
- Secondary `#292727`
- Muted `#212020`
- Muted foreground `#A69C98`
- Border `#32302F`
- Input `#3B3837`
- Primary / 品牌与主要行动 `#78ED9D`
- Destructive / 错误 `#C75A50`

### 组件校准候选色

`/component-gallery` 在页面根节点局部重映射相同的标准 Token，用于并列校准候选色；这些值不覆盖全局产品 Token，也不得写进组件源码。

- Background `#0D0D0D`
- Foreground `#FFFFFF`
- Card `#191919`
- Popover `#202020`
- Secondary `#262626`
- Muted `#1E1E1E`
- Muted foreground `#A7A7A7`
- Border `#343434`
- Input `#3E3E3E`

- Primary / 品牌与主要行动 `#78ED9D`
- Chart 1 / 信息 `#6DB5FF`
- Chart 2 / 注意与辅助 `#FEFA3D`
- Chart 3 / 协作 `#FF5DE7`
- Chart 4 / 成功与持续执行 `#78ED9D`
- Chart 5 / 环境辅助 `#B48CFF`
- Destructive / 错误 `#C75A50`

信号色不能随机分配。绿色是品牌主色，用于主要行动、选中与焦点强调，也承载成功与持续执行；黄色只用于需要注意的辅助信号，蓝色用于信息，洋红用于协作语义，红色用于错误和破坏性操作。单个产品视图通常只出现一个主强调色；组件校准页可以并列展示全部角色。

## 3. 排版

- UI、品牌导航与正文：Geist Variable
- 大标题：Outfit Variable
- 路径、尺寸和测量值：Geist Mono Variable
- 中文回退：PingFang SC、Microsoft YaHei 与系统 sans-serif

正文与对话使用 15px 至 16px。全局品牌使用 18px / 620，一级导航使用 15px / 600，导航面板标题使用 15px / 620、说明使用 14px / 470。元数据下限 13px。大标题上限 96px，字距不得紧于 `-0.04em`。等宽字体只用于代码、路径、色值和测量，不作为科技装饰。

## 4. 组件展示页 `/component-gallery`

该路由是视觉系统的校准环境，不是面向最终用户的一级产品页面，因此不进入当前产品壳层，也不改变 `/` 到 `/chat` 的默认路径。

- 路由只在开发模式注册，生产构建不暴露该页面
- 桌面由左侧 `vertical-navigation` 和单列展示面组成
- 顶部继承全局 Layout 中的 `centered-navigation`；左侧 `vertical-navigation` 只负责本页锚点
- 首屏同时呈现导航本体、排版主样和完整信号色谱
- 内容使用边界分段与列表式台账，不使用 Storybook 式等宽卡片墙
- 背景只使用由标准 Token 推导的连续低对比层次光，不使用点阵或网格装饰
- 窄屏仍保留居中的浮动捷径列，不转换为顶部或底部工具栏；触控目标至少 44px
- 页面必须同时验证中文、英文、键盘焦点和 reduced motion

## 5. Vertical navigation

- 文件与组件命名固定为 `vertical-navigation` / `VerticalNavigation`
- 桌面轨道距离视口 16px，宽 76px，使用 sidebar Token
- 项目基础尺寸 48px，指针附近最大 60px；缩放由到指针的真实距离连续计算
- Tooltip 位于项目右侧，同时响应 hover 与 keyboard focus
- 选中状态使用对应信号色的细边界、浅色表面和同色图标
- 窄屏使用居中的紧凑单列，不增加顶部品牌或底部返回入口
- `prefers-reduced-motion` 下直接使用 48px，并取消入场与弹性过渡

## 6. Centered navigation

- 文件与组件命名固定为 `centered-navigation` / `CenteredNavigation`
- 组件由根 Layout 渲染，跨 `/chat` 与开发态 `/component-gallery` 持续存在
- 品牌 Logo、跨页面入口与语言切换集中在一个固定 32rem 宽度的居中悬浮玻璃岛；展开面板继承同一宽度，只改变高度，禁止横向跳动
- 品牌和一级导航使用 Geist，依靠明确字号、字重与留白建立层级，不用低对比小字弱化全局入口
- 桌面端分类通过 hover、focus 或 click 展开双列入口；Escape 与移出组件关闭
- 窄屏折叠为品牌、语言和菜单按钮，展开后显示同一组真实入口
- 顶部冷紫径向光提供玻璃背后的真实环境；导航本体无描边，以半透明渐变、背景模糊和向下阴影建立层级
- 内部展开区不使用分割线，入口卡片通过低对比表面与悬浮阴影区分层次
- 不保留参考源码中的 Flowbase、Pricing、登录、免费试用或其他虚构入口

## 7. 产品壳层与对话

- 根路由 `/` 继续重定向至 `/chat`
- 根 Layout 负责唯一的全局 `centered-navigation`；`ProductShell` 不再自建顶部栏
- 每个页面自行渲染自己的 `vertical-navigation`，不得把页面动作塞进全局导航
- `vertical-navigation` 始终作为视口覆盖层，不进入聊天舞台、消息列或 Composer 的宽度计算；核心任务区以完整视口水平中心为基准
- `/chat` 左侧只保留唯一的新建对话入口和按需打开的历史入口
- 对话只显示消息与 Composer，不使用节点、连接线、参数检查器或常驻运行面板
- 新建对话首屏以单一大型 Composer 为视觉中心；底部左侧放 Agent、模型和思考配置，右侧保留独立发送按钮
- 窄屏下左侧导航仍是覆盖层，但动作组改为顶部排列，避免与居中的 Composer 相互遮挡
- 已有对话保持窄阅读列，Composer 贴近底部但不遮挡消息和审批操作
- 思考、工具执行和中间步骤默认折叠，用户按需展开真实详情
- 不展示“就绪/未就绪”或同义资源可用性状态

## 8. 形状、纵深与动效

- 全局基础圆角 Token 为 16px；小型图标井约 12px，控件和导航项目约 16px
- Composer、浮层与全局导航使用约 21px 的 `radius-xl`
- 玻璃导航、Composer 与浮层不使用可见描边；数据表格或语义分组确需分隔时才使用 1px Token 边界
- 阴影必须同时具备垂直偏移和柔和模糊；禁止零偏移彩色光晕
- 玻璃材质只用于覆盖内容的全局导航、Composer 与浮层：半透明标准表面、方向性渐变、背景模糊和有垂直位移的阴影；普通内容卡片不使用玻璃
- CSS 负责 150ms 至 220ms 的悬浮、按下与焦点反馈
- Motion 负责 Dock 磁性缩放、Tooltip 和浮层迁移
- GSAP 只用于真正需要编排的一次性产品入场，不做环境循环动画
- 动效必须从可读的默认状态开始，并提供 reduced-motion 结果

## 9. 图标与控件

产品主体优先使用 Tabler Icons。经用户提供的 React Bits Pro 组件允许使用其既有 Lucide 依赖，但同一组件内部不得混用图标家族。

品牌标志以原始 SVG 为唯一母版，保留其非对称圆形轨道、内部交叠路径和中心汇合结构，不再用字母、规则叶片或重新发明的通用 AI 结替代。界面中的紧凑标识使用品牌绿单色蒙版，以微量同色阴影补偿 16–32px 下的线宽损失；原始多色版本只作为品牌档案保留。标志不得演变为字母徽记、速度线、块状折带、规则旋叶、双箭头或 Web3 徽记。Hover 与键盘焦点只允许整体发生一次 6° 微旋和轻微增亮，禁止常驻旋转。

无可见文字的按钮必须提供本地化 `aria-label`。控件必须覆盖默认、悬浮、焦点、按下、禁用、加载和错误状态。用户已明确授权引入并适配 `app/components/react-bits/vertical-navigation.tsx`；其他 `react-bits`、`ai-elements` 和 `ui` 源码仍遵守项目保护约束。

## 10. 验证清单

- `/` 正确进入 `/chat`
- `/chat` 没有画布结构、重复新建入口或就绪状态
- 全局导航只由根 Layout 渲染一次，聊天页左侧导航只包含页面动作
- `/component-gallery` 桌面与移动布局完整
- 垂直导航 hover、focus、active、Tooltip 和窄屏紧凑列可用
- 居中导航 hover、focus、click、Escape 与窄屏菜单可用
- 中文与英文不溢出
- 主要触控目标至少 44px
- 正文与占位符对比度达标
- reduced-motion 可用
- 无横向溢出，控制台无错误

## 11. 禁止项

- 当前对话页不得出现点阵画布、节点、连接线、参数检查器或常驻运行面板
- 不重复提供新建对话入口
- 不用主题化文案解释文化或科技感
- 不用无功能小字、装饰编号、伪参数、虚构状态或虚构产品数据
- 不以随机霓虹、渐变文字、玻璃卡片墙或发光边框代替真实信息层级
