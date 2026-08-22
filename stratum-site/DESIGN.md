---
name: 运筹 Stratum — stratum-site
description: 流体雾场·纸墨：宣纸暖白大气场随滚动沉入墨黑，MoltenMetal 熔墨 shader 为背景（React Bits 底稿，纸墨配色），中英混排大标题，朱砂配给制 + 古铜辅助
colors:
  paper: "#fbf4e7"
  paper-dim: "#f1e8d6"
  mist: "#a89d88"
  ink: "#1f1b15"
  ink-soft: "#605747"
  abyss: "#16120e"
  abyss-raise: "#251f18"
  bone: "#f7f0e2"
  fog: "#a79c89"
  seal: "#b24731"
  seal-deep: "#8f3722"
  bronze: "#be9563"
typography:
  display:
    fontFamily: "Archivo, Noto Sans SC, sans-serif"
    fontSize: "clamp(2.25rem, 5.2vw, 4.5rem)"
    fontWeight: 900
    lineHeight: 1.08
    letterSpacing: "-0.02em"
  display-accent:
    fontFamily: "Source Serif 4, Noto Serif SC, serif"
    fontStyle: italic
    fontWeight: 400
  headline:
    fontFamily: "Archivo, Noto Sans SC, sans-serif"
    fontSize: "clamp(1.75rem, 3.2vw, 2.75rem)"
    fontWeight: 700
    lineHeight: 1.15
  body:
    fontFamily: "Archivo, Noto Sans SC, sans-serif"
    fontSize: "1.0625rem"
    fontWeight: 400
    lineHeight: 1.75
  eyebrow:
    fontFamily: "Chivo Mono, Noto Sans SC, monospace"
    fontSize: "0.75rem"
    fontWeight: 500
    letterSpacing: "0.22em"
rounded:
  card: "20px"
  pill: "999px"
spacing:
  section: "clamp(6rem, 14vh, 11rem)"
  gutter: "clamp(1.5rem, 4vw, 3rem)"
components:
  button-primary:
    backgroundColor: "{colors.ink}"
    textColor: "{colors.paper}"
    rounded: "{rounded.pill}"
    padding: "16px 32px"
  button-primary-hover:
    backgroundColor: "{colors.seal}"
  prompt-box:
    backgroundColor: "rgba(255,255,255,0.8)"
    textColor: "{colors.ink}"
    rounded: "{rounded.card}"
    padding: "12px"
---

# Design System: 运筹 Stratum — stratum-site

## Overview

**Creative North Star: "纸上下潜"（Diving Through Paper-Ink）**

页面是一幅垂直展开的纸墨长卷：顶部是暖纸白的明亮大气场，一幅熔墨在其中缓慢游动、随指针漂移；向下滚动即向下潜——雾、深水、墨黑——最深处陈列架构与事实；随后浮回纸面，以一个干净的行动收尾。深浅变化不是装饰，它就是产品承诺"渐进式透明"的空间化演示：表面是对话，深处是 runtime。

材质哲学：纸、墨、雾。没有玻璃拟态、没有霓虹辉光、没有网格线装饰；深度靠色调分层与柔和的环境阴影表达。动效是有机体级别的——一切运动都像液体：快起缓落，从不弹跳。

**Key Characteristics:**
- 垂直明暗潜程：paper → mist → abyss → paper，一页之内完成
- MoltenMetal 熔墨场是全页固定背景（InkField：雾边纸色 → 深褐丝缕 → 朱砂红芯），随指针漂移；下潜渐变是半透明罩纱，浅处墨痕透出、深处红芯如火种，滚动无割裂
- 中文黑体巨型标题，英文衬线斜体词混排强调（如 *Agent*）
- 暖调三色体系：宣纸米白为地，朱砂配给制钤印，古铜承担运行态与轨迹
- 圆角有机（卡片 20px、交互件全圆角），无直角、无 hairline 边框装饰

## Colors

暖调纸墨双色阶 + 朱砂与古铜。色板参照东方纸墨体系（宣纸 / 朱砂 / 古铜），明暗是叙事轴，不是主题切换。**红色与绿色系永远不得同屏并置**——全站没有任何绿色。

### Primary
- **墨 Ink** (#1F1B15): 暖黑。纸面主文字、主按钮地、深色 UI 件。页面中最重的实色。
- **宣纸 Paper** (#FBF4E7): 页面顶部与结尾的地色，暖米白纸面。

### Secondary
- **深处 Abyss** (#16120E): 下潜段的暖墨黑地色，架构与证明内容栖身于此。
- **骨白 Bone** (#F7F0E2): 深处主文字。
- **雾灰 Fog** (#A79C89) / **墨软 Ink Soft** (#605747): 两个明度区的次级文字，各自服务自己的地。

### Tertiary
- **朱砂 Seal** (#B24731): 第一强调色，配给制。只出现在：审批/关键状态标记、focus ring、每屏至多一处的主行动强调。深色变体 Seal Deep (#8F3722) 用于 hover。
- **古铜 Bronze** (#BE9563): 第二强调色，承担"正在运行"——运行态色点、执行轨迹线、暗部点缀。它分担过去绿色/青色的职责，与朱砂同色系不冲突。
- **雾 Mist** (#A89D88) / **纸凹 Paper Dim** (#F1E8D6) / **深浮 Abyss Raise** (#251F18): 过渡中间调与各自的卡片地。

### Named Rules
**The One Seal Rule.** 朱砂的 UI 强调在任何一屏内最多两处（熔墨背景的红芯是材质肌理，不计入配额）。它是钤印，不是高亮笔；大面积红色块永远禁止。
**The No-Green Rule.** 全站禁止绿色系（含青绿、磷青、荧光绿）。运行态用古铜，审批态用朱砂，成功/同步态无色点——红绿并置在本品牌语境是事故。
**The Dive Rule.** 地色变化只沿垂直潜程发生（paper → abyss → paper），同一视口内禁止明暗地块拼接。

## Typography

**Display Font:** Archivo 900（西文）+ Noto Sans SC 900（中文）
**Accent Font:** Source Serif 4 italic（仅英文强调词，嵌入大标题）
**Body Font:** Archivo 400/500 + Noto Sans SC 400/500
**Label/Mono Font:** Chivo Mono 500（眉题、数据、状态标签）

**Character:** 黑体的重量承担说服力，衬线斜体的一个英文词承担灵气；等宽体只出现在"仪器读数"场景——眉题、状态、命令。

### Hierarchy
- **Display** (900, clamp(2.25rem, 5.2vw, 4.5rem), 1.08, -0.02em): 首屏与段落大标题；一行中文不超过 12 字，宁小勿大。
- **Display Accent** (Source Serif 4 italic 400): 每屏最多一个英文强调词，字号随 display。
- **Headline** (700, clamp(1.75rem, 3.2vw, 2.75rem), 1.15): 段落标题。
- **Body** (400, 1.0625rem, 1.75): 正文，最大行宽 38ch（中文）/ 65ch（英文）。
- **UI** (500, 0.9375rem): 控件文字、卡片正文、导航字标。
- **Input** (400, 1rem): 输入框文本；不低于 16px，避免 iOS 聚焦时页面自动缩放。
- **Eyebrow** (Chivo Mono 500, 0.75rem, +0.22em, uppercase): 段落眉题与状态读数。

### Named Rules
**The One Italic Rule.** 衬线斜体强调词每屏最多一个，且永远是英文词；中文不用斜体。
**The Mono Readout Rule.** 等宽体只承载"可测量"的内容：数字、状态、命令、标签；不用等宽体写句子。

## Layout

单中轴叙事：所有首屏级内容压在页面中线上，最大内容宽 1120px，两侧 gutter 为 clamp(1.5rem, 4vw, 3rem)。段落纵向节奏为 clamp(6rem, 14vh, 11rem)，标题上方空间恒大于下方。深处段落（abyss）用完整地色块包裹，上下以柔和过渡带衔接，不做硬切线。移动端（<768px）全部单栏，display 字号随 viewport 收缩，熔墨背景保留。

## Elevation & Depth

本系统不用阴影建造结构；深度由色调分层承担（paper/mist/abyss 的垂直潜程）。阴影只作为环境光存在：悬浮 pill 与卡片有一层大而软的环境投影，hover 时微微加深并上浮 2px——像物体在液体中略微升起。

### Shadow Vocabulary
- **pill** (`0 24px 60px rgb(31 27 21 / .18), inset 0 1px 0 rgb(255 255 255 / .7)`): 首屏悬浮输入框，页面唯一"浮空"的物体。
- **card** (`0 2px 4px rgb(31 27 21 / .05), 0 12px 32px rgb(31 27 21 / .08)`): 纸面卡片。
- **card-deep** (`0 2px 4px rgb(0 0 0 / .3), 0 16px 40px rgb(0 0 0 / .35)`): 深处卡片。

### Named Rules
**The Buoyancy Rule.** 元素只在响应状态时获得高度（hover 上浮 2px、阴影加深）；静止时万物贴地。

## Shapes

有机圆润：卡片 20px 圆角，一切交互件（按钮、输入、chip）全圆角 pill。禁止直角卡片、hairline 边框堆叠和硬分割线；分隔靠留白与色调过渡。唯一的"硬"元素是终端命令卡的 8px 圆角——它引用终端的器物感。

## Components

### Buttons
- **Shape:** 全圆角 pill（999px）。
- **Primary:** 墨地纸字（ink on paper），padding 16px 32px，字重 600；hover 变朱砂并上浮 2px，transition 250ms ease-fluid。
- **Ghost:** 透明地 + 1px 墨色 25% 描边，hover 描边转实。
- **Focus:** 2px 朱砂 outline，offset 3px，全站统一。

### Prompt Box（首屏签名组件）
- **Style:** 白偏灰（box #f4f3f0）85% 透明 + backdrop blur 的卡片盒（20px 圆角、pill 大阴影浮于墨场之上）；盒外不再有任何可见承载结构（无渐变带、无边框板），直接浮于页面；上部自动长高 textarea（封顶 200px），下部工具行：模型 Dropdown + 联网开关（朱砂 8% 为开）+ 建议 Dropdown——工具按钮一律白底 + chip 软阴影从盒面浮起，禁止用 paper-dim 实色（与盒底糊成一片）；右侧实时字数（mono tabular）+ 朱砂圆形发送钮（hover 转 Seal Deep，执行中即停止钮，空内容禁用为纸凹灰）。
- **Behavior:** Enter 发送 / Shift+Enter 换行（兼容中文输入法组词）；空内容发送禁用；建议点选即填入；提交携带任务描述跳转 `/conversation`（宿主可用 onSubmit 接管）。聚焦只加深阴影，不画外框。
- **Dropdown（派生公共组件）:** paper 地 + card 阴影的 listbox，完整键盘导航（↑↓/Home/End/Enter/Esc/Tab），选中项朱砂勾。

### Cards / Containers
- **Corner Style:** 20px 圆角。
- **Background:** 纸面用 paper-dim，深处用 abyss-raise；无边框，靠地色差区分。
- **Shadow Strategy:** 见 Buoyancy Rule，静止贴地。
- **Internal Padding:** clamp(1.5rem, 3vw, 2.5rem)。

### Chips / 状态标签
- **Style:** 全圆角，Chivo Mono 0.75rem 字距 0.12em；深处为 1px 骨白 14% 描边 + 半透明地。
- **State:** 状态点用色点表达——古铜运行中、朱砂待审批；静止状态无色点。

### Navigation
- 顶栏 48px 内边距，左侧字标"运筹 STRATUM"，右侧 mono 小字导航 + 中/EN 切换；滚动进入深处时反相为骨白。无下划线、无背景条，纯文字浮于场上。

### NavTree（侧栏签名组件）
- **章法：** mono 小节标签（12px、+0.12em、全大写）+ 左侧树线导轨（1px 墨色 10%）+ 条目缩进；12px 紧凑行（与 mono 小节标签同字号，Gemini 量级密度），行间距 2px，小节间距 24px。
- **Hover:** 朱砂 6% 底 + 朱砂文字（图标跟随）；层级为 ink-soft 静止 → 朱砂 6% 悬浮 → 朱砂 8% + 朱砂标选中。
- **Active:** 朱砂文字 + 朱砂 8% tinted 底，导轨上落 2px 朱砂标——导航选中态是朱砂的正当岗位（不计入装饰配额）。
- **Collapsed:** 收成 64px 图标轨，标签/树线/徽标隐去，图标居中，悬停给 title。

### Terminal Card（深处签名组件）
- 8px 圆角、abyss-raise 地、Chivo Mono；命令行前缀 `$` 用朱砂，注释行用 fog；右上角三个窗控点。

### Trace Ribbon（深处签名组件）
- SVG 执行轨迹：古铜至骨白渐变折线 + 朱砂审批尖峰，stroke 2.5px，下方渐变面积淡出在雾中；状态点带 seal-pulse 呼吸。它是"每次执行留下可回放轨迹"的图形化事实。

## Do's and Don'ts

### Do:
- **Do** 让明暗变化只随垂直滚动发生，用完整段落地色块过渡。
- **Do** 把熔墨当作活材料：随指针漂移、自主流动、reduced-motion 退化为静态渐变。
- **Do** 每屏只设一个视觉焦点：一个词、一枚印、或一条轨迹。
- **Do** 中文标题控制在两行以内，宁可缩短文案不可缩字号。

### Don't:
- **Don't** 使用玻璃拟态卡片、霓虹辉光描边或网格线背景装饰——那是别的世界。
- **Don't** 用朱砂做大面积底色、按钮常态色或正文强调色。
- **Don't** 给中文设置斜体；中文的强调靠字重与衬线混排。
- **Don't** 在静止状态使用脉冲/呼吸动画；seal-pulse 只属于"正在发生"的状态点。
