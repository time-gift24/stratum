---
name: constitution-review
description: Use when reviewing code against a project's CONSTITUTION.md — 宪法审查、合规检查、constitution compliance review、对照项目宪法审 diff 或指定路径
---

# Constitution Review

## Overview

对照目标项目根目录的 `CONSTITUTION.md` 审查代码，输出分级违规报告。条款永远以项目根文件为准，本 skill 不含任何条款。默认只读：不改代码、不跑构建、不装依赖；仅当用户明确要求修复时才修改。

## 流程

1. **定位宪法**：找项目根 `CONSTITUTION.md`。不存在 → 停止并告知，提示可复制 `examples/CONSTITUTION.axum.md` 到项目根起步。不凭空编造条款。
2. **条款分类**：两级判定：
   - 条款属"禁止清单 / 铁律 / Red Flag"类 → 永远走**逐条对照**（即使内容可被 clippy 判定，一票否决的语义权重高于机械可判定性）。
   - 其余条款中，违规能被 rustfmt / clippy / cargo-deny 机械判定的（风格、格式化、工具链类）→ 走**配置检查**：验证 `rustfmt.toml` / `.clippy.toml` / `deny.toml` / CI workflow 对应步骤存在并启用，缺失记 `violation`。不审查代码本身。
   - 其余语义条款（分层、事务边界、敏感数据等）→ 走**逐条对照**。
   解析不出条款时退化为按章节对照，并在报告中注明。
3. **确定范围**：默认 `git diff HEAD`；用户指定 base 用对应 diff；指定路径做全量，大仓库按 crate/目录分批再汇总。`git diff HEAD` 为空时**不得**报"未发现违规"——先检查未推送提交（`git log @{u}..HEAD`）或与用户确认 base，报告中明示实际审查范围。
4. **审查**：逐条对照语义条款。每条发现必须有 `文件:行号` + 代码摘录，无证据不列入；推断性发现显式标注"推断"。

## 分级

| 级别 | 来源 |
|---|---|
| `red-flag` | 宪法"禁止清单 / Red Flag / 铁律"类条款 |
| `violation` | 含"禁止 / 必须 / 不得"的强制条款；机械条款的配置缺失；兜底：无法匹配任何分级关键词的条款（注明"分级依据不足"） |
| `suggestion` | 含"优先 / 推荐 / 尽量"的建议条款 |

## 报告模板

```
## Constitution Review Report
- 审查依据: CONSTITUTION.md (<commit sha 前 8 位>)
- 审查范围: <git diff HEAD (N files) | 指定路径>
- 结论: X red-flag / Y violation / Z suggestion

### 条款覆盖
<已对照条款清单；未审条款及原因（如无相关文件）。必填，不得省略>

### Red Flags / Violations / Suggestions
| 条款 | 位置 | 证据 | 说明 |

### Constitution Gap
<宪法未覆盖但值得关注的问题，提示补宪法，不计入统计>
```

无违规时明确写"未发现违规"，不编造问题。

## Common Mistakes

| 错误 | 纠正 |
|---|---|
| 把 clippy/rustfmt 能查的问题写进报告 | 该条款走配置检查路径；报告只含语义违规与配置缺失 |
| 压力下跳过部分条款且不声明 | 条款覆盖清单必填，未审条款列原因 |
| 审查时顺手修代码 | 默认只读；用户明确要求才修复 |
| 推断写成事实 | 无证据不列入；推断显式标注 |
| 空 diff 直接报"未发现违规" | 先确认实际审查范围：未推送提交（`git log @{u}..HEAD`）或与用户确认 base |
