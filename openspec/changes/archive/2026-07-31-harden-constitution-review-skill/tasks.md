# Tasks: harden-constitution-review-skill

## 1. SKILL.md 修改

- [x] 1.1 流程第 2 步（条款分类）改为两级判定：禁止清单/铁律类永远逐条对照；其余机械可判定（风格/工具链类）走配置检查
- [x] 1.2 流程第 3 步（确定范围）补空 diff 保护：为空时检查未推送提交或与用户确认 base，报告明示实际范围，禁止以空 diff 报"未发现违规"
- [x] 1.3 分级表补兜底：无关键词命中的条款默认 `violation` 并注明"分级依据不足"
- [x] 1.4 Common Mistakes 补一行：空 diff 直接报"未发现违规" → 先确认实际审查范围

## 2. GREEN 复测

- [x] 2.1 空 diff 场景（已提交改动、无提示 prompt）复测：确认报告明示实际范围且不报假阴性
- [x] 2.2 精确宪法场景复测：确认禁止清单条目仍走逐条对照（不错位到配置检查），无回归
- [x] 2.3 模糊宪法场景复测：确认兜底分级生效（默认 violation + 注明依据不足）

## 3. 质量与归档

- [x] 3.1 `wc -w SKILL.md` 仍 < 500 词（254）
- [x] 3.2 运行 `openspec validate --all --strict`
- [x] 3.3 sync spec 到主 specs 并归档 change
