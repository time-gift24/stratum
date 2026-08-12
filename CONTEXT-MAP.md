# Context Map

## Contexts

- [Agent Kernel](./CONTEXT.md) — 驱动模型与工具迭代，并通过事件流和 journal 保证耐久恢复

## Relationships

- **Agent template version → AgentRuntime**：一个不可变 Agent template 版本可以被多个相互隔离的 AgentRuntime 复用；每个 AgentRuntime 在整个生命周期内固定同一版本。
- **AgentRuntime → Agent Kernel**：组合层把固定的 Agent 定义和当前 Turn 输入交给 kernel；运行聚合身份、持久化分区、hosting 与 transport 不进入 kernel 状态机。
