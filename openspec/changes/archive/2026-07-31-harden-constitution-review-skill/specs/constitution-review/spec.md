# Spec: constitution-review (harden delta)

## MODIFIED Requirements

### Requirement: 条款解析与分级

The skill SHALL parse the constitution Markdown into a clause list using loose structural conventions: 含"禁止/必须/不得/铁律/Red Flag"的语句为硬性条款，含"优先/推荐/尽量"的语句为建议条款，附录"禁止清单"中的条目一律为 red-flag。无法匹配任何分级关键词的条款 SHALL default to `violation`，并在该条发现的说明中注明"分级依据不足"。解析失败时 SHALL degrade to per-section review instead of erroring out. 报告开头 MUST 列出本次对照的条款清单摘要。

#### Scenario: 正常解析

- **WHEN** 宪法遵循松散结构约定
- **THEN** skill 输出条款清单，每条标注推导出的严重级（red-flag / violation / suggestion）

#### Scenario: 分级兜底

- **WHEN** 某条款不含任何分级关键词（如模糊宪法中的"错误处理要合理"）
- **THEN** 该条款的违规记为 `violation`，说明中注明"分级依据不足"

#### Scenario: 解析退化

- **WHEN** 宪法结构无法按约定切分条款
- **THEN** skill 按章节整体对照审查，并在报告中注明解析已退化，不报错退出

### Requirement: 默认审查范围为 git diff

By default the skill SHALL review `git diff HEAD`（未提交改动）。用户指定 base 引用时 SHALL 审查对应 diff（如 `main...HEAD`）；用户指定路径时 SHALL 对该路径做全量审查。全量审查大型仓库时 SHALL 按 crate/目录分批进行并汇总。当 `git diff HEAD` 为空时，the skill MUST NOT report "未发现违规"——SHALL 检查未推送提交（如 `git log @{u}..HEAD`）或与用户确认 base，并在报告的审查范围中明示实际审查的内容。

#### Scenario: 默认 diff 审查

- **WHEN** 用户触发 skill 且未指定范围
- **THEN** skill 审查 `git diff HEAD` 涉及的全部变更文件

#### Scenario: 空 diff 保护

- **WHEN** 默认范围的 `git diff HEAD` 为空（改动已提交或工作区干净）
- **THEN** skill 检查未推送提交或与用户确认 base，按确认后的范围审查，并在报告中明示实际范围；不得以空 diff 为由报"未发现违规"

#### Scenario: 指定路径全量审查

- **WHEN** 用户指定一个或多个路径
- **THEN** skill 对这些路径下相关源码做全量审查，大仓库时分批并汇总结论

### Requirement: 条款分类与检查路径

The skill SHALL classify each constitution clause before reviewing. 分类按两级判定：条款属"禁止清单 / 铁律 / Red Flag"类时 SHALL 永远走逐条对照（即使其内容可被 clippy 等工具机械判定，一票否决的语义权重高于机械可判定性）；其余条款中，违规能被 `rustfmt` / `clippy` / `cargo-deny` 机械判定的（风格、格式化、工具链类）SHALL be routed to a config-presence check（验证对应配置文件与 CI 步骤存在并启用，缺失记 `violation`）；all other clauses SHALL be routed to per-clause code review。报告因此只含两类发现：语义条款违规、静态门禁配置缺失。

#### Scenario: 禁止清单永远走逐条对照

- **WHEN** 宪法禁止清单含"禁止 println!"等同时可被 clippy 判定的条目
- **THEN** 该条目仍在审查范围内逐处对照代码，不错位到配置检查路径

#### Scenario: 机械条款走配置检查

- **WHEN** 宪法含"CI 必须运行 cargo audit"类机械可判定条款（非禁止清单条目）
- **THEN** skill 检查 `deny.toml` 与 CI workflow 对应步骤存在并启用，缺失记 `violation`，不扫描代码本身

#### Scenario: 语义条款走逐条对照

- **WHEN** 宪法含"Handler 不得直接调用 Repository"类语义条款
- **THEN** skill 在审查范围内逐处对照，输出含 `文件:行号` 证据的发现
