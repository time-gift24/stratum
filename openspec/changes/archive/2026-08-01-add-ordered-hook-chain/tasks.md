## 1. stratum-core：链版本事件字段

- [x] 1.1 `DurableAgentEvent::LoopStarted` 新增可选 `extension_set_version_id` 字段（`#[serde(default)]` + `skip_serializing_if`，旧日志可解析），`event_type` 与序列化测试同步

## 2. stratum-tools：统一 schema 校验边界

- [x] 2.1 workspace 引入 `jsonschema` 依赖（根 Cargo.toml，crate 用 `workspace = true`），`deny.toml` 许可证确认
- [x] 2.2 新增 schema 校验模块：注册时编译缓存 `input_schema`（非法 schema 注册即拒绝），`validate_against_schema` 输出类型化 `InvalidArgument`
- [x] 2.3 `BuiltinToolRegistry::validate` 切换：schema 校验先行，通过后再走 `Tool::validate` 自定义语义校验；既有内置工具的非法输入用例在新边界下仍被拒绝（测试固化）

## 3. stratum-agent：HookHandler 与 ChainHookRuntime

- [x] 3.1 `hook_runtime` 新增 `HookHandler` trait（五方法默认 No-op、`descriptor()` 返回 `HookHandlerVersionId` 身份）与 `HookHandlerDescriptor`，分文件放置并从 crate 导出
- [x] 3.2 `ChainHookRuntime` 实现 `HookRuntime`：构造时固定有序 `Vec<Arc<dyn HookHandler>>` 并计算 `ExtensionSetVersionId`；实现五点链语义（顺序变换 Cow 线程化、Block 短路、Stop 短路丢弃已收集 Inject、Inject 有序合并、Handler 失败 fail closed）
- [x] 3.3 `HookRuntime` 增加默认返回 `None` 的 `extension_set_version()`；`AgentLoopBuilder` 读取并随 `LoopStarted` 提交；resume 时与事件流版本比对，不匹配 fail closed
- [x] 3.4 确认 kernel 复验点覆盖整条 transform 链的最终参数（现状语义不变，测试固化）

## 4. 测试

- [x] 4.1 链语义：顺序变换线程化（transform_context/args/result 三类）、Block 短路（后续 Handler 未被调用）、Stop 短路丢弃 Inject、Inject 有序合并、Handler 失败中断链
- [x] 4.2 版本固定：同序构造版本一致；resume 版本不匹配 fail closed；无版本 runtime 跳过校验
- [x] 4.3 链与既有合同互操作：链作为 runtime 注入后，既有 hook 测试语义不变（No-op 等价、journal 写入、deadline/取消）
- [x] 4.4 schema 边界：类型/必填/约束拒绝、schema 通过后自定义校验仍生效、非法 schema 注册拒绝、Hook 改参后复验拦截

## 5. 文档、质量门禁与校验

- [x] 5.1 归档 `crates/stratum-agent/AGENTS.md`（HookHandler/ChainHookRuntime、链语义、版本固定）与 `crates/stratum-tools/AGENTS.md`（schema 校验边界）
- [x] 5.2 勾选 `TODO.md` H2 剩余条目（链顺序、短路、统一校验边界、处理器顺序固化）
- [x] 5.3 运行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`
- [x] 5.4 运行 `openspec validate add-ordered-hook-chain --type change --strict --no-interactive` 与 `openspec validate --all --strict`
