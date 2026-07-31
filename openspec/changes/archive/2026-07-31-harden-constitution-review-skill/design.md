# Design: harden-constitution-review-skill

## Context

初版 skill 经 RED-GREEN-REFACTOR 验证（见 archive/2026-07-31-add-constitution-review-skill/baseline-findings.md）。交付后复审发现三个规则空白，复测结论：

- **B（空 diff）**：两次 RED 复测（有提示 / 无提示）agent 均自行改用 `git show HEAD` 或全量审查，假阴性未复现。按 writing-skills"control 未表现失败则不加指引"原则，本可不修；但单行规则可消除该变异风险，作为 REFACTOR 固化观察到的正确行为，成本极低。
- **A（分类边界）**：初版验证中 agent 均自发采用"禁止清单→逐条对照、风格类→配置检查"的正确边界，但 skill 文本未写明，换个 agent/措辞可能把 red-flag 漏出审查路径。
- **C（分级兜底）**：模糊宪法（s3 场景）下 agent 全判 violation，结果合理但无规则依据。

## Goals / Non-Goals

**Goals:** 三条规则各加一句话，消除对 agent 临场判断的依赖；SKILL.md 保持 <500 词。

**Non-Goals:** 不改报告模板结构；不新增审查能力；不重跑完整基线（只对受影响场景做 GREEN 复测）。

## Decisions

- **D1（A 边界）**：分类谓词改为两级判定——先看条款是否属"禁止清单/铁律/Red Flag"（永远逐条对照），再看是否机械可判定（风格/工具链类→配置检查）。禁止清单的一票否决语义权重高于机械可判定性。
- **D2（B 空 diff）**：`git diff HEAD` 为空时，依次检查未推送提交（`git log @{u}..HEAD`）、与用户确认 base；报告中"审查范围"必须反映实际审查内容。空 diff 直接报"未发现违规"列入 Common Mistakes。
- **D3（C 兜底）**：无分级关键词命中的条款默认 `violation`，说明中注"分级依据不足"——宁高勿低，由人去降级，避免强制条款被静默降为 suggestion。

## Risks / Trade-offs

- [规则增多导致 SKILL.md 膨胀] → 三条各一句，总量仍 <300 词。
