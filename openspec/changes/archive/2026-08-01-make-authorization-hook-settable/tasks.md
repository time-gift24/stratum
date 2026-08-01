## 1. 可写授权合同（已随 PR #38 实现）

- [x] 1.1 `TransformToolCallDecision` 改为 `Continue | Modify(TransformToolCallModification)`，`arguments` 与 `authorization` 均可选，双 `None` 判 `HookFailure::InvalidOutput`
- [x] 1.2 新增 `AuthorizationOverride`（`PreAuthorize` / `Set { kind, danger }`）与公共导出，类型文档明确"覆写是 handler 明示责任，kernel 不做含降级的合理性检查"
- [x] 1.3 kernel 计算生效授权并无分支地搬运到 `decide_tool_call` 与 `after_tool_call`；`ToolHookTarget.authorization` 文档改为生效值语义

## 2. execute 收窄（已随 PR #38 实现）

- [x] 2.1 `ToolExecutor::execute` 收窄为 `pub(crate)`，参数为已解析工具句柄与最终 call，函数体只剩取消检查、`ToolExecutionStarted` 耐久提交与 dispatch
- [x] 2.2 删除 execute 内的授权查询与校验，消除 kernel 路径的重复查询

## 3. 测试（已随 PR #38 实现）

- [x] 3.1 Set 覆写贯通 decide/after、PreAuthorize 抹除、空 Modify 判非法、参数与授权同改、validate 单测
- [x] 3.2 既有 hooks 测试前向切换，executor 内联测试适配新签名

## 4. 文档与归档

- [x] 4.1 `crates/stratum-agent/AGENTS.md` 归档生效授权语义、hook_lookup 职责与 execute 纯机制边界
- [x] 4.2 运行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`（28 套件 501 通过 0 失败）
- [x] 4.3 运行 `openspec validate make-authorization-hook-settable --type change --strict --no-interactive` 与 `openspec validate --all --strict`，随后归档同步 canonical spec
