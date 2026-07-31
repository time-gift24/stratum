# constitution-review Specification

## Purpose

定义 `constitution-review` agent skill 的行为契约：以目标项目根目录的 `CONSTITUTION.md` 为唯一审查依据，对 git diff 或指定路径的代码执行只读合规审视，输出分级违规报告（red-flag / violation / suggestion + constitution-gap）。机械可判定条款路由到静态工具配置检查，skill 只审语义条款。

## Requirements

### Requirement: Skill 以项目根 CONSTITUTION.md 为唯一审查依据

The skill SHALL locate the target project root and read `CONSTITUTION.md` from it as the sole source of review clauses. The skill MUST NOT embed clause content itself. 若项目根不存在 `CONSTITUTION.md`，the skill SHALL stop and report that no constitution was found, and MAY point the user to the bundled example template (`examples/CONSTITUTION.axum.md`) as a starting point.

#### Scenario: 项目根存在宪法

- **WHEN** 在含有 `CONSTITUTION.md` 的项目中触发 skill
- **THEN** skill 读取该文件并以其中条款作为全部审查依据，不引用 skill 自带示例中的任何条款

#### Scenario: 项目根缺少宪法

- **WHEN** 在不含 `CONSTITUTION.md` 的项目中触发 skill
- **THEN** skill 停止审查并告知未找到宪法，提示可复制 `examples/CONSTITUTION.axum.md` 起步，不凭空编造条款进行审查

### Requirement: 条款解析与分级

The skill SHALL parse the constitution Markdown into a clause list using loose structural conventions: 含"禁止/必须/不得/铁律/Red Flag"的语句为硬性条款，含"优先/推荐/尽量"的语句为建议条款，附录"禁止清单"中的条目一律为 red-flag。解析失败时 SHALL degrade to per-section review instead of erroring out. 报告开头 MUST 列出本次对照的条款清单摘要。

#### Scenario: 正常解析

- **WHEN** 宪法遵循松散结构约定
- **THEN** skill 输出条款清单，每条标注推导出的严重级（red-flag / violation / suggestion）

#### Scenario: 解析退化

- **WHEN** 宪法结构无法按约定切分条款
- **THEN** skill 按章节整体对照审查，并在报告中注明解析已退化，不报错退出

### Requirement: 默认审查范围为 git diff

By default the skill SHALL review `git diff HEAD`（未提交改动）。用户指定 base 引用时 SHALL 审查对应 diff（如 `main...HEAD`）；用户指定路径时 SHALL 对该路径做全量审查。全量审查大型仓库时 SHALL 按 crate/目录分批进行并汇总。

#### Scenario: 默认 diff 审查

- **WHEN** 用户触发 skill 且未指定范围
- **THEN** skill 审查 `git diff HEAD` 涉及的全部变更文件

#### Scenario: 指定路径全量审查

- **WHEN** 用户指定一个或多个路径
- **THEN** skill 对这些路径下相关源码做全量审查，大仓库时分批并汇总结论

### Requirement: 结构化分级报告

The skill SHALL output a report with: 审查依据（含宪法文件 commit sha）、审查范围、结论统计，以及 red-flag / violation / suggestion 三个分级小节。每条发现 MUST 包含条款引用、`文件:行号` 与代码摘录证据；无证据的发现 MUST NOT 列入报告。报告 MUST 单列 `constitution-gap` 小节，收录宪法未覆盖但值得关注的灰色问题，并提示用户补充宪法。

#### Scenario: 有违规

- **WHEN** 审查发现违反条款的代码
- **THEN** 报告按严重级分组列出，每条含条款编号、`文件:行号`、代码摘录与说明

#### Scenario: 无违规

- **WHEN** 审查未发现任何违规
- **THEN** 报告明确给出"未发现违规"结论，仍列出对照条款清单摘要，不编造问题

#### Scenario: 宪法未覆盖的灰色问题

- **WHEN** 发现不在任何条款覆盖范围内但值得注意的问题
- **THEN** 列入 `constitution-gap` 小节并标注建议补充宪法，不计入违规统计

### Requirement: 条款分类与检查路径

The skill SHALL classify each constitution clause before reviewing: clauses whose violations are mechanically decidable by `rustfmt` / `clippy` / `cargo-deny` SHALL be routed to a config-presence check（验证对应配置文件与 CI 步骤存在并启用，缺失记 `violation`）；all other clauses SHALL be routed to per-clause code review。报告因此只含两类发现：语义条款违规、静态门禁配置缺失。

#### Scenario: 机械条款走配置检查

- **WHEN** 宪法含"CI 必须运行 cargo audit"类机械可判定条款
- **THEN** skill 检查 `deny.toml` 与 CI workflow 对应步骤存在并启用，缺失记 `violation`，不扫描代码本身

#### Scenario: 语义条款走逐条对照

- **WHEN** 宪法含"Handler 不得直接调用 Repository"类语义条款
- **THEN** skill 在审查范围内逐处对照，输出含 `文件:行号` 证据的发现

### Requirement: Skill 可发现性

The skill's frontmatter `description` SHALL describe only triggering conditions（第三人称，"Use when..." 风格）and MUST NOT summarize the review workflow。关键词覆盖 SHALL 包含中英文触发词（constitution / 宪法 / compliance / 合规 / review / 审查）。

#### Scenario: 仅凭 description 命中

- **WHEN** 用户提出"对照项目宪法审查这次改动"类请求
- **THEN** agent 仅凭 description 即可判断应加载本 skill，无需阅读正文

### Requirement: 全程只读

The skill MUST NOT modify any file, run builds, or install dependencies. 证据收集 SHALL 仅使用文本搜索与文件读取（Grep / Read / git diff）。

#### Scenario: 审查过程无副作用

- **WHEN** skill 执行完整审查流程
- **THEN** 工作区文件、git 状态、依赖环境均不发生变化

### Requirement: 附带示例宪法模板

The skill SHALL bundle the Axum 版宪法 as `examples/CONSTITUTION.axum.md`，文件内 MUST 明确标注其为起步示例、非审查依据。

#### Scenario: 新项目起步

- **WHEN** 用户在没有宪法的项目中询问如何起步
- **THEN** skill 指向 `examples/CONSTITUTION.axum.md`，并说明应复制到项目根按项目实际修改后使用
