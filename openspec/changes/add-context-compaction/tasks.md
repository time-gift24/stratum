## 1. stratum-core：事件与 decision 记录

- [x] 1.1 `DurableAgentEvent` 新增 `TranscriptCompacted { upto, summary, compacted_iteration }` 变体（snake_case 稳定序列化、`event_type()` 投影、手写 Deserialize 前门的 wire 枚举同步 + 往返测试）
- [x] 1.2 `HookDecisionRecord` 的 prepare 记录新增 Compact 表示（upto + summary），序列化测试锁定

## 2. stratum-agent：Compact decision 与 kernel 压缩执行

- [x] 2.1 `PrepareNextTurnDecision` 新增 `Compact { upto, summary }` 变体与 `check()` 校验：upto 非 0；summary 必须是 system 角色、无 tool_calls/tool_call_id/reasoning_content
- [x] 2.2 kernel 在迭代边界执行压缩：跟踪当前迭代起始消息下标；校验 upto 不越界、不切断 tool_call/result 配对、不切入当前迭代；先提交 `TranscriptCompacted` 再提交迭代边界；committed context 前缀替换为摘要标记消息（标记文本模板写入 crate 文档）
- [x] 2.3 压缩后 Hook 快照自然呈现压缩基线（transform_context / prepare_next_turn 无需特判）；摘要标记消息计入 `LoopOutcome.new_messages`
- [x] 2.4 `ChainHookRuntime` 的 prepare 链语义扩展：Compact 短路（同 Stop——压缩后没有"照常继续"），与 Inject/Stop 的组合行为定义并测试（Compact 出现即定案，丢弃已收集 Inject）

## 3. stratum-agent：resume 与崩溃窗口

- [x] 3.1 重放应用 `TranscriptCompacted`：按事件顺序将已重建前缀替换为摘要标记消息，支持多次压缩
- [x] 3.2 崩溃窗口闭合：Completed(Compact) 已提交而 `TranscriptCompacted` 未提交时，resume 从 journal 回放 decision 并以记录摘要执行压缩，不再调用 Handler

## 3b. stratum-infra：压缩检查点索引

- [x] 3.3 `FilesystemDurableEventSink` 在 `TranscriptCompacted` 落盘后向 `compact.jsonl` 追加检查点（`compacted_iteration`、`event_line`、`upto`、`summary_digest`），写入顺序不可逆（先事件流后索引）
- [x] 3.4 resume 快速路径：读取 `LoopStarted` 与最新检查点，校验事件流对应行后从该行起重放；索引缺失/损坏/不匹配回退全量重放（不 fail closed）

## 4. 测试

- [x] 4.1 校验矩阵：upto=0、越界、切断配对、切入当前迭代、非 system 角色/带 tool identity 的 summary 全部判 InvalidOutput 且不提交任何事件
- [x] 4.2 压缩执行：TranscriptCompacted 先于迭代边界提交；压缩后下一次模型请求的 context 是压缩基线；new_messages 含摘要标记；transform 快照首条为 kernel 标记消息（handler 可识别已压缩）
- [x] 4.3 链语义：Compact 短路、与 Inject/Stop 的组合矩阵
- [x] 4.4 resume：重放应用单次/多次压缩；崩溃窗口回放（摘要不二次生成、Handler 不被调用）；压缩前 journal 记录压缩后仍可匹配
- [x] 4.5 检查点索引：快速路径与全量重放结果一致；索引缺失/截断/校验不匹配回退全量；事件已落盘而索引未写的崩溃窗口行为正确
- [ ] 4.6 constitution-review：对照根 CONSTITUTION.md 派发子代理分条款审查本 change 完整 diff，修复全部 red-flag 与 violation

## 5. 文档、质量门禁与校验

- [x] 5.1 归档 `crates/stratum-agent/AGENTS.md`（Compact 合同、压缩不变量、标记消息模板、崩溃窗口回放）与 stratum-core 相关约定
- [x] 5.2 勾选 `TODO.md` 的 H5 条目
- [x] 5.3 运行 `cargo fmt --check`、`cargo check --workspace --all-targets`、`cargo clippy --workspace --all-targets`、`cargo test --workspace --all-targets`
- [x] 5.4 运行 `openspec validate add-context-compaction --type change --strict --no-interactive` 与 `openspec validate --all --strict`
