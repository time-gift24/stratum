# Tasks: add-constitution-review-skill

## 1. RED：基线测试（无 skill）

- [x] 1.1 设计 3+ 基线场景：派 subagent 对照项目根 `CONSTITUTION.md` 审查含违规的 Rust diff（可用 `crates/stratum-api` 构造）；压力组合：大 diff、模糊条款、暗示"顺便修一下"
- [x] 1.2 无 skill 运行场景，逐字记录真实失败：是否编造条款、是否缺 `文件:行号` 证据、是否混入 clippy 可判定问题、是否顺手改代码、报告是否空泛
- [x] 1.3 归纳失败模式清单——GREEN 阶段的唯一输入

## 2. GREEN：最小 SKILL.md

- [x] 2.1 创建 `.agents/skills/constitution-review/`；将本次生成的 Axum 版宪法存为 `examples/CONSTITUTION.axum.md`，文件头部标注"起步示例，非审查依据，复制到项目根后按项目实际修改"
- [x] 2.2 front matter：`name` + `description`，description 只写触发条件，不写流程摘要；覆盖中英文关键词（constitution / 宪法 / compliance / 合规 / review / 审查）
- [x] 2.3 只针对 1.3 的真实失败写最小内容：审查流程（定位项目根 → 读 CONSTITUTION.md，缺失则停止并指向示例模板）、条款解析分级、报告模板（含 `constitution-gap`）、只读约束、静态工具分工
- [x] 2.4 带 skill 重跑 1.1 的场景，验证 subagent 合规

## 3. REFACTOR：堵漏

- [x] 3.1 重测中发现的新借口/新漏报 → 显式反驳补入 SKILL.md（rationalization table / red flags 清单，如基线显示需要）——GREEN 三场景全合规，无新借口，无需改动
- [x] 3.2 重测直到稳定通过——三场景一轮全部通过，行为稳定

## 4. 质量与归档

- [x] 4.1 `wc -w SKILL.md` < 500 词（实测 225）；确认无内嵌条款内容，示例模板标注齐全
- [x] 4.2 运行 `openspec validate --all --strict` 并修复所有问题
- [x] 4.3 确认 `.agents/skills/` 相关 AGENTS.md/说明（如有）补充 constitution-review 与 rust-skills 的分工（writing vs review）——`.agents/` 下无 skills 级 AGENTS.md/README，条件不成立
- [x] 4.4 提醒用户：合入前完成 crate `AGENTS.md` 归档（本仓库 AGENTS.md 文档规范要求）
