# tool-input-validation Specification（add-ordered-hook-chain）

## ADDED Requirements

### Requirement: Schema 是工具参数的统一校验权威
`stratum-tools` 必须（SHALL）提供以 `ToolSpec.input_schema` 为权威的 JSON Schema 校验边界。注册表必须（SHALL）在注册时编译并缓存每个工具的 schema，校验调用必须（SHALL）先执行 schema 校验，再执行工具自定义的语义校验。schema 校验失败必须（SHALL）产生类型化 `InvalidArgument` 错误，且不进入工具自定义校验或任何外部副作用。

#### Scenario: Schema 拒绝非法输入
- **WHEN** 工具输入违反其 `input_schema`（类型错误、缺少必填字段、违反约束）
- **THEN** 注册表以类型化 InvalidArgument 拒绝，工具的自定义 validate 与执行均不发生

#### Scenario: Schema 通过后执行自定义校验
- **WHEN** 工具输入通过 schema 校验
- **THEN** 注册表继续调用工具自定义的 validate 逻辑，其拒绝语义保持现状

#### Scenario: 非法 Schema 在注册时被拒绝
- **WHEN** 注册的工具携带无法编译的 input_schema
- **THEN** 注册以类型化错误失败，该工具不会进入注册表

### Requirement: Hook 修改后的参数经过同一 Schema 边界复验
AgentLoop 的原始参数校验与 transform 链后的最终复验必须（SHALL）使用同一个 schema 校验边界；Hook 链修改后不满足 schema 的参数不得（SHALL NOT）进入 decide 相位、审批或 Tool 执行。

#### Scenario: 链后复验拦截非法参数
- **WHEN** transform 链输出的最终参数不满足工具 input_schema
- **THEN** AgentLoop 生成校验错误结果，不进入 decide_tool_call、不提交 ToolExecutionStarted、也不调用 Tool

#### Scenario: 审批所见参数通过 Schema
- **WHEN** decide 相位的 Handler（含审批 Handler）检查工具调用
- **THEN** 其看到的参数已通过最终 schema 复验，与实际执行的参数一致
