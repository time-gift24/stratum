## ADDED Requirements

### Requirement: Runtime version 是不可变身份
系统必须（SHALL）使用不同的、经过校验的身份类型表示 Agent、Workflow、有序 SkillSet 与有序 ExtensionSet 的版本。已发布版本在发布后不得（SHALL NOT）改变其行为或 Handler 顺序。

#### Scenario: ExtensionSet 顺序发生变化
- **WHEN** 相同的一组 Handler 以不同顺序发布
- **THEN** 系统分配或解析出不同的 `ExtensionSetVersionId`

#### Scenario: Session 使用后续版本
- **WHEN** 同一 Session 中后续 Turn 选择更新的 Agent 或 ExtensionSet 版本
- **THEN** Session 身份保持不变，新 Turn 记录更新后的版本

### Requirement: 可恢复 Turn 固定其 runtime snapshot
在接受一个可恢复 Agent Turn 之前，系统必须（SHALL）持久化该 Turn 使用的已解析 Agent version、model 配置、ToolSet fingerprint、SkillSet version、ExtensionSet version 和精确 Handler 顺序。

#### Scenario: 已发布版本变化后恢复
- **WHEN** 新的 Agent、SkillSet 或 ExtensionSet 版本发布后，已有 Turn 执行恢复
- **THEN** 系统解析原 Turn 固定的版本与 Handler 顺序

#### Scenario: 固定的 runtime 不可用
- **WHEN** 无法解析原 Turn 固定的 runtime component
- **THEN** 在调用 model、Tool 或 Hook 之前，恢复操作 fail closed

### Requirement: Hook invocation 标识一次 Handler 调用
一个 Hook point 上的每次 Handler 调用必须（SHALL）获得不同的 `HookInvocationId`。持久化 invocation record 必须（SHALL）将该 ID 与 Session、Agent、Turn、Hook point、Handler 位置、不可变 Handler 版本、operation identity 和 input digest 绑定。

#### Scenario: 一个 Hook point 上有多个 Handler
- **WHEN** 三个有序 Handler 处理同一个 Hook point
- **THEN** 系统按 Handler 顺序创建三个不同的 Hook invocation 身份

#### Scenario: 同一个 Handler 位于不同 Hook point
- **WHEN** 一个 Handler 在同一 Turn 的两个 Hook point 上参与处理
- **THEN** 每个 Hook point 使用不同的 Hook invocation 身份

### Requirement: 调用前提交 Hook journal
Hook runtime 必须（SHALL）在调用 Handler 之前持久化提交 `Pending` invocation record，并且必须（SHALL）在执行受 decision 影响的动作之前持久化提交已完成的 decision。

#### Scenario: Handler 调用前崩溃
- **WHEN** 进程在提交 `Pending` 之后、调用 Handler 之前停止
- **THEN** 恢复时找到已有的 invocation 身份，并且不创建第二个逻辑 invocation

#### Scenario: Decision 提交后崩溃
- **WHEN** 进程在提交已完成 decision 之后、应用该 decision 之前停止
- **THEN** 恢复时复用已提交的 decision，不再次调用 Handler

### Requirement: Hook 恢复采用 fail-closed
恢复时必须（SHALL）复用完全匹配的已完成 invocation result，必须（SHALL）保留终态失败结果，并且必须（SHALL）在继续 Turn 之前拒绝语义地址、Handler version 或 input digest 的任何不匹配。

#### Scenario: 已完成 invocation 匹配
- **WHEN** 持久化 invocation 的版本与 input digest 和恢复后的 Hook 输入相匹配
- **THEN** 复用持久化 decision

#### Scenario: Input digest 不同
- **WHEN** 语义 invocation 地址已存在，但恢复后的 input digest 不同
- **THEN** Turn fail closed，且不调用 Handler

#### Scenario: 已存在终态 Hook 失败
- **WHEN** 某次 Hook invocation 先前已经进入终态 failed 或 timed-out 状态
- **THEN** 恢复时重现类型化失败，而不是静默重试

### Requirement: Pending invocation 重试保持幂等键
进程崩溃遗留的 `Pending` invocation 只能（SHALL）使用其原始 `HookInvocationId` 和固定的 Handler version 重试。可被重试的 Handler 对该 invocation 身份必须（SHALL）无副作用或具备幂等性。

#### Scenario: 远程 Handler 结果未提交
- **WHEN** 远程 Handler 可能已经完成，但其结果尚未持久化提交
- **THEN** 恢复时使用原始 invocation 身份调用固定的 Handler

### Requirement: 影响 decision 的 Hook 错误必须类型化并 fail closed
影响 decision 的 Hook 错误、超时、取消、无效输出、不兼容协议版本和不可用的固定 Handler 必须（SHALL）产生类型化失败，并且必须（SHALL）阻止受影响的 model、Tool、message 或 iteration action 继续执行。

#### Scenario: Before-tool Handler 返回无效输出
- **WHEN** before-tool Handler 返回无法校验的 decision
- **THEN** Tool 不会被批准或执行，Turn 收到类型化 Hook 失败

### Requirement: Hook journal 与历史及观测分离
Hook invocation 状态必须（SHALL）属于 Session/Turn 执行状态。它不得（SHALL NOT）存储为 Agent 对话历史，也不得（SHALL NOT）根据 EventBus 观测重建。

#### Scenario: Event retention 到期
- **WHEN** EventBus 中保留的 Hook 观测事件到期
- **THEN** Hook 的持久化恢复行为保持不变

### Requirement: Hook 信任边界取决于 Extension 形式
runtime 必须（SHALL）按信任等级对 Skill 内容、Script Extension、链接式 Rust Hook 和远程 Hook Service 分类，并且必须（SHALL）执行相应形式的最小边界。

#### Scenario: Skill 请求额外 Tool 权限
- **WHEN** Skill 内容引用 Agent 无权使用的 Tool
- **THEN** 加载该 Skill 不会授予缺失的权限

#### Scenario: Script Extension 在 production mode 运行
- **WHEN** 调用不可信的 Script Extension
- **THEN** 它在 Agent 进程之外运行，使用显式 capability，并受到时间、内存、输出和并发限制

#### Scenario: 选择链接式 Rust Hook
- **WHEN** 链接式 Rust Hook 参与 Turn
- **THEN** 它被视为完全可信的进程内代码，且其 runtime 兼容性为恢复而固定

#### Scenario: 调用远程 Hook Service
- **WHEN** Hook Service 处理 invocation
- **THEN** 系统强制执行传输认证、tenant/project 授权、固定 service 身份与版本、输入输出限制和 invocation 幂等性

### Requirement: 保护 Hook 载荷与错误
Hook 日志、trace、audit record 和公开错误必须（SHALL）省略 prompt、原始 Tool 参数、Tool 结果、secret、credential、extension 输出和 host 路径，除非未来明确的数据策略授权某个具体字段。

#### Scenario: Hook invocation 失败
- **WHEN** Handler 返回包含敏感输入的内部错误
- **THEN** 公开失败只暴露安全的类型化分类，不包含敏感载荷
