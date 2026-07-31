# Proposal: harden-constitution-review-skill

## Why

对 `constitution-review` skill 做交付后复审时发现三个规则空白。复审后的 RED 复测结果：弱点 B（空 diff 假阴性）两次未复现——agent 均自行恢复正确范围，属于加固而非修复已观察失败；弱点 A（机械/语义分类边界）与 C（模糊宪法分级无兜底）在初版验证中靠 agent 临场发挥得到正确结果，需固化为规则，消除对运气的依赖。

## What Changes

- 条款分类规则补边界：宪法的禁止清单/铁律类条款永远走逐条对照（即使其内容可被 clippy 机械判定）；仅风格/格式化/工具链类条款走配置检查路径。
- 空 diff 保护：`git diff HEAD` 为空时禁止直接报"未发现违规"，必须检查未推送提交或与用户确认 base，并在报告中明示实际审查范围。
- 分级兜底：条款无法匹配任何分级关键词时默认记 `violation`，并在说明中注明"分级依据不足"。

## Capabilities

### New Capabilities

（无）

### Modified Capabilities

- `constitution-review`: 修改「条款分类与检查路径」（补禁止清单边界）、「默认审查范围为 git diff」（补空 diff 保护）、「条款解析与分级」（补分级兜底）三条 Requirement。

## Impact

- 修改：`.agents/skills/constitution-review/SKILL.md`（流程第 2、3 步 + 分级表 + Common Mistakes）。
- 修改主 spec：`openspec/specs/constitution-review/spec.md`（sync 后）。
- 无代码、无依赖变更。
