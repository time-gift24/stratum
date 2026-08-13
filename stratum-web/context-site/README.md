# Context Site

`context-site` 生成根目录 `CONTEXT.html`，作为 Stratum 领域手册与工程待办的唯一可阅读静态制品。它不是 Next.js route，也不连接后端。

完整 ReAct 结构图由 typed content 在构建期经 Mermaid CLI 编译成内联 SVG。构建依赖 `@mermaid-js/mermaid-cli`（MIT）和 `puppeteer`（Apache-2.0），只在本地生成阶段运行：输入只能来自受版本控制的 typed content 或同域封闭模板。构建机需要系统 Chrome 或 Chromium；浏览器下载被禁用，`CONTEXT.html` 只保留 SVG，不包含 Mermaid runtime、远程资源或可执行图表脚本。本机无法自动发现浏览器时，设置 `CONTEXT_SITE_CHROME_PATH` 为其可执行文件路径。

## 人工维护源

- `content/context.ts`：领域模型、正常路径、失败、恢复、风险与证据。
- `content/todo.ts`：工程待办、依赖、状态、验收与明确延期。
- `site.css` / `runtime.js`：静态说明站点的布局与只读交互。
- `DESIGN.md`：本站独有的视觉和构建边界。

不要直接编辑根目录 `CONTEXT.html`，也不要恢复 `CONTEXT.md` / `TODO.md`。

## 命令

从 `stratum-web` 目录执行：

```sh
pnpm build:context-site
pnpm check:context-site
```

第一条更新根目录制品；第二条在临时目录重新构建并做 byte-for-byte 比较。两条命令都必须离线完成。

## 必要人工验收

自动门禁可以证明内容、证据、内部链接和生成制品一致，但不能替代真实浏览器的排版与辅助技术体验。每次修改 `site.css`、`runtime.js` 或生成模板后，至少人工检查：

1. 断网后直接打开根目录 `CONTEXT.html`；Network 面板保持零请求，领域手册与工程待办均可切换。
2. 只用键盘操作 skip link、两个页签（含 `←` / `→` / `Home` / `End`）、章节导航和证据 `<details>`；焦点始终可见。
3. 分别在 320px、768px 与桌面宽度检查长 UUID、Rust symbol、事件账本和 mini ledger；不得裁字，横向账本只能由统一容器滚动。
4. 复制概念/TODO 深链并刷新、后退、前进；页面必须恢复到同一 surface 与阅读位置。Cmd/Ctrl/Shift 点击内部链接仍保留浏览器原生行为；“展开全部证据”生成的 `?evidence=all` 也必须在复制/刷新后保持。
5. 开启 `prefers-reduced-motion` 与浏览器翻译；动画立即呈现最终态，`AgentId`、UUID、事件名、路径与 symbol 不被翻译。
6. 让不了解 Stratum 的阅读者只读“核心结构”一节，并请其指出：`AgentRuntime`、`kernel`、`durable fact`、`event stream` 分别位于哪里，以及为何 NATS 不能作为恢复依据。若无法复述，先修正文或图上的定义，再继续扩充后续章节。

人工结果只记录 PASS/FAIL、浏览器/视口和问题定位，不复制另一份领域内容。
