# Proposal: add-constitution-review-skill

## Why

团队已在迭代中沉淀出一份项目级 `CONSTITUTION.md`（本次以 Rust + Axum 生产级 Web 服务为引发场景，含架构分层、错误处理、API 规范、日志追踪、数据库、安全、测试、部署、代码风格、禁止清单十节）。但目前没有任何机制保证 AI 或人类编码时真正遵守它——宪法写在文档里，review 全靠人记。需要一个可复用的 agent skill，把"对照宪法审视代码"变成一步可触发的标准动作，输出有据可查的违规报告。

注意：本次生成的 Axum 版宪法只是一个"引发"示例，不是最终目标。Skill 必须是通用机制——读取目标项目根目录的 `CONSTITUTION.md`，条款内容由各项目自己维护。

## What Changes

- 新增项目级 skill：`.agents/skills/constitution-review/SKILL.md`，触发后对照目标项目根目录的 `CONSTITUTION.md` 逐条审视代码，输出结构化审查报告（条款引用 + 违规证据 `文件:行号` + 严重级别）。
- 审查范围默认为当前 git diff（工作区相对 HEAD 或指定 base），支持用户指定路径做全量审查。
- Skill 只读、只报告，不修改任何代码；不内嵌宪法条款，条款永远以项目根 `CONSTITUTION.md` 为准。
- 随 skill 附带一份示例宪法模板（本次生成的 Axum 版，标注为 example，供没有宪法的项目复制起步）。
- 报告格式统一为三个严重级：`red-flag`（宪法禁止清单，一票否决）、`violation`（违反强制条款）、`suggestion`（偏离推荐实践）。
- 静态/机械类检查（代码格式、clippy lint、依赖审计）不进入 skill 审查范围，交由 `rustfmt.toml`、`.clippy.toml`、`deny.toml` 等配置文件 + CI 承担；skill 只验证这些配置文件按宪法要求存在并启用。

非目标：
- 不做自动修复，不在报告中直接改代码。
- 不做 CI 集成（本次只交付 skill 本身；接入 pre-commit / GitHub Actions 是后续独立 change）。
- 不覆盖前端审查（本仓库前端已有 impeccable / DESIGN.md 体系）。

## Capabilities

### New Capabilities

- `constitution-review`: 一个 agent skill，读取项目根 `CONSTITUTION.md`，对 git diff 或指定路径的代码执行逐条合规审视，输出分级违规报告（red-flag / violation / suggestion），含条款引用与 `文件:行号` 证据，全程只读。

### Modified Capabilities

（无）

## Impact

- 新增目录：`.agents/skills/constitution-review/`（`SKILL.md` + `examples/CONSTITUTION.axum.md`）。
- 不修改任何 crate、Cargo.toml 或现有代码；零新依赖。
- 与现有 `.agents/skills/rust-skills/` 关系：rust-skills 管通用 Rust 编码规则，constitution-review 管项目自定义宪法的合规审查，两者互补不重叠。
