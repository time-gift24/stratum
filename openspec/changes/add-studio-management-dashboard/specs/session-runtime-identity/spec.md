## MODIFIED Requirements

### Requirement: AgentId 标识可复用的不可变 Template 版本
每个 `AgentId` 必须（SHALL）永久标识 `agents` 中的一条 immutable definition 版本 row；row 由作者命名的 exact `(name, version string tag)` 唯一定位。version tag 必须（SHALL）来自 Studio Agent definition，大小写敏感且没有排序语义；create request 不得（SHALL NOT）指定或覆盖它。

exact pair 已存在且 `definition_schema_version + canonical resolved_definition` 严格相同时必须（SHALL）复用原 `AgentId`，定义不同时必须（SHALL）返回 `agent_version_conflict` 且不得覆盖；不同 tag 即使定义相同也必须（SHALL）创建新 `AgentId`。同一 `AgentId` 可以（SHALL）被多个 AgentRuntime pin；后续 runtime state、Turn model override 与 Studio authoring definition 变化不得（SHALL NOT）改写 immutable definition。

#### Scenario: 同一 Definition 版本被多个 Runtime 复用
- **WHEN** 两个不同 create key 读取 same name/tag 且 canonical definition 相同
- **THEN** 它们获得不同 `AgentRuntimeId` 但 pin 同一 `AgentId`

#### Scenario: Prompt 与 Tools 的恢复来源
- **WHEN** API 外层开始新 Turn 或 resume 既有 Turn
- **THEN** prompt 与 ordered tools 来自 state pinned `AgentId` 的 immutable definition，不从当前 Studio authoring row 或另一 runtime 复制

#### Scenario: Mutable Model 不回写 Definition
- **WHEN** 后续 Turn 成功更新某个 runtime 的 `model_config`
- **THEN** 只有该 `agent_states` row 变化，immutable definition 与共享它的其他 runtime 保持不变

#### Scenario: 作者复用 Tag 修改定义
- **WHEN** exact name/tag 已存在但当前 Studio definition 的 canonical 内容不同
- **THEN** create 返回 `agent_version_conflict`，既有 AgentId 与所有 runtime pin 不变
