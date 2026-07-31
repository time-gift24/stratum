# Design: add-constitution-review-skill

## Context

本次生成的 `CONSTITUTION.md`（Axum 版）是引发点，方向锚定推荐技术栈：Axum + Tower、tracing、metrics、OpenTelemetry、utoipa、clippy/rustfmt CI 门禁、cargo-audit/deny、thiserror/anyhow、多阶段 Docker。但 skill 不能绑定这一份宪法——每个项目的技术栈和条款不同，条款必须由各项目自己的 `CONSTITUTION.md` 承载。

本仓库已有 `.agents/skills/rust-skills/`（通用 Rust 编码规则，writing 阶段用）。本次新增的是 review 阶段的合规审视能力，二者阶段不同、来源不同（通用规则 vs 项目自定义条款）。

## Goals / Non-Goals

**Goals:**

- 一个自包含的 agent skill（单 `SKILL.md` + 一个示例模板文件），复制到任何项目的 `.agents/skills/` 即可用。
- 审查依据完全来自目标项目根目录的 `CONSTITUTION.md`；skill 不内嵌条款。
- 默认审查 `git diff`（工作区相对 HEAD，或用户指定 base），支持指定路径全量审查。
- 输出结构化、可定位、分级的报告，每条违规引用宪法条款编号并给出 `文件:行号` 证据。
- 全程只读：不修改代码、不运行构建、不安装依赖。

**Non-Goals:**

- 自动修复代码、自动改写以符合宪法。
- CI 门禁集成（pre-commit / GitHub Actions）——后续独立 change。
- 对无 `CONSTITUTION.md` 的项目凭空生成条款并审查（只提示可用示例模板起步）。
- 前端视觉审查（已有 impeccable / DESIGN.md 体系）。

## Decisions

### D1: 条款外置，skill 只做"对照机制"

Skill 内不写入任何宪法条款。运行时流程：定位项目根 → 读 `CONSTITUTION.md` → 解析出条款清单 → 逐条对照代码。理由：条款是项目资产，随项目演进；内嵌会导致 skill 与条款双向腐化。本次的 Axum 版宪法作为 `examples/CONSTITUTION.axum.md` 附带，仅作起步模板，明确标注"示例，非审查依据"。

被否决方案：条款内嵌 skill（换栈即改 skill，腐化快）；内嵌+项目覆盖（两套来源，合并语义复杂，违反克制设计）。

### D2: 条款解析采用"松散结构约定"而非严格 schema

宪法是 Markdown，不要求机器可校验的 front matter。Skill 按以下约定提取条款：

1. 一级/二级标题为章节（如 `## 2. 错误处理（强制）`）。
2. 含 `禁止`/`必须`/`不得`/`铁律`/`Red Flag` 的语句为硬性条款 → 严重级 `red-flag` 或 `violation`。
3. 含 `优先`/`推荐`/`尽量` 的语句为建议 → 严重级 `suggestion`。
4. 附录"禁止清单（Red Flags）"checkbox 项一律为 `red-flag`。

理由：不要求项目改写现有宪法格式；宽松解析失败时退化为"按章节整体对照"，而不是报错退出。风险是条款切分粒度粗——可接受，审查报告由 agent 语义判断兜底。

### D3: 默认审查 diff，而非全量

默认取 `git diff HEAD`（未提交改动）；用户可指定 base（如 `main...HEAD`）或路径做全量。理由：diff 审查噪声小、与提交前检查场景吻合；全量审查历史代码会产生大量存量违规噪声，应由用户显式触发。

### D4: 报告格式固定四段

```
## Constitution Review Report
- 审查依据: CONSTITUTION.md (commit <sha 前 8 位>)
- 审查范围: git diff HEAD (N files) / <指定路径>
- 结论: X red-flag / Y violation / Z suggestion

### Red Flags（一票否决）
| 条款 | 位置 | 证据 | 说明 |
### Violations（违反强制条款）
| 条款 | 位置 | 证据 | 说明 |
### Suggestions（偏离推荐实践）
| 条款 | 位置 | 证据 | 说明 |
```

每条必须含 `文件:行号` 与代码摘录，禁止无证据的泛泛意见；报告中明确区分"宪法覆盖项"与"宪法未覆盖项"（未覆盖的灰色问题单列一节，标注 `constitution-gap`，提示用户补充宪法而非静默放过）。

理由：固定格式让报告可 diff、可归档、可接入未来的 CI；`constitution-gap` 一节让宪法本身持续演进——这回应"宪法只是引发"的定位。

### D5: 条款先分类，再决定检查方式

审查流程第一步是对每条宪法条款做分类，谓词可观察：该条款的违规能否被 `rustfmt` / `clippy` / `cargo-deny` 机械判定。

- 能机械判定 → 转入「配置文件存在性检查」：验证 `rustfmt.toml`、`.clippy.toml`、`deny.toml`、CI workflow 对应步骤存在并启用；缺失或未启用记 `violation`。配置取值由工具自身在 CI 执行，skill 不越俎代庖。
- 不能机械判定（分层依赖、事务边界、敏感数据记录等语义条款）→ 进入逐条对照流程，证据获取用 Grep/Read，不执行 `cargo build`/`clippy`。

分类是 recipe 的第一步而非禁令：机械条款根本不进入"审查代码"的路径，误报在结构上无处可生——而不是靠"不要报告机械违规"这种 prohibition 约束（writing-skills 实测：shaping 类问题用 prohibition 反效果）。

被否决方案：skill 全包所有条款（与静态工具重复报告）；skill 调用 cargo clippy 作为子步骤（违反只读约束，环境依赖重）；prohibition 式分工约定（见上）。

### D7: Skill 生产流程遵循 writing-skills 的 RED-GREEN-REFACTOR

SKILL.md 的内容不由设计想象决定，而由无 skill 基线测试中观察到的真实失败模式决定（Iron Law: NO SKILL WITHOUT A FAILING TEST FIRST）。GREEN 阶段只写解决基线失败的最小内容，不为假想情况加内容；REFACTOR 阶段把新出现的借口显式反驳补入 skill。

依据：用户级 `~/.agents/skills/writing-skills/`（TDD applied to process documentation）。本 design 中的条款形态判断（recipe vs prohibition）同样以该方法论为准。

## Risks / Trade-offs

- [宽松解析误切条款，漏审或错分级] → 解析结果在报告开头列出"本次对照条款清单"摘要，用户可一眼发现切分异常；切分失败退化为按章节对照。
- [Agent 语义判断产生误报] → 每条违规必须附代码证据，无证据不得列入；suggestion 级允许标注置信度。
- [宪法本身质量差时报告无意义] → skill 不负责评审宪法质量；`constitution-gap` 机制反向推动宪法完善。
- [大型 monorepo 全量审查超上下文] → 全量模式按 crate/目录分批审查并汇总，SKILL.md 中明确该流程。

## Migration Plan

新增文件，无存量修改，无需迁移。回滚 = 删除 `.agents/skills/constitution-review/`。

## Open Questions

- 示例模板是否只保留 Axum 版，还是同时给一份"最小骨架"（空章节待填）？倾向：先只给 Axum 版，骨架等真实第二个项目出现时再抽。
